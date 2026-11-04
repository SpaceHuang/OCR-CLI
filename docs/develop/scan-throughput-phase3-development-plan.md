# 扫描吞吐优化三期执行计划（backend sampling 主杠杆 + 提交路径 fallback）

**日期**：2026-11-05
**状态**：v2.0 草案——待评审，未排期，评审通过前不进入实现（沿一期/二期硬性评审门禁纪律）。
**输入数据**：
- 二期总报告与实现评审响应：`docs/test/reports/phase2-summary-2026-11-04.md`、`docs/review/scan-throughput-phase2-implementation-review-response.md`
- X1 报告两轮实测（md-small-20 26.2 s / 5.9×）：`docs/develop/cross-doc-orchestration-2026-11-04.md` §2④
- T0 拆分数据：`docs/test/reports/phase2-t0-2026-11-03-data/`（depth-8 step profile）
- v1.0 方案评审（BLOCK，v1.1 修订依据）：`docs/review/scan-throughput-phase3-development-plan.md`（2026-11-04）
- 绑定事实（二期 A2b 裁决链）：`llama-cpp-sys-2` 0.1.153 的 C API 已有 `llama_set_sampler` / `llama_get_sampled_token_ith` / `llama_get_sampled_logits_ith`（`llama-context.cpp` 实读核对）；wrapper `llama-cpp-2` 0.1.152 **未暴露**上述接口（源码 grep 核对）
- 诊断通道：`OCRX_STEP_PROFILE`（二期 T0.1/A1）+ v1.1 设计的独立 `CommitProfile`（fallback 用）

## 0. 现状与问题定位

### 0.1 二期整改后的成本账本（金集 34.02 s，depth 8，实测）

| 成本项 | 量级 | 状态 |
|---|---:|---|
| decode_wait（GPU 图执行 + 整词表 DtoH 尾部） | 11.5 s | 隔离 bench 地板 ≈8.4 s（B=8，1527 tok/s），**≈3 s 为 DtoH/同步开销** |
| **logits_ffi（逐槽 `get_logits_ith` 同步）** | **7.9 s** | 钉定绑定固有成本——二期 A2b 裁决的遗留项，解除路径已明确（见 §0.2） |
| encode（mmproj 图编码） | 1.8 s | GPU 工作量，不可消除（A2d 裁决延续） |
| argmax（A2b 向量化后） | 1.8 s | 已优化到位 |
| slot_setup + residual | 1.3 s | 小 |
| 校验+加载固定税 | ≈5.3 s | BLK-1 完整性契约不动；多文档已摊薄（X1） |
| 提交段（渲染 + PNG 编码 + 写盘） | ≈4.4 s（≈0.15 s/页） | fallback P 杠杆对象（v1.2 校准后量级已封顶） |

多文档口径：md-small-20 编排 **26.2 s（5.9×）**，每文档 floor ≈1.3 s（窗口 ≈1.0 s + 提交段 ≈0.3 s）——已接近"文档级 FIFO + 现行窗口协议"的地板。

### 0.2 主杠杆依据：backend sampling（设备端贪心采样）

**机制**：`llama_set_sampler` 为每序列挂接采样器后，llama.cpp 在图执行尾声设备端完成采样（`needs_raw_logits` 变 false），**整词表 logits 的设备→主机搬运直接消失**（金集口径 ≈152 k 词 × 8 输出 × 4 B × 3543 步 ≈ 17 GB 搬运），每步 8 次 `get_logits_ith` 同步收敛为 1 次采样 token 读取。

**量化预期**：潜在 **−6~9 s（金集 −18~26% → ≈25–28 s）**。置信分层：
- 高置信 ≈3–5 s：logits_ffi 的图尾部溢出等待份额（compact 对照数据——同词表同步步数下 logits_ffi 5.6 ↔ 10.6 s 随图速变化，证明该等待占大头）；
- 待 spike 实测：逐调用开销合并 + DtoH 消失的净收益、图尾等待是否转移而非消失。

