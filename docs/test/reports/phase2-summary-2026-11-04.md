# Phase-2 总报告：扫描吞吐优化二期（杠杆 A/C + 跨文档编排，2026-11-04）

**方案：** `docs/plan/scan-throughput-phase2-development-plan.md`（v1.2，评审 UNBLOCK/PASS）
**分支：** `feature/scan-throughput-phase2`（worktree `D:\Develop\OCR-CLI\.worktrees\scan-throughput-phase2`，自 main `620fb2a`）
**执行方式：** TDD（每项先立测试/门禁再实现），全部性能数字为同机（RTX 4090 D）Vulkan 实测，输入钉定不变（金集 sha256 `5d7adf7d…`）。

## 1. 落点总览

| 阶段 | 内容 | 结果 | 提交 |
|---|---|---|---|
| T0.1/A1 | `OCRX_STEP_PROFILE` 分段计时独立调试通道 + schema 兼容锁 | 合入；金集插桩构建输出与冻结参照逐字节一致 | `6b15b21` |
| T0.1 | 每步拆分 | 窗口 30.4 s = decode 43.9% + **逐槽 FFI 34.6%** + argmax 11.3% + encode 5.9%；batch/detok/ring <0.05%（no-win 判定依据） | 报告 |
| T0.2 | n_ctx 双点复采 | 35840 vs 73728 @B4：1023.4 vs 1019.7 tok/s（<0.4%）——排除 n_ctx 归因 | 数据 |
| T0.3 | 多文档 fixture + 基线 | md-small-20 逐个调用 154.3–155.8 s；脚本+manifest 钉定，PDF 不入库 | `28819db` |
| T0.4 | compact 档 | 24.4 s / 527 tok/s；相似度 **0.9942**（<0.999 红线） | 数据 |
| A2a/A2c | batch 复用 / 环检测 | **不实施**（T0.1 实测 <2%，实施前判定） | 报告 |
| A2b | 逐槽 FFI 合并 | FFI 数量合并**绑定受限不可实施**（wrapper 未暴露 backend-sampler/批量 logits 接口 + unsafe 禁用）；**argmax 向量化合入** | `7d06dd5` |
| A2d | 编码/解码重叠 | 评估结论不做（上界 2.5 s，机制改动超范围） | 报告 |
| **A 落点** | | 墙钟 40.18 → **38.24 s**（−4.9%），逐字节一致 | 报告 |
| **C** | 批深 4→8 解冻 | **冻结 N=8**：34.02 s（−11.0%）、527 tok/s、相似度 1.0000（逐字节）、KV 1656 MiB ≤ 断言、100 页回归 0 异常、Ctrl-C 冒烟过 | `cd6…`（window-depth-8 提交） |
| **X1** | 跨文档编排 | 合入；实现评审 4 项 BLOCK 整改（worker 子令牌取消、校验每调用一次、相对 -o、stdout 契约）后重采：**md-small-20 26.2 s = 5.9×、md-mixed 18.2 s = 3.6×**，安全验收四项全过、内容逐字节一致 | `56a084c` + 整改提交 |
| X2/B2/D | 决策点 | **全部关闭**：X2 不做（上界≈9% vs 语义重设计）、B2 不做（X1 摊薄后残余 0.7–2 s 且受 BLK-1 约束）、D 不推广（0.9942<0.999） | 决策文档 |

## 2. 里程碑对照（方案 §5）

| 里程碑 | 方案现实目标 | 实测 | 判定 |
|---|---:|---:|---|
| M0 现状 | 40.0 s | 40.18 s（插桩基线复测） | ✓ |
| M1 +A | ≤32 s | **38.24 s** | ❌（T0.1 修订：缺口主体为绑定固有同步 + GPU 地板，host 可兑现份额仅 argmax；方案预留的"目标值据 T0.1 修订"条款适用，v1.0 上界线撤回） |
| M2 +C = 单文档终线 | ≤27 s | **34.02 s** | ❌（同上 + B8 兑现率 40% 与 N=4 的 47% 同量级，双口径纪律符合预期） |
| vs P0（55.5 s） | ≈2.1× | **1.63×** | — |

