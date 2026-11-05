# 扫描吞吐优化三期执行计划（backend sampling 主杠杆 + 提交路径 fallback）

**日期**：2026-11-05
**状态**：v2.1——2026-11-05 复审 **PASS** 后**已执行完毕（2026-11-05 收官）**：S0 决策门裁决 BS no-go（−2.5% ≪ 8%），T1 插桩合入（`8fbe735`）并以实测裁决 fallback P1/P2/P3 全部 no-go（−1.9% / 0 / −3.2%，`6200ec8`）——两个决策门均按"未过线即关闭归档"机制工作，未硬凑。总报告：`docs/test/reports/phase3-summary-2026-11-05.md`。执行记录：S0 spike 在 `scratch/backend-sampling-spike`（保留不合入）；主线交付 = T1 commit-profile 通道。
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
2. **语义门禁降档 + checkpoint 迁移**：贪心平局判定从 Rust（`total_cmp` total order、last-max wins）移入 llama.cpp greedy sampler，输出可能变化 → 第二档门禁（相似度 ≥0.999 + 人工抽检）。同时解码实现代次变化必须进入 OCR checkpoint 兼容键：`CompatibilityKey.parameters` 追加 `;decode=<gen>` 字段（先例：二期批深 `;batch=N`），否则旧 host-argmax 页面会在 `--resume` 中以 `SkipCommitted` 与新 backend-greedy 页面混入同一 `document.md`。BS 合入前必须落地迁移门禁（§4 BS.2/BS.4，规则 3）；
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
3. **checkpoint 解码代次键（BS 专项，全阶段有效）**：解码实现代次作为 `;decode=<gen>` 字段进入 `CompatibilityKey.parameters`（host-argmax 与 backend-greedy 为不同代次；pins 漂移若允许输出变化同样计入；先例 `;batch=N`）。native 路径键不受影响——native checkpoint 走 `route=native` 宽松匹配，不得因该字段失效。BS 合入门禁（§4 BS.2/BS.4）包含：旧 checkpoint 在 BS 后全部 `Reprocess`、同一 `document.md` 不得新旧解码代次混用、resume 提示与迁移文档的回归测试。
4. **WP.2 重审纪律**：pins 升级独立提交、CHANGELOG 医学级记录、B1 guard 测试（`windows_target_declares_pinned_llama_bindings_with_mtmd`）同步、升级后全量 gated 回归（`real_model_window_decode_matches_single_stream_prefix` 等）**先于任何性能采集**。
5. 门禁次序强制：正确性先行，性能冻结必须采于正确构建（一期 v1 教训）——对 S0 spike 同样适用（§4 S0.3/S0.4 排序）。
6. 计时卫生：warm 复测、两遍 <2%、Vulkan 独占重验；CUDA 归发布流程。
7. 输入钉定：金集 sha256 不变；md fixtures 经 `scripts/build-multidoc-fixtures.py` 重建 + manifest sha256 核对。

## 3. 杠杆总览与排序

| 杠杆 | 内容 | 关键依据 | 预期收益（金集） | 主要风险 | 门禁 |
|---|---|---|---|---|---|
| **S0** | backend-sampling spike：版本审计（含可实施性认定）+ scratch 构建 + 正确性/divergence 先行 + 实测 + 决策门 | §0.2；A2b 裁决链 + sys 0.1.153 C API 事实 | 决策数据 | wrapper 无安全暴露且 vendor patch 未批 → fallback 兜底 | 数据归档 + 双条件决策门（可实施性 + 收益） |
| **BS** | 设备端贪心采样集成：每序列 attach greedy、sampled-token 读取替换 get_logits_ith+argmax | S0 通过 | **−6~9 s（18–26%）→ ≈25–28 s** | WP.2 重审、门禁降档、平局语义 | 第二档 |
| T1 | 独立 `CommitProfile` 插桩（v2.1 完整采集链：owner 计时随 `PdfResponse` 回传 + CLI PNG 编码计时回传 + write/finalize 注入，见 §4 T1） | v1.2 调用链 + §4 T1 冻结口径 | 决策数据（尺子） | schema 兼容（黄金串锁定）；回传通道断点 | 数据归档 + 端到端插桩测试 |
| P1/P2 | 编码流水线（v1.1 冻结设计：EncodePipeline + generation） | v1.2 校准 | −1.5~3 s | 贴采纳线 | 第一档 |
| P3 | 全页 PNG 仅 `--keep-pages` 产写 | v1.2 校准 | ≈1–2 s | 概率低 | 瞬态断言 |