**代价与门槛**（本方案显式承接，不回避）：
1. **WP.2 绑定冻结重审**：一期决策"不升级 llama-cpp-2/llama-cpp-sys-2"——升级 pins 是独立提交 + 全量 gated 回归前置（§4 BS.1）；
2. **语义门禁降档**：贪心平局判定从 Rust（`total_cmp` total order、last-max wins）移入 llama.cpp greedy sampler，输出可能变化 → 第二档门禁（相似度 ≥0.999 + 人工抽检）；
3. **Vulkan 后端采样支持**需 spike 确认（设备端采样 epilogue 是否在该后端生效/受益）。

### 0.3 fallback 依据：提交段（v1.2 校准）

实现评审 BLK-2 整改证伪了 v1.0 的"渲染+PNG ≈4 s/文档"归因（主体是逐文档校验）后，提交段量级封顶：PNG 编码 ≈0.05–0.15 s/页、渲染 ≈0.1–0.2 s/页、写盘 ≈0.05–0.1 s/文档。fallback 杠杆（P1/P2/P3）的收益上界：md-small-20 ≈−2~4 s（8–15%，贴 ≥5% 采纳线）、金集 ≈−1.5~3 s（4–9%）。**降级为条件杠杆**：仅 S0 决策门未通过时逐项评估。

## 1. 目标与非目标

### 1.1 目标

| 口径 | 现状（v1.2 基线） | 主目标 | 冲刺 |
|---|---:|---:|---:|
| 金集单文档墙钟 | 34.02 s | **≤28 s（BS 路径；spike 后按实修订一次）** | ≤26 s（二期原主目标线） |
| md-small-20 编排 | 26.2 s | 随 BS 小幅受益（fallback 路径 ≤23 s） | — |
| 产物契约 | BS：第二档（输出可能变化）；fallback P1/P2：第一档逐字节；P3：瞬态断言 | 见 §2 | — |

### 1.2 非目标

- **不解禁 unsafe**：绑定能力获取只走 wrapper 升级一条路（workspace `unsafe_code = "forbid"` 不动）。
- 不改机制 A 写盘原子性、X1 文档级 FIFO 边界、解码 GPU 地板、mmproj 编码份额（A2d 延续）。
- **不改动 runtime `StepProfile` 的字段与 schema**（v1.1 BLK-2 延续；BS 只更新个别桶的语义说明）。
- KV 量化、speculative decoding、校验缓存（BLK-1 契约）延续为非目标。
- 不做跨窗口/跨文档提交流水线；不动 figure crop 产物与 native 文本路径。

## 2. 规则（沿用二期，全阶段有效）

1. **收益判定**：<2% 回滚；**BS 合入线 = spike 实测金集墙钟收益 ≥8%**（对齐其在账本中的份额，防止"转移而非消失"被硬凑）；fallback P1/P2 各自 ≥5%（md-small-20 口径）才合入；P3 按"消除量 ≥30% 每文档成本"。
2. **门禁分档**：BS = 第二档（结构化相似度 ≥0.999 vs 现行冻结参照 + 人工抽检 + 自一致性逐字节两遍）；T1/P1/P2 = 第一档（`document.md` + 非计时 `page-*` 逐字节）；P3 = 瞬态断言更新（v1.1 MAJOR-1 口径，不重基线默认成功产物）。P 杠杆与 BS 门禁互不混写。
3. **WP.2 重审纪律**：pins 升级独立提交、CHANGELOG 医学级记录、B1 guard 测试（`windows_target_declares_pinned_llama_bindings_with_mtmd`）同步、升级后全量 gated 回归（`real_model_window_decode_matches_single_stream_prefix` 等）**先于任何性能采集**。
4. 门禁次序强制：正确性先行，性能冻结必须采于正确构建（一期 v1 教训）。
5. 计时卫生：warm 复测、两遍 <2%、Vulkan 独占重验；CUDA 归发布流程。
6. 输入钉定：金集 sha256 不变；md fixtures 经 `scripts/build-multidoc-fixtures.py` 重建 + manifest sha256 核对。

