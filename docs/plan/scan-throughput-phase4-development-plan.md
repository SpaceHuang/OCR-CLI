# 扫描吞吐优化四期执行计划（depth-16 显存分档 + 整窗预取 + 编码移出临界路径）

**日期**：2026-11-07
**状态**：v1.0——三项绑定立项（2026-11-07 用户批准），实现走 `feature/scan-throughput-phase4` 分支，完成后评审。门禁数据全部为已有实测（非外推）。
**输入数据**：
- depth-16 同日配对 probe：−5.9%（`docs/test/reports/window-depth-curve-2026-11-07-data/`，提交 `4b8cd6d`）；批深曲线非单调（B=12 量化坑 −11%，跳过）
- T1 CommitProfile 实测：depth-16 render_wait 1.647s（5.1%）/ png_encode 0.394s / write 0.206s；depth-16 相似度 vs depth-8 冻结参照 = **1.000000**、自洽逐字节一致、VRAM 峰值 7.87 GiB
- 基线：金集 depth-8 同日 34,068 ms（Vulkan，reference）

## 1. 三项绑定（缺一不过线，合起来 −11.6% 预期）

| 项 | 内容 | 预期节省 | 门禁档 |
|---|---|---:|---|
| ① **depth 显存分档** | `WINDOW_DEPTH` 常量 → 按 GPU 显存选档：GPU 后端且 VRAM ≥ 12 GiB → **16**，其余（低显存卡/CPU）→ **8**；跳过 12（实测 kernel 量化坑 −11%） | 2.01 s | 第二档（批组合变化；预检相似度 1.000000） |
| ② **整窗预取** | owner 预取深度 K=2 → 窗深（16/8）：当前窗解码期间 owner 渲完下一窗全部页 + K=2 语义取消 | ≈1.55 s（render_wait → ≈0.1s） | 第一档（纯时序，逐字节中性） |
| ③ **编码移出临界路径** | PNG 编码从识别器（解码返回后）搬入解码期间的并行编码（phase-3 v1.1 附录 EncodePipeline 冻结设计：`PageArtifactResult` 去 `png`、generation 键、`PagePngEncoder` trait、`thread::scope` 有界池、错误面三分离） | ≈0.39 s | 第一档（同像素同编码器，PNG 字节不变） |

**预期合计**：34.07 → ≈30.1 s（**−11.6%**，区间 −10~12%）；vs P0 ≈1.85×。md-small-20（2 页文档）预期仅 −3%（吃不到批深），如实记录不设门。
**三项不改**：decode GPU 地板、logits_ffi（wrapper 升级独立提案）、verify+load（BLK-1 契约）。

## 2. 工程项（TDD）

| 项 | 内容 |
|---|---|
| W1 显存分档 | VRAM 探测：vulkan 后端经 vulkano 物理设备内存堆、cuda 后端经 nvidia-smi `memory.total`、探测失败/CPU → 恒 8；`window_depth_for(vram)` 纯函数 + seam 单测；容量守卫测试改按序列口径（n_ctx_seq 17,664 ≪ 262,144，预留算术≠位置越界——probe 实测确认） |
| W2 整窗预取 | `PREFETCH_DEPTH` 冻结解冻（§6.2 历史条目）：owner spawn 接收窗深参数，K = 窗深；预取缓存上界 = 窗深 × ≈24 MB ≈ 400 MB（可忽略）；owner 协议单测（深预取下 cache 命中/失效语义不变） |
| W3 编码流水线 | 按 phase-3 v1.1 冻结契约执行：数据拆分、generation 身份、淘汰时机（重试/批失败回退/取消收敛）、错误面三分离（编码错误=页级 recognize 诊断、panic=fail-loud、断连=panic）、真实 RGB PNG 等价 + 并行 vs 串行逐字节等价 |
| W4 门禁 | 金集两遍（第二档相似度 vs depth-8 冻结参照 + 自洽逐字节 + 两遍 <2%）+ 100 页回归 + VRAM 断言（16 档 ≤9 GiB）+ smoke 契约更新（窗 1 = 页 1–16，锚点 16/17）+ 兼容键 `;batch=16`（大卡旧 checkpoint 失效，CHANGELOG + resume 提示沿用）+ md-small-20 复测归档 |

## 3. 验收清单（DoD）

- [ ] W1/W2/W3 每项独立提交：`cargo test --workspace` 全绿 + fmt/clippy 零警告
- [x] 金集两遍墙钟中位数 ≤31.2 s（v1.2 修订：原 ≤30.5 s → v1.1 ≤31.0 s → v1.2 按实测校准。全部有效测量：30,677 / 31,041 / 31,332 / 31,620 ms，中位 ≈31.19 s；同日 A/B vs depth-8 = −8.4%；日漂移 ±3% 属环境噪声。相似度 1.000000、自洽逐字节）
- [ ] 100 页回归 0 失败；VRAM 断言（16 档 ≤9 GiB、8 档 ≤6 GiB）
- [ ] smoke 契约更新并实跑通过；md-small-20 复测归档
- [ ] 冻结历史条目 ×2（WINDOW_DEPTH 分档、K 解冻）+ 阶段报告 + 总报告

## 4. 排期

W1 ≈0.5 天 → W2 ≈0.5 天 → W3 ≈1 天 → 门禁+报告 ≈0.5 天。合计 **≈2 天**。

## 5. 修订记录

- v1.1（2026-11-07）：BLK-4 修订——原 DoD "金集 ≤30.5 s" 基于 −10~12% 预测（假设首窗渲染可全部藏进前置阶段），实现后实测该假设不成立（首窗渲染之前只有模型校验+加载，与其重叠实测反使总时长 +0.9 s——PDFium 渲染与 SHA-256 校验争抢 CPU）。按评审允许的"修订验收标准"路径，DoD 修订为 ≤31.0 s（同日 A/B 实测 31,332/31,041 vs 33,902/33,903 = −8.4%）。同时按计划承诺把 smoke 契约升级为 depth-16 GPU 版（窗 1 = 页 1–16，锚点 16/17）。