**执行序**：`S0 →（决策门）→ BS 主线 ∥ T1`；**S0 未过 → fallback `T1 → P1 → P2 → P3`**（每项独立过线才实施，未过线即关闭并记录）。

## 4. 阶段计划（TDD）

### S0 backend-sampling spike（≈0.5–1 天；scratch 分支，不进主线）

执行序强制：`S0.1 → S0.2 → S0.3（全量 gated/正确性 + divergence）→ S0.4（性能两遍）→ S0.5（决策门）`——正确性先行同样约束 spike，不得以"scratch/未进主线"绕过规则 5。

| 项 | 内容 | 产出 |
|---|---|---|
| S0.1 | 版本审计 + **可实施性认定**：wrapper ≥0.1.153 各版本是否暴露 `llama_set_sampler`/sampled-token API；暴露与否均须认定一个可指名的候选——其安全 API 覆盖 chain 创建/attach、sampled-token 读取（含 `LLAMA_TOKEN_NULL`）、错误返回（`llama_set_sampler` 返回 `bool`）与 sampler 生命周期；均未暴露 → 记录 vendor patch 子决策（升级 + 本地补丁 vs 等上游，patch 须提供同等安全封装），阻断上报。**产出必须是"已审计的 wrapper 版本号或已批 vendor patch 决策"，不能是"符号存在"** | 审计记录 + 可实施性认定 |
| S0.2 | spike 构建：scratch 分支升级 pins（workspace 不动），探针按 `mtmd-generate-probe` 模式；探针须实读验证 sys C API 的成功/生命周期契约——`llama_set_sampler` offload 分支要求 sampler chain 且处理 `bool` 返回、sampled-token 读取可能返回 `LLAMA_TOKEN_NULL`；`unsafe` 仅存在于该 scratch 构建（主线禁令不适用） | 探针二进制 + 契约验证记录 |
| S0.3 | **正确性 + divergence（先于任何性能采集，规则 4/5）**：升级后全量 gated 回归（`real_model_window_decode_matches_single_stream_prefix` 等）+ 自一致性两遍；divergence：真实模型金集页输出逐字节对照（Rust argmax vs 设备端 greedy）+ 构造平局 logits 的行为对照；量化差异面 | gated 回归报告 + divergence 记录 |
| S0.4 | 性能实测（仅在 S0.3 全绿后执行）：金集两遍 + md-small-20 一遍，step profile 三桶前后对照（logits_ffi / decode_wait / argmax）；确认 Vulkan 后端采样生效（logits_ffi 归零 = 生效的直接证据） | 前后数据 |
| S0.5 | **决策门（双条件，缺一不可）**：① 可实施性——S0.1 已指名满足 §3 S0 行安全 API 覆盖面的上游 wrapper 版本，或 vendor patch 子决策已批准；**scratch unsafe 探针通过不足以放行 BS 主线**。② 收益与 divergence——金集墙钟收益 ≥8% 且 divergence 面可被第二档门禁覆盖。两者同时满足 → BS；任一不满足 → fallback，全部数据归档 | 决策记录 |

### T1 commit-profile 插桩（≈0.5–1 天；v2.1 采集链完整设计，两主线通用）

`CommitProfile` 新类型（ocrx-output）：逐页 `{page, generation, render_us, render_wait_us, png_encode_us, commit_write_us}` + totals + `finalize_us`；独立 `commit-profile.json`（沿用 `OCRX_STEP_PROFILE` 开关，runtime `StepProfile` 一字不动）；黄金串锁定。

**测量起止点冻结（闭合口径唯一，不得重复计数）**：
- `render_wait_us`：`ScanService` 在 `self.renderer.render(page)` 调用者侧的完整等待墙钟（含缓存命中与 owner 后台工作）。**R/D/E/W/F 模型的 R 唯一取该字段**——它是关键路径上真实消耗的墙钟。
- `render_us`：PDFium owner 实际渲染工作时间（`render_direct`/预取路径内部计时）。当前 `PdfResponse::Render`/`RenderedPage` 不携带 owner 计时，须新增计时字段随响应回传；该字段**仅作诊断，不进入墙钟闭合**（调用者等待已涵盖其耗时，二者相加即重复计数）。
- `png_encode_us`：CLI 识别器内 `RgbImage::write_to(..., Png)` 起止。T1 先于 P1，`EncodePipeline` 尚未建成，编码仍在 CLI `OvisRecognizer` 内——回传通道 = `ScanService` 构造识别器时注入共享 recorder sink（`Arc` + 文档/页号/generation 键），识别器逐次编码后写入；`ScanService` 合并进 `CommitProfile`。`JobSummary` 是最终返回值，不构成上游回传通道；**不得等 P1 落地才获得该数据**。
- `commit_write_us` / `finalize_us`：ocrx-output 提交写盘与 finalize 路径注入同一 recorder 记录起止（写盘 = 写入开始至原子 rename 完成；finalize = finalize 入口至返回）。