## 3. 杠杆总览与排序

| 杠杆 | 内容 | 关键依据 | 预期收益（金集） | 主要风险 | 门禁 |
|---|---|---|---|---|---|
| **S0** | backend-sampling spike：版本审计 + scratch 构建 + 实测 + divergence | §0.2；A2b 裁决链 + sys 0.1.153 C API 事实 | 决策数据 | wrapper 无现成暴露 → vendor patch 决策点 | 数据归档 + 决策门 |
| **BS** | 设备端贪心采样集成：每序列 attach greedy、sampled-token 读取替换 get_logits_ith+argmax | S0 通过 | **−6~9 s（18–26%）→ ≈25–28 s** | WP.2 重审、门禁降档、平局语义 | 第二档 |
| T1 | 独立 `CommitProfile` 插桩（v1.1 BLK-2 设计原样） | v1.2 调用链 | 决策数据（尺子） | schema 兼容（黄金串锁定） | 数据归档 |
| P1/P2 | 编码流水线（v1.1 冻结设计：EncodePipeline + generation） | v1.2 校准 | −1.5~3 s | 贴采纳线 | 第一档 |
| P3 | 全页 PNG 仅 `--keep-pages` 产写 | v1.2 校准 | ≈1–2 s | 概率低 | 瞬态断言 |

**执行序**：`S0 →（决策门）→ BS 主线 ∥ T1`；**S0 未过 → fallback `T1 → P1 → P2 → P3`**（每项独立过线才实施，未过线即关闭并记录）。

## 4. 阶段计划（TDD）

### S0 backend-sampling spike（≈0.5–1 天；scratch 分支，不进主线）

| 项 | 内容 | 产出 |
|---|---|---|
| S0.1 | 版本审计：wrapper ≥0.1.153 各版本是否暴露 `llama_set_sampler`/sampled-token API；均未暴露 → 记录 vendor patch 子决策（升级 + 本地补丁 vs 等上游），阻断上报 | 审计记录 |
| S0.2 | spike 构建：scratch 分支升级 pins（workspace 不动），探针按 `mtmd-generate-probe` 模式；`unsafe` 仅存在于该 scratch 构建（主线禁令不适用） | 探针二进制 |
| S0.3 | 实测：金集两遍 + md-small-20 一遍，step profile 三桶前后对照（logits_ffi / decode_wait / argmax）；确认 Vulkan 后端采样生效（logits_ffi 归零 = 生效的直接证据） | 前后数据 |
| S0.4 | divergence：真实模型金集页输出逐字节对照（Rust argmax vs 设备端 greedy）+ 构造平局 logits 的行为对照；量化差异面 | divergence 记录 |
| S0.5 | **决策门**：金集墙钟收益 ≥8% 且 divergence 面可被第二档门禁覆盖 → BS；否则 → fallback，全部数据归档 | 决策记录 |

### T1 commit-profile 插桩（≈0.5 天；v1.1 BLK-2 设计原样执行，两主线通用）

`CommitProfile` 新类型（ocrx-output）：逐页 `{page, render_us, render_wait_us, png_encode_us, commit_write_us}` + totals + `finalize_us`；独立 `commit-profile.json`（沿用 `OCRX_STEP_PROFILE` 开关，runtime `StepProfile` 一字不动）；黄金串锁定；md-small-20 + 金集各一遍 → 墙钟闭合校验（±5%）→ **fallback 目标值如有走线则修订一次**。

### BS 设备端采样集成（≈2–3 天；S0 通过后）