单文档口径的差距构成（34.02 s 分解，depth-8 step profile）：decode_wait 11.5 s（GPU 地板+整块 DtoH）、logits_ffi 7.9 s（钉定绑定每步同步，解除需升级 wrapper 或解禁 unsafe——独立提案）、encode+setup 2.6 s、argmax 1.8 s、其余 1.4 s。

## 3. 验收清单核对（方案 §7 DoD）

- [x] T0 四份数据落档；A 目标值修订一次（偏差 >20% 条款触发：FFI 份额为缺口主体而非 batch 构建）
- [x] 每个合入项：`cargo test --workspace` 全绿（**381 passed / 0 failed**，基线 357 → +24 新测试）+ fmt/clippy 零警告 + 门禁档报告归档
- [x] 单文档金集两遍复测 <2%（每阶段均 0.06%–0.38%）；主目标 wall ≤26 s **未达**（34.02 s）——逐项判定：未兑现杠杆 = 逐槽 FFI 合并（钉定绑定限制，A2b 裁决）+ 编码重叠（A2d 裁决），均有数据与解除路径
- [x] `md-small-20` 基线 + X1 收益归档（整改后 **5.9×**，1.5× 门禁过；初版 1.35× 的归因错误由评审 BLK-2 证伪并修正）
- [x] 三个决策点各有 spike/测量记录与明确结论（X2 不做 / B2 不做 / D 不推广）
- [x] 阶段报告：`phase2-t0-2026-11-03.md`、`phase2-hostpath-2026-11-04.md`、`window-depth-8-2026-11-04.md`、`cross-doc-orchestration-2026-11-04.md`、决策点文档 + 本总报告
- [x] 首轮评审 BLK-1/BLK-2/MAJOR-1 整改闭环：B1 撤销未实施 ✓（BLK-1 裁决维持，scan 默认路径仍全量校验）；X1 切换协议经 X1.0 冻结文档先行 + 四项安全验收 ✓；门禁口径按 MAJOR-1 修正执行（逐字节集合 = document.md + 非计时 page-*；result.json/gpu-resource.json 结构化比较）✓
- [x] 一期遗留移交项复核：W4.2（登记基准机基线）仍归发布流程，本分支未动 ✓

## 4. 交付物索引

**代码**（8 个功能提交，全链 TDD）：
- `ocrx-runtime`：step_profile 调试通道、argmax_total_cmp（向量化 + 位级等价属性测试）
- `ocrx-core`：WINDOW_DEPTH 8 冻结历史
- `ocrx-output`：run_pages_with_model 共享 actor 入口
- `ocrx-cli`：多输入解析、SwitchDocument 协议、scan_document/run_scan_multi 编排器、hash8 输出布局、scan-summary.json、resume 提示语
- `scripts/`：build-multidoc-fixtures.py、measure-multidoc-baseline.py；windows-ctrl-c-smoke.ps1 depth-8 契约

**数据**：`docs/test/reports/phase2-t0-2026-11-03-data/`、`phase2-t0-2026-11-04-data/`（`-text` 归档）

**未尽事项与移交**：
1. **CUDA 后端复测**归发布流程（一期 §7.2 原则：收益构成不同须独立实测；本机 CUDA 工具链在位，配方沿 §5.5）。
2. **逐槽 FFI 合并解除路径**：升级 llama-cpp-2（违反 WP.2 冻结，需独立提案）或解禁 unsafe 直连 sys（独立提案）——预期收益上限 ≈7.9 s/金集扫描（depth-8 实测 logits_ffi 桶）。
3. **渲染/PNG 与解码重叠**：md-small-20 类场景的下一个 1.3–1.5× 来源（每文档 ≈4 s 固有成本），涉及机制 A 提交时序——独立提案。
4. ~~md-small-20 X1.4④ 门禁 1.35× < 1.5×~~ 已随实现评审 BLK-2 整改解决（5.9×）；整改记录见 `docs/review/scan-throughput-phase2-implementation-review.md` 与评审响应文档。
5. 评审前不合入 main（沿一期模式：分支 + 评审）。