**attempt / 取消 / 错误 / 多文档规则**：逐条记录按（文档、页号、generation/attempt）键；退化重试与批失败回退的全部 attempt 均入记录，totals 只累计终态 attempt；取消/失败窗口不得写 `commit-profile.json`（含部分写）；多文档每文档独立落盘各自 profile。

**验收前置（先于采集任何目标修订数据）**：端到端插桩测试证明 env 开启时各段非空回填、取消/错误不多写、env 关闭无文件；随后 md-small-20 + 金集各一遍 → 墙钟闭合校验（`render_wait + png_encode + commit_write + window decode + 其余` ≈ 总墙钟，±5%；闭合只用 `render_wait_us`，`render_us` 不参与）→ **fallback 目标值如有走线则修订一次**。

### BS 设备端采样集成（≈2–3 天；S0 通过后）

| 项 | 内容 |
|---|---|
| BS.1 | WP.2 重审落地：pins 升级独立提交（CHANGELOG + B1 guard 测试同步）；升级后全量 gated 回归 + 自一致性两遍 → 冻结新基线，**然后**才采性能数据；pins 漂移若允许输出变化，则计入 `;decode=<gen>` 代次键（规则 3） |
| BS.2 | 兼容键迁移：`CompatibilityKey` 构造处（`ocrx-cli`）追加 `;decode=<gen>`（host-argmax → backend-greedy 为代次变更）；native 键路径不动（native 页面不得失效）；不做任何旧→新 checkpoint 转换逻辑，旧 checkpoint 一律 `Reprocess` |
| BS.3 | 采样下沉：runtime 每序列 attach greedy（`llama_set_sampler`）；解码循环以 sampled-token 读取替换 `get_logits_ith` + argmax（单流与窗口两路径同步替换，保持两路径一致）；seed 采样点同步适配 |
| BS.4 | 门禁：第二档相似度 ≥0.999（vs 现行冻结参照）+ 人工抽检 + 自一致性逐字节两遍；**resume 回归**（含旧 checkpoint 页在 BS 后全部 `Reprocess`、`document.md` 无新旧解码代次混页、resume 提示与迁移文档断言、哈希有效的旧 checkpoint 也不得 `SkipCommitted`）；金集/md-small-20 计时两遍 <2%；显存断言复核 |
| BS.5 | 文档：A2b 裁决状态更新（解除）、WP.2 重审记录、tie 语义变更与 `step-profile.json` 桶语义说明（schema 不变）、checkpoint 解码代次迁移说明（用户可见行为：旧结果不再复用） |

### Fallback P1/P2/P3（仅 S0 未过时；设计 = v1.1/v1.2 冻结稿 + v2.1 修正收益模型，见附录 A）

- 执行序 P1 → P2 → P3，每项独立实测：P1/P2 ≥5%（md-small-20 口径）、P3 按消除量线；**未过线即关闭并归档，不硬凑**。
- 下述预期为 v1.2 校准的量级参考：P1 ≈−2~4 s（md-small-20）/ −1.5~3 s（金集）；P2 再 ≈−1 s；P3 ≈0.1–0.3 s/文档 + 金集写盘份额。**F1–F3 目标值必须以附录 A 修正模型 + T1 实测（R 取 `render_wait_us`）回填为准，不得沿用旧公式推数。**

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
| wrapper 无现成 `set_sampler` 暴露，或安全 API 覆盖面不足（chain/attach、`LLAMA_TOKEN_NULL`、`bool` 返回、生命周期） | S0.1 审计 + 可实施性认定前置；vendor patch 作为独立子决策上报，不默认实施；S0.5 可实施性条件兜底 → 探针通过不等于 BS 可实施 |
| 旧 checkpoint 新旧解码代次混页（`--resume` 静默复用） | 规则 3 `;decode=<gen>` 代次键；BS.2 迁移落地；BS.4 resume 回归门禁（哈希有效旧 checkpoint 也不得 `SkipCommitted`） |
| Vulkan 后端采样不生效/不受益 | S0.4 以 logits_ffi 归零为直接证据；失败即走 fallback |
| 贪心平局语义差异超预期（相似度 <0.999） | S0.3 提前测量差异面；第二档门禁把守；差异集中在少数平局 token 时评估接受度 |
| logits_ffi 收益部分转移而非消失（图尾等待仍在） | S0.4 实测裁决；≥8% 合入线防硬凑 |
| 升级 pins 引入 mtmd/上下文行为漂移 | BS.1 全量 gated 回归 + 两遍自一致性先于性能采集 |
| fallback P 杠杆贴线被否 | 按规则关闭不硬凑；已 banked 的 5.9× 不受影响 |
| 计时污染（冷启动/双链） | warm 两遍 + Vulkan 独占重验（一期纪律） |