| 项 | 内容 |
|---|---|
| BS.1 | WP.2 重审落地：pins 升级独立提交（CHANGELOG + B1 guard 测试同步）；升级后全量 gated 回归 + 自一致性两遍 → 冻结新基线，**然后**才采性能数据 |
| BS.2 | 采样下沉：runtime 每序列 attach greedy（`llama_set_sampler`）；解码循环以 sampled-token 读取替换 `get_logits_ith` + argmax（单流与窗口两路径同步替换，保持两路径一致）；seed 采样点同步适配 |
| BS.3 | 门禁：第二档相似度 ≥0.999（vs 现行冻结参照）+ 人工抽检 + 自一致性逐字节两遍；金集/md-small-20 计时两遍 <2%；显存断言复核 |
| BS.4 | 文档：A2b 裁决状态更新（解除）、WP.2 重审记录、tie 语义变更与 `step-profile.json` 桶语义说明（schema 不变） |

### Fallback P1/P2/P3（仅 S0 未过时；设计 = v1.1/v1.2 冻结稿，见附录 A）

- 执行序 P1 → P2 → P3，每项独立实测：P1/P2 ≥5%（md-small-20 口径）、P3 按消除量线；**未过线即关闭并归档，不硬凑**。
- v1.2 校准预期：P1 ≈−2~4 s（md-small-20）/ −1.5~3 s（金集）；P2 再 ≈−1 s；P3 ≈0.1–0.3 s/文档 + 金集写盘份额。

## 5. 里程碑

| 里程碑 | 内容 | 金集 | md-small-20 |
|---|---|---:|---:|
| M0 现状（二期整改落点） | 编排 + depth 8 | 34.02 s | 26.2 s |
| M1 +BS（主目标） | 设备端采样（spike 实测回填） | **≤28 s** | 随 BS |
| M2 +BS（冲刺） | 同上 | **≤26 s**（二期原线） | — |
| F1–F3（fallback） | P1/P2/P3 逐项过线 | ≤32 s | ≤23 s |

## 6. 风险登记

| 风险 | 缓解 |
|---|---|
| wrapper 无现成 `set_sampler` 暴露 | S0.1 审计前置；vendor patch 作为独立子决策上报，不默认实施 |
| Vulkan 后端采样不生效/不受益 | S0.3 以 logits_ffi 归零为直接证据；失败即走 fallback |
| 贪心平局语义差异超预期（相似度 <0.999） | S0.4 提前测量差异面；第二档门禁把守；差异集中在少数平局 token 时评估接受度 |
| logits_ffi 收益部分转移而非消失（图尾等待仍在） | S0.3 实测裁决；≥8% 合入线防硬凑 |
| 升级 pins 引入 mtmd/上下文行为漂移 | BS.1 全量 gated 回归 + 两遍自一致性先于性能采集 |
| fallback P 杠杆贴线被否 | 按规则关闭不硬凑；已 banked 的 5.9× 不受影响 |
| 计时污染（冷启动/双链） | warm 两遍 + Vulkan 独占重验（一期纪律） |

## 7. 验收清单（DoD）

- [ ] S0 spike 报告 + 决策记录（做/不做 + 全部数据归档）
- [ ] T1 commit-profile 数据落档（两主线通用，含墙钟闭合校验）
- [ ] BS（若做）：pins 升级与 WP.2 重审记录 + 第二档门禁报告 + tie 语义与 step-profile 桶语义说明
- [ ] fallback（若走）：逐项实测报告 + 合入/关闭判定；目标值修订一次记录
- [ ] 全量：`cargo test --workspace` 全绿 + fmt/clippy 零警告 + 对应门禁档报告归档
- [ ] 阶段报告：`docs/test/reports/phase3-<stage>-<date>.md` 系列 + 总报告

## 8. 排期估计（单人）

S0 ≈0.5–1 天 → T1 ≈0.5 天 → BS ≈2–3 天（若做）→ fallback ≈2–4 天（若走）。总计 **≈3–8 个工作日**，取决于 S0 决策门走向。

## 附录 A：fallback 冻结设计（自 v1.2 收编，仅 S0 未过时生效）

**调用链事实（v1.1 已核对）**：PNG 编码在 `ocrx-cli` 识别器内、识别返回之后（`main.rs` 窗口路径 `recognize_window_cancelled` / 单页路径 `recognize_artifact_cancelled`）；`commit_page`（ocrx-output）只写盘不编码；depth>1 窗口走内联提交循环；`image` 依赖在 `ocrx-cli`/`ocrx-ovisocr2`，`ocrx-output` 无 `image` 依赖。

**EncodePipeline 契约（v1.1 BLK-1 冻结）**：
- `PageArtifactResult` 移除 `png` → 仅识别结果；新增 `EncodedPagePng { generation, bytes }`；`RenderedPage` 增 `generation`（scan 内单调，覆盖重试 attempt）。
- `PagePngEncoder` trait 定义在 `ocrx-output`、生产实现在 `ocrx-cli`（`image` 0.25.10 默认参数逐字节一致）；`std::thread::scope` 有界池（并发 = 窗口深度），识别入口整窗入队与同窗解码并行；提交按终态 generation 取字节，错配 = panic（fail-loud）。
- 淘汰：重试入队撤旧 attempt 未开始任务；批失败回退先撤整窗在飞；取消 scope 收敛零写盘；机制 A 写盘路径物理不动。
- 错误面三分离：编码错误 = 页级 recognize 诊断（与现行逐字一致）；worker panic = `resume_unwind` fail-loud；断连 = panic。
- TDD 底线：真实 RGB PNG 等价、退化 attempt 终态匹配、批失败回退、panic/断连/取消、并行 vs 串行逐字节等价。

**统一收益模型（v1.1 MAJOR-2）**：对每窗口 w，`R/D/E/W` 渲染/解码/编码/写盘墙钟，`F` finalize：
```
现行：T ≈ Σ(R + D + E + W) + F
P1 后：T ≈ Σ(R + D + max(0, E − D) + W) + F
P2 后：T ≈ Σ(R + D + max(0, E − D − R) + W) + F
P3：E 项与 W 的 PNG 分量归零；增量只计 P2 后仍暴露部分，不重复计入
```
M1–M3 数值唯一来源 = T1 实测（R/D/E/W/F）回填；P3 不预设数字。

## 9. 修订记录

- v1.0（2026-11-04）：初版。范围 = 二期 X1.4④ 归因承接的提交路径流水线化（T1 + P1–P4）。
- v1.1（2026-11-04）：按 v1.0 方案评审（BLOCK）修订——BLK-1 调用链重写 + EncodePipeline 冻结设计（§3.2）；BLK-2 独立 `CommitProfile` 通道重设计（不触 runtime `StepProfile`）；MAJOR-1 P3 门禁改为瞬态断言（不重基线默认成功产物）；MAJOR-2 统一收益模型（§3.3），撤销 ≤60 s 冲刺线。
- v1.2（2026-11-05）：实现评审 BLK-2 整改数据触发偏差条款——md-small-20 基线 114.2 → 26.2 s（5.9×），"PNG 编码 ≈1.5–2 s/页"证伪（实测 ≈0.05–0.15 s/页），目标重订为 ≤23 s / 金集 −1.5~3 s；P 杠杆贴采纳线，T1 为 go/no-go 前置。
- v2.0（2026-11-05）：**杠杆重构**——backend sampling（设备端贪心采样）依 A2b 裁决链升为主杠杆（S0 spike → 决策门 → BS 集成，第二档门禁 + WP.2 重审纪律；预期金集 −6~9 s → ≤28 s，冲刺 ≤26 s 补二期未竟主目标）；T1 降为两主线通用子项；P1/P2/P3 降级为 S0 未过时的条件 fallback（附录 A 收编冻结设计）；新增"不解禁 unsafe"非目标与 BS ≥8% 合入线。