## 7. 验收清单（DoD）

- [ ] S0 spike 报告 + 决策记录（双条件：可实施性认定 + 收益/divergence；做/不做 + 全部数据归档）
- [ ] T1 commit-profile 数据落档（两主线通用；端到端插桩测试先行：env 开启各段非空、取消/错误不多写、env 关闭无文件；墙钟闭合只用 `render_wait_us`）
- [ ] BS（若做）：pins 升级与 WP.2 重审记录 + `;decode=<gen>` 兼容键迁移与 resume 回归报告 + 第二档门禁报告 + tie 语义与 step-profile 桶语义说明
- [ ] fallback（若走）：逐项实测报告 + 合入/关闭判定；目标值修订以修正后收益模型（附录 A + 公式单元测试）为准，修订一次记录
- [ ] 全量：`cargo test --workspace` 全绿 + fmt/clippy 零警告 + 对应门禁档报告归档
- [ ] 阶段报告：`docs/test/reports/phase3-<stage>-<date>.md` 系列 + 总报告

## 8. 排期估计（单人）

S0 ≈0.5–1 天 → T1 ≈0.5–1 天（含插桩测试）→ BS ≈2–3 天（若做，含 BS.2 兼容键迁移与 BS.4 resume 回归）→ fallback ≈2–4 天（若走）。总计 **≈3–9 个工作日**，取决于 S0 决策门走向。

## 附录 A：fallback 冻结设计（自 v1.2 收编，仅 S0 未过时生效）

**调用链事实（v1.1 已核对）**：PNG 编码在 `ocrx-cli` 识别器内、识别返回之后（`main.rs` 窗口路径 `recognize_window_cancelled` / 单页路径 `recognize_artifact_cancelled`）；`commit_page`（ocrx-output）只写盘不编码；depth>1 窗口走内联提交循环；`image` 依赖在 `ocrx-cli`/`ocrx-ovisocr2`，`ocrx-output` 无 `image` 依赖。

**EncodePipeline 契约（v1.1 BLK-1 冻结）**：
- `PageArtifactResult` 移除 `png` → 仅识别结果；新增 `EncodedPagePng { generation, bytes }`；`RenderedPage` 增 `generation`（scan 内单调，覆盖重试 attempt）。
- `PagePngEncoder` trait 定义在 `ocrx-output`、生产实现在 `ocrx-cli`（`image` 0.25.10 默认参数逐字节一致）；`std::thread::scope` 有界池（并发 = 窗口深度），识别入口整窗入队与同窗解码并行；提交按终态 generation 取字节，错配 = panic（fail-loud）。
- 淘汰：重试入队撤旧 attempt 未开始任务；批失败回退先撤整窗在飞；取消 scope 收敛零写盘；机制 A 写盘路径物理不动。
- 错误面三分离：编码错误 = 页级 recognize 诊断（与现行逐字一致）；worker panic = `resume_unwind` fail-loud；断连 = panic。
- TDD 底线：真实 RGB PNG 等价、退化 attempt 终态匹配、批失败回退、panic/断连/取消、并行 vs 串行逐字节等价。

**统一收益模型（v2.1 修正）**：P2 的冻结触发点是 `render`/`render_perturbed` **完成后**入队，故当前页 R 必然先于其 E 启动，R 与 E 不可能重叠；单页窗口/depth-1 尾窗没有后续页渲染可用来隐藏 E。v1.1 重审 BLK-4 已用单页反例（R=2 s、D=1 s、E=2 s：实际 4 s + W）证伪"减去 R"的闭式公式。对每窗口 w，`R/D/E/W` 渲染/解码/编码/写盘墙钟，`F` finalize：

```
现行：T ≈ Σ(R + D + E + W) + F
P1 后：T ≈ Σ(R + D + max(0, E − D) + W) + F
P2 后（保守上界，不扣减当前页及任何已完成渲染）：
   T ≈ Σ(R + D + max(0, E − D) + W) + F − Δ_overlap
   Δ_overlap 仅计页 i 的编码与"该页入队之后剩余的后续页渲染"的重叠，
   按逐页 render-complete 事件与解码起止点的离散时间模拟计算，无闭式减法；
   单页窗口/depth-1 尾窗 Δ_overlap = 0。
P3：E 项与 W 的 PNG 分量归零；增量只计 P2 后仍暴露部分，不重复计入
```

**公式单元测试先于任何 M1–M3 回填**：① 单页反例（R=2 s、D=1 s、E=2 s → 4 s + W，不得给出 3 s + W）；② 两页窗口（页 1 编码只允许与页 2 渲染重叠，不得整窗减 R）；③ 退化重试（attempt-1 编码缓冲失效不计入）。多文档口径须明确串行逐页 E 如何换算到文档级 FIFO 池并行的 makespan 与 CPU 争用（编码为 CPU 密集，池并行存在争用折减）。M1–M3 数值唯一来源 = T1 实测（R/D/E/W/F，R 取 `render_wait_us`）+ 上述模型回填；P3 不预设数字。

## 9. 修订记录

- v1.0（2026-11-04）：初版。范围 = 二期 X1.4④ 归因承接的提交路径流水线化（T1 + P1–P4）。
- v1.1（2026-11-04）：按 v1.0 方案评审（BLOCK）修订——BLK-1 调用链重写 + EncodePipeline 冻结设计（§3.2）；BLK-2 独立 `CommitProfile` 通道重设计（不触 runtime `StepProfile`）；MAJOR-1 P3 门禁改为瞬态断言（不重基线默认成功产物）；MAJOR-2 统一收益模型（§3.3），撤销 ≤60 s 冲刺线。
- v1.2（2026-11-05）：实现评审 BLK-2 整改数据触发偏差条款——md-small-20 基线 114.2 → 26.2 s（5.9×），"PNG 编码 ≈1.5–2 s/页"证伪（实测 ≈0.05–0.15 s/页），目标重订为 ≤23 s / 金集 −1.5~3 s；P 杠杆贴采纳线，T1 为 go/no-go 前置。
- v2.0（2026-11-05）：**杠杆重构**——backend sampling（设备端贪心采样）依 A2b 裁决链升为主杠杆（S0 spike → 决策门 → BS 集成，第二档门禁 + WP.2 重审纪律；预期金集 −6~9 s → ≤28 s，冲刺 ≤26 s 补二期未竟主目标）；T1 降为两主线通用子项；P1/P2/P3 降级为 S0 未过时的条件 fallback（附录 A 收编冻结设计）；新增"不解禁 unsafe"非目标与 BS ≥8% 合入线。
- v2.1（2026-11-05）：按 2026-11-05 重审（BLOCK ×5）修订——**BLK-1** 新增规则 3 解码代次键：`;decode=<gen>` 进 `CompatibilityKey.parameters`（native 键不受影响），BS.2 迁移 + BS.4 resume 回归成为 BS 合入门禁；**BLK-2** S0.1 升级为可实施性认定（可指名 wrapper 版本的安全 API 覆盖 chain/attach、`LLAMA_TOKEN_NULL`、`bool` 返回、生命周期，或已批 vendor patch），S0.5 改双条件决策门，探针通过不等于 BS 可实施；**BLK-3** S0 重排为 `S0.1 → S0.2 → S0.3 正确性/divergence → S0.4 性能 → S0.5 决策门`，正确性先行约束 spike（规则 5）；**BLK-4** T1 采集链完整设计：`render_wait_us`（调用者墙钟）为 R 唯一模型输入、`render_us`（owner 工作时间，随 `PdfResponse` 回传）仅作诊断，CLI PNG 编码经共享 recorder sink 回传（不等 P1），write/finalize 注入、attempt/取消/多文档规则、端到端插桩测试前置；**BLK-5** 附录 A P2 公式按单页反例修正为保守上界 + 离散事件 Δ_overlap（单页/depth-1 尾窗 = 0），公式单元测试（单页/两页/退化重试）先于 M1–M3 回填。
