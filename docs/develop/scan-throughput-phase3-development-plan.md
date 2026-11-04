# 扫描吞吐优化三期执行计划（提交路径流水线化：渲染/PNG 编码与解码重叠）

**日期**：2026-11-04
**状态**：v1.1 评审修订版——BLK-1/BLK-2 修订完成，待复审；复审通过前不进入实现（沿一期/二期硬性评审门禁纪律）。
**输入数据**：
- 二期总报告：`docs/test/reports/phase2-summary-2026-11-04.md`
- X1 报告（md-small-20 未达标归因 §2④）：`docs/develop/cross-doc-orchestration-2026-11-04.md`
- T0 拆分数据：`docs/test/reports/phase2-t0-2026-11-03-data/`、`phase2-t0-2026-11-04-data/`
- v1.0 评审（BLOCK，本文修订依据）：`docs/review/scan-throughput-phase3-development-plan.md`（2026-11-04）
- 诊断通道：`OCRX_STEP_PROFILE`（二期 T0.1/A1 交付；本方案 T1 **不改动** runtime `StepProfile`，新增独立 commit-profile 通道）

## 0. 现状与问题定位

### 0.1 二期落点与残余瓶颈

| 场景 | 现状 | 窗口解码占每文档 | 非窗口占每文档 |
|---|---:|---:|---:|
| 金集单文档（30 页，depth 8） | 34.02 s | 24.3 s（71%） | ≈9.7 s（≈0.32 s/页） |
| md-small-20 编排（20×2 页图像文档） | 114.2 s（1.35× vs 逐个调用） | ≈1.0 s（18%） | **≈4.6 s（82%）** |

二期 X1.4④ 的结论：编排的固定税摊薄（2.1 s/文档）已完全兑现，剩余瓶颈是**每文档 ≈4 s 的"渲染 + 全页 PNG 编码 + 提交"串行段**——它既不在 GPU 上，也不在推理 actor 上，是纯 host 串行成本。

### 0.2 提交段的真实调用链（v1.1 按 BLK-1 重写，代码位置已逐一核对）

现行"编码 → 提交"分属两个 crate、四个环节，**PNG 编码不在提交函数里，而在 CLI 识别器内、发生在识别返回之后**：

1. **渲染**：`ocrx-output` `ScanService::run_pages_with_model` 按窗口渲染（batched 路径逐页 `render`，`lib.rs:868-880`；单页/重试路径 `serial_attempt_outcome`，`lib.rs:673-717`，退化重试以 47/50、53/50 扰动重渲染）。
2. **识别 + PNG 编码**（`ocrx-cli` `OvisRecognizer`，`main.rs:2032-2136`）：
   - 窗口路径 `recognize_window_cancelled`（`main.rs:2074-2136`）**等待整窗** `write_window_artifacts_with_status_cancelled` 返回后，才在 `main.rs:2113-2126` 逐页执行 `RgbImage::write_to(..., ImageFormat::Png)`；
   - 单页路径 `recognize_artifact_cancelled`（`main.rs:2032-2071`）同样在识别返回后于 `main.rs:2058-2064` 内联编码。
3. **数据交接**：`PageArtifactResult` 携带已编码字节（`ocrx-output/src/lib.rs:604-615` 的 `png: Vec<u8>` 字段）。
4. **提交（两条路径，均只写盘不编码）**：
   - depth-1 窗口 → `serial_attempt_outcome` 后走 `commit_terminal_pages`（调用点 `lib.rs:852`，函数体 `lib.rs:1585-1657`）；
   - depth>1 窗口（**md-small-20 的 2 页窗口与金集 depth-8 窗口都走这条**）→ 窗内联提交循环（`lib.rs:1009-1126`），批失败回退与退化重试也汇入此处；
   - 两者最终都调 `commit_page`（`lib.rs:2467-2504`）：仅 `atomic_write` PNG/MD/JSON + checkpoint。
5. **收尾**：全部成功后 `finalize_document` + `cleanup_full_page_images`（`lib.rs:1129-1137`、`lib.rs:1171-1178`）删除 `images/page-NNNN.png`（`--keep-pages` 除外）；figure crop（`images/page-NNNN-figure-MMM.png`）由 `ocrx-ovisocr2` 在识别调用内产出，不在此清理范围。
6. **依赖事实**：`image` crate 在 `ocrx-cli`/`ocrx-ovisocr2`/`ocrx-pdf`，**`ocrx-output` 无 `image` 依赖**；窗口深度 = `ocrx_runtime::WINDOW_DEPTH`（`lib.rs:791-792`）。

成本构成（量级仍为推断，**位置为事实**，实测由 T1 钉死）：

| 子项 | 估计量级 | 依据 |
|---|---:|---|
| **全页 PNG 编码**（`page-NNNN.png`） | ≈1.5–2 s/页（图像页最坏情形） | `image` 0.25.10 默认 deflate，~8MP 照片型 RGB；金集文本页 ≈0.2–0.3 s/页——**成本强内容依赖** |
| PDFium 渲染 | ≈0.2–0.5 s/页 | 已被 owner 线程预取与上一窗解码部分重叠（一期 P3） |
| 落盘/收尾 | <0.5 s/文档 | `commit_page` atomic write、checkpoint、`document.md` finalize |

**结构性发现（杠杆 P3 的依据）**：默认成功路径下 `commit_page` 写下的全页 PNG 在 merge 后被 `cleanup_full_page_images` 删除；`--resume` 只依赖 checkpoint + `page-*.md`。默认路径的最终产物集合**不含**全页 PNG——它是"编码 2 s、写完即弃"的过路文件。

### 0.3 关键观察（杠杆 P2 的依据，v1.1 修正表述）

全页 PNG 的像素输入只来自 `RenderedPage.rgb8`，**与识别输出无关**——编码原则上可与同窗 GPU 解码完全并行。现行实现把它串在识别返回之后（`main.rs:2113-2126`），且与识别结果共处 `PageArtifactResult` 一个结构体，这是纯调度与数据所有权问题。P2 的前提是**先把"识别结果"和"渲染页 PNG 编码结果"拆开**（§3.2），不能只在现有函数里挪几行。

## 1. 目标与非目标

### 1.1 目标

| 口径 | 现状 | 主目标 | 冲刺 |
|---|---:|---:|---:|
| md-small-20 编排墙钟（20×2 页图像文档） | 114.2 s | **≤85 s（≈1.9× vs 逐个调用基线 155 s；初估，T1 后按 §3.3 模型修订一次）** | 冲刺线不预设：≤60 s 属 M3 决策点输出，须由 §3.3 模型推导（MAJOR-2 修订，T1 前不作为可验收预期） |
| 金集单文档墙钟（解码主导，收益较小） | 34.02 s | ≤32 s（初估） | ≤31 s（初估） |
| 产物契约 | P1/P2：逐字节门禁集合不变；P3：默认成功与 `--keep-pages` 最终产物不变，仅中断/失败瞬态断言更新；P4：PNG 字节变更 → 涉 PNG 参照局部重基线 | 见 §2 | — |

### 1.2 非目标

- 不改机制 A 的写盘原子性：取消门之后的"零提交"契约、Ctrl-C 冒烟契约（`windows-ctrl-c-smoke.ps1`）保持逐字不变。
- 不做跨窗口/跨文档的提交流水线（X1.0 冻结的文档级 FIFO 边界不动）。
- 不动解码地板（GPU 工作量）与 mmproj 编码份额（二期 A2d 裁决延续）；**不改动 runtime `StepProfile` 的任何字段与语义**（BLK-2）。
- 不引入 rayon 等新依赖（编码池用 `std::thread::scope` 有界并发，窗口深度即上限）。
- 不做 PNG 编码器多线程化（单图 deflate 流串行；P4 的快速压缩设置是另一条路）。
- 不动 figure crop 产物（`page-NNNN-figure-MMM.png`，识别调用内产出）与 native PDF 文本路径（`commit_native_page` 无图像，不受影响）。

## 2. 规则（沿用二期，全阶段有效；规则 3 按 MAJOR-1 重写）

1. 收益判定：<2% 回滚；P1/P2 建议 ≥5%（md-small-20 口径）才合入；P3 属瞬态产物集变更，判据是"消除量 ≥30% 每文档成本 + 评审通过"。
2. 第一档（逐字节）适用 P1/P2：`document.md` + 非计时 `page-*` 与 depth-8 现行冻结参照逐字节一致——**编码并行化/提前化不改变任何字节**（同像素、同编码器 `image` 0.25.10 默认参数、逐页独立编码字节确定，仅时序变化）。
3. **P3（瞬态产物集变更）按路径拆分门禁**：默认成功产物与 `--keep-pages` 最终内容产物**逐字节不变，不重基线任何冻结参照**；真正变化的只有中断/失败目录中的瞬态全页 PNG（默认路径不再写入）。门禁 = 核对并更新 `windows-ctrl-c-smoke.ps1`/`real_scan_fault_injection.rs` 中涉及瞬态 PNG 的断言 + 新增"resume 不读全页 PNG"回归测试 + 行为评审 + 使用指引更新。figure crop 与精确全页文件名分开陈述，不在本项范围。**P4 改变 PNG 字节**：凡包含 `page-*.png` 的冻结参照/断言局部重基线（实施前核对参照清单），第二档相似度评审。P3/P4 与 P1/P2 的门禁互不混写。
4. 门禁次序强制（二期 §2 规则 2 延续）：正确性门禁先行，性能冻结必须采于正确构建。
5. 计时卫生：warm 复测、两遍 <2%、Vulkan 独占重验。
6. 输入钉定：金集 sha256 不变；md fixtures 经 `scripts/build-multidoc-fixtures.py` 重建 + manifest sha256 核对。

## 3. 杠杆总览与排序

### 3.1 杠杆表

| 杠杆 | 内容 | 关键依据 | 预期收益（md-small-20） | 主要风险 | 门禁 |
|---|---|---|---|---|---|
| T1 | **独立 commit-profile 通道**：逐页 `render_us`/`render_wait_us`/`png_encode_us`/`commit_write_us` + `finalize_us`，新文件 `commit-profile.json`；**runtime `StepProfile` 与其 `encode`（mmproj）语义一字不动** | §0.2 是位置事实 + 量级推断，先拆分后动手（二期教训：A2a 预判即 no-win） | 决策数据；**P2/P3 目标值据此修订一次，§3.3 模型回填** | schema 兼容（新类型黄金串锁定） | 数据归档 |
| **P1** | **编码流水线地基（BLK-1 修订）**：`PageArtifactResult` 拆出 `png`；编码移入 `EncodePipeline`（`std::thread::scope` 有界池，池深 = 窗口深度），触发点 = 识别入口整窗入队，与同窗解码并行；两条提交路径按 (page, generation) 取字节，`commit_page` 物理不动 | §0.2：编码在 `main.rs:2113-2126` 且识别与编码耦合在 `PageArtifactResult`；拆分后并行与重叠才有挂载点 | 初估 ≈−15 s（暴露项 max(0, E−D)，§3.3） | 数据拆分波及 fake 识别器测试设施；错误面/attempt 身份需按 §3.2 冻结 | 第一档 |
| **P2** | **触发前移至渲染完成**：`render`/`render_perturbed` 完成即入队（像素 `Arc` 共享不复制）；退化重试/批失败回退的旧代次失效与撤岗；取消收敛 | §0.3；编码躲到 GPU 时间与渲染段之后，暴露项降至 max(0, E−D−R) | P1 之上初估再 ≈−10~15 s（→≈85 s；§3.3 回填）；P3 落地后编码项已消失 | 旧代次缓冲错配；内存上界（§3.2 口径）；取消语义测试 | 第一档 |
| **P3** | 全页 PNG 仅 `--keep-pages` 产写；默认路径消除编码+写盘 | §0.2 结构性发现；默认成功产物集合本就不含全页 PNG | **增量 = P2 后仍暴露的编码尾差 + PNG 写盘份额**（数值由 §3.3 模型回填，不与 P1/P2 已隐藏部分重复计入） | 瞬态产物断言、外部消费方 | 瞬态断言更新 + 行为评审（独立于第一档，**无参照重基线**） |
| P4 | PNG 编码器快速压缩设置（`CompressionType::Fast`） | 图像页编码 3–10× 提速，文件增大（需实测体积增量） | 与 P1/P2/P3 叠加，图像页为主收益 | PNG 字节变更（涉 PNG 参照局部重基线）；`--keep-pages` 产物体积上升 | 第二档 |

### 3.2 编码流水线设计（BLK-1 修订新增；P1/P2 的冻结契约）

**落点选择**：编码从 CLI 识别器（`main.rs:2058-2064`、`2113-2126`）移出，收敛到 `ocrx-output` 新组件 `EncodePipeline`，由 `ScanService` 持有。`PagePngEncoder` trait 定义在 `ocrx-output`，生产实现在 `ocrx-cli`（`image` 0.25.10 默认 PNG 参数，与现行内联编码逐参数一致）——`ocrx-output` **不新增 `image` 依赖**，测试注入 encoder fake。理由：识别器 trait 只返回识别结果，编码结果走独立通道，所有权分离可测。

**数据结构拆分**：

- `PageArtifactResult` 移除 `png` 字段 → 仅识别结果（markdown/page_json/truncated/degenerated/warnings/raw_output）。
- 新增 `EncodedPagePng { generation: u64, bytes: Vec<u8> }`，由 `EncodePipeline` 持有。
- `RenderedPage` 增 `generation: u64`（scan 内单调计数，`render`/`render_perturbed` 每次调用分配；X1 文档级 FIFO 串行，scan 内唯一即全局唯一）。`TerminalPage::Committed` 携带终态 generation。
- 既有 fake 识别器/4 处 `png: vec![page as u8]` 预制测试（`lib.rs:2638/2666/2879/2893`）同步迁移为断言"commit 收到的字节来自 encoder fake"。

**任务身份**：编码任务键 = generation（已覆盖文档/窗口/页/attempt——每次重渲染都是新 generation）。提交按 `TerminalPage` 携带的终态 generation 取字节；取不到或不匹配 = 内部不变量违例 → **panic（fail-loud），不是错误面**。

**淘汰与生命周期**：

| 时机 | 行为 |
|---|---|
| 退化重试入队 attempt-2/3 | 撤销同页 attempt-1 未开始任务；在飞 deflate 不中断，完成后按 generation 丢弃 |
| 批失败 depth-1 回退 | 先撤销整窗未终结页的在飞任务，再进 `serial_attempt_outcome` |
| 取消 | `run_pages_with_model` 每个返回点前 worker 全部收敛（P1 用 `thread::scope`，scope 结束即 join；P2 池 Drop 时 join），缓冲全部丢弃，零写盘 |
| 提交完成 | bytes 被 `commit_page` 消费后即弃 |

**错误面冻结（三种语义不混用）**：

| 事件 | 语义 |
|---|---|
| 普通编码错误 | 与现状逐字一致：页级失败，`stage="recognize"` / `error_code="recognition_failed"`（现行编码错误在识别器内产生 `io::Error` → `TerminalPage::RecognizeFailed` → `classify_recognition_error`；新链路在取字节阶段映射出**相同的页诊断**，不升级为整任务 I/O 失败） |
| worker panic | `JoinHandle` Err → 扫描线程 `resume_unwind`（保持现状"编码线程 panic = 进程级 fail-loud"的可观察行为） |
| 通道断开 | 内部不变量违例 → panic |

**取消与写盘边界**：提前编码可与识别并行，但取消窗不得写 PNG/MD/JSON/checkpoint（现有 break 路径物理不动）；已完成窗口的既有提交顺序不变。

**内存峰值口径**（替代 v1.0 的"8×(24+4) MB"粗账）：新增持 = 在飞编码结果（≈0.1–0.3× 原始 RGB/页，队列上界 = 窗口深度）+ P2 的 `Arc` 共享（计数搬移，**不复制字节**；P1 的 `thread::scope` 借用窗口像素，零新增 RGB 副本）。既有副本单列不计入新增：`entries` 克隆（`main.rs:2081-2092`）、`TerminalPage::Committed` 的 image 克隆（`lib.rs:998-999`）、PDFium 预取（一期 P3）、重试渲染副本。P2.4 以 working set 实测验证全口径。

**TDD 覆盖底线（仅 fake 预制 png 字节不足以放行）**：真实 RGB PNG 编码等价（真实 `image` 编码器）；退化第 2/3 次 attempt（终态 PNG 必须来自终态渲染的 generation，旧代次缓冲不落盘）；批失败 depth-1 回退；编码失败注入（页诊断与现行输出逐字一致）；worker panic；通道断连；取消（零写盘 + 收敛）；并行 vs 串行逐字节等价。

### 3.3 统一收益模型（MAJOR-2 修订新增；M1–M3 数值唯一来源）

对每个窗口 w：`R_w` 渲染墙钟、`D_w` 解码墙钟（`step-profile.json` window_total）、`E_w` 逐页 PNG 编码墙钟（池并行口径）、`W_w` `commit_page` 写盘墙钟、`F` finalize+cleanup 墙钟。

```
现行：  T_doc ≈ Σ_w (R_w + D_w + E_w + W_w) + F          （全串行）
P1 后：  T_doc ≈ Σ_w (R_w + D_w + max(0, E_w − D_w) + W_w) + F   （编码藏进解码，池并行尾差暴露）
P2 后：  T_doc ≈ Σ_w (R_w + D_w + max(0, E_w − D_w − R_w) + W_w) + F （编码再藏进渲染段）
P3 后：  E 项与 W 的 PNG 分量归零（CPU 与写盘同时消除，不再有"隐藏/暴露"问题）
```

- P3 的收益**只等于** P2 公式中仍暴露的编码项 + PNG 写盘份额，不得重复计入已被 P1 并行化或 P2 重叠隐藏的部分；若 P2 已把编码完全隐藏，P3 增量只剩写盘与争用缓解，M3 数字相应缩小。
- M1/M2/M3 数值 = T1 实测（R/D/E/W/F）代入本模型重算**一次**；§5 中"M0/M1/M2"之外不预设 M3 数字。
- ≤60 s 冲刺线（v1.0 的 M3）撤销为"M3 决策点依据本模型输出的数字或撤回"，不作为可推导验收预期。

### 3.4 执行序

`T1 → P1 → P2 → 决策点 {P3, P4}`

- T1 独立先行（BLK-2 修正后即可开工，插桩点在现行代码位置，P1 落地时插桩点随编码迁入 pipeline，字段语义与 schema 不变）；
- P1 先行：数据拆分 + 有界并行 + 识别入口触发一次到位，为 P2 提供池设施与 generation 通道；
- P2 在 P1 后实施：仅移动触发点并补旧代次失效；
- P3 是独立决策点：评审材料 = T1 消除量实测 + resume/cleanup/审计不读全页 PNG 的代码级证据 + 外部消费方排查；
- P4 仅在 `--keep-pages` 用户反馈体积/速度痛点时启动。

## 4. 阶段计划（TDD）

### T1 提交段插桩（≈0.5–1 天；改动 crate：`ocrx-output` 为主、`ocrx-cli` 仅落盘点；**不触 `ocrx-runtime`**）

| 项 | 内容 | 产出 |
|---|---|---|
| T1.1 | `CommitProfile` 新类型（ocrx-output）：逐页 `{page, render_us, render_wait_us, png_encode_us, commit_write_us}` + totals + `finalize_us`；`schema_version:1` 黄金串测试锁定（新增类型与文件，runtime `StepProfile` 及其 `step-profile.json` 一字不动——`encode` 的 mmproj 语义与二期可比性保留） | 插桩合入 |
| T1.2 | 插桩点（现行代码位置）：渲染循环（`lib.rs:868-880`）与 `serial_attempt_outcome`（`lib.rs:684-689`）、编码块（`main.rs:2058-2064`、`2113-2126`）、`commit_page`（`lib.rs:2467-2504`）、finalize+cleanup（`lib.rs:1129-1137`）；env 沿用 `OCRX_STEP_PROFILE`，落盘 `commit-profile.json`（与 `step-profile.json` 同一落盘点旁，`main.rs:3170-3180`），经 `JobSummary` 附带 `Option<CommitProfile>` 回传 CLI | 插桩合入 |
| T1.3 | 通道行为冻结（黄金串 + 回归）：env 关 → 无文件、零开销；`run_pages_with_model` Err → 不落盘（与 step-profile 同语义）；cancelled Ok → 照常落盘（只含已终结页）；写失败 → stderr 警告不致命；多文档 → 每文档 root 各一份，**通道不做跨文档聚合**，聚合由分析脚本对逐文档文件求和 | 行为测试归档 |
| T1.4 | md-small-20 + 金集各跑一遍：R/D/E/W/F 实测 → 回填 §3.3 模型 → **P2/P3 目标值修订一次**（偏差 >20% 触发条款）；墙钟闭合校验（render+wait+encode+write+finalize+窗口解码+残差 vs 文档墙钟，±5%，不闭合先找漏段再谈目标） | `docs/test/reports/phase3-t1-<date>-data/` 归档 |

### 阶段 P1（≈2 天；改动 crate：`ocrx-output` + `ocrx-cli`；TDD：单测先行）

| 项 | 内容 |
|---|---|
| P1.1 | 数据拆分：`PageArtifactResult` 移除 `png`；`RenderedPage` 增 `generation`；`PagePngEncoder` trait（ocrx-output）+ `ocrx-cli` 生产实现（`image` 0.25.10 默认参数，与现行内联编码对合成 RGB 逐字节一致的单测锁定）；fake 识别器测试设施迁移（`lib.rs:2638/2666/2879/2893` 等） |
| P1.2 | `EncodePipeline`（ScanService 持有）：`std::thread::scope` 有界池（并发 = 窗口深度），识别入口整窗入队，与同窗解码并行；两条提交路径（`commit_terminal_pages` `lib.rs:1585` 与内联循环 `lib.rs:1015-1126`）按 (page, generation) 取字节，`commit_page` 物理不动；scope 结束即收敛 |
| P1.3 | 契约测试（encoder fake 注入，逐项对齐 §3.2 冻结表）：编码错误 → 页级 recognize/recognition_failed 诊断逐字一致；worker panic → fail-loud；断连 → panic；generation 不匹配 → panic；取消 → 零写盘 + 收敛 |
| P1.4 | 门禁：真实 RGB PNG 编码等价 + 金集两遍逐字节 + md-small-20 两遍计时；编码挤占观察（T1 数据对比编码时长是否因渲染/喂数竞争恶化） |

### 阶段 P2（≈1 天；改动 crate：`ocrx-output`/`ocrx-cli`）

| 项 | 内容 |
|---|---|
| P2.1 | 触发前移：`render`/`render_perturbed` 完成即入队（serial 与 batched 两路径、含重试 attempt）；像素交接 `Arc` 共享（不复制字节）；池生命周期跨越 render→commit，Drop 收敛点冻结 |
| P2.2 | 旧代次失效：退化 attempt-2/3 入队即撤同页旧任务（在飞不中断、完成按 generation 丢弃）；批失败 depth-1 回退先撤整窗未终结页在飞任务；专项测试：终态 PNG 必须匹配终态 attempt |
| P2.3 | 取消语义：解码中取消 → 在飞编码缓冲全部丢弃、零写盘（机制 A 契约锁定，Ctrl-C 冒烟断言扩展覆盖编码 worker 在飞态） |
| P2.4 | 门禁：金集逐字节两遍 + md-small-20 计时；内存峰值实测（working set 口径，对账 §3.2 全清单） |

### 决策点 P3 / P4（各 ≈1 天，不预设合入）

- **P3**：门控全页 PNG 产写于 `--keep-pages`（`--keep-pages` 开启时产物与现状逐字节一致）。评审输入 = T1 消除量实测 + resume/cleanup/审计不读全页 PNG 的代码级证据 + 外部消费方排查。通过 → 更新瞬态 PNG 相关断言（冒烟/故障注入）+ "resume 不读全页 PNG"回归测试 + README/使用指引更新；**不重基线默认成功/`--keep-pages` 冻结参照**（MAJOR-1）。
- **P4**：快速压缩设置数据实测（体积增量/速度比）；涉 PNG 参照局部重基线 + 第二档门禁；仅当 P3 未过且 `--keep-pages` 仍有痛点时启动。

## 5. 里程碑

| 里程碑 | 内容 | md-small-20 |
|---|---|---:|
| M0 现状（二期落点） | 编排 + depth 8 | 114.2 s |
| M1 +P1 | 流水线化 + 识别入口触发的并行编码 | 初估 ≤99 s（§3.3 回填） |
| M2 +P2 = 主目标 | 编码藏进解码+渲染段 | 初估 ≤85 s（§3.3 回填；主验收线） |
| M3 +P3（决策点） | 消除过路 PNG | **无预设数字**：由 §3.3 模型对"暴露编码尾差 + PNG 写盘份额"给出或撤回 |

金集口径同步小幅受益（编码段 ≈0.2–0.3 s/页，初估 −2~3 s → ≈31–32 s；同模型回填）。

## 6. 风险登记

| 风险 | 缓解 |
|---|---|
| T1 实测推翻 §0.2 量级推断（编码占比 <30%） | §3.3 模型回填；P2/P3 目标值按实修订；<2% 项按规则回滚不硬凑 |
| P1 数据拆分波及面超预估（trait/fake/测试设施） | P1.1 单独成项先行合入；fake 迁移与生产实现同 PR 评审 |
| P1/P2 并行编码挤占渲染 owner 线程或解码喂数 | T1 数据先行；池并发上界 = 窗口深度；两遍计时对比裁决 |
| 旧代次缓冲错配（退化/回退后提交错误 attempt 的 PNG） | generation 键 + 不变量 panic + P2.2 专项测试（终态匹配） |
| 取消语义被 P2 破坏（缓冲写盘早于取消门） | P2.3 专项测试 + Ctrl-C 冒烟扩展；写盘代码路径物理不动 |
| worker panic / 断连被吞成静默页失败 | §3.2 错误面冻结表 + P1.3 逐项测试（panic 路径 resume_unwind） |
| P3 破坏外部消费方（脚本/审计依赖 page-*.png） | 评审前仓库内排查 + 使用指引公告；`--keep-pages` 完整保留 |
| P4 参照重基线引入漂移误判 | 重基线独立提交，仅涉 PNG 参照；重基线前后非 PNG 产物逐字节比对 |
| 内存峰值超口径 | P2.4 working set 实测对账 §3.2 清单；超口径回退触发点前移范围 |

## 7. 验收清单（DoD）

- [ ] T1 两份数据落档 + §3.3 模型回填 + P2/P3 目标值修订一次（若偏差 >20%）+ 墙钟闭合校验记录
- [ ] P1/P2 每项：`cargo test --workspace` 全绿 + fmt/clippy 零警告 + 金集逐字节两遍门禁报告归档
- [ ] §3.2 契约测试全项归档：编码错误映射 / worker panic / 断连 / generation 错配 / 取消收敛
- [ ] md-small-20 主目标 ≤85 s（T1 修订后口径；未达则逐项判定报告）
- [ ] 取消语义专项测试 + Ctrl-C 冒烟扩展归档（P2）
- [ ] P3/P4 决策文档（做/不做 + 数据）；P3 若通过：瞬态断言更新记录 + resume 回归测试归档（无全参照重基线）
- [ ] 阶段报告：`docs/test/reports/phase3-<stage>-<date>.md` 系列 + 总报告

## 8. 排期估计（单人）

T1 ≈0.5–1 天 → P1 ≈2 天 → P2 ≈1 天 → 决策点 ≈1–2 天。总计 **≈5–7 个工作日**。

## 9. 修订记录

- v1.0（2026-11-04）：初版。范围 = 二期 X1.4④ 未达标归因（`cross-doc-orchestration-2026-11-04.md` §2④）承接的提交路径流水线化杠杆 P1–P4 + T1 拆分前置；机制 A 写盘原子性、X1 文档级 FIFO 边界、解码地板均为非目标。
- v1.1（2026-11-05）：**量化前提重大修订（本方案 §2 规则 1 的偏差 >20% 条款触发）**。实现评审 BLK-2 整改后重采：md-small-20 编排 = 26.2 s（5.9×），每文档摊薄后真实工作 floor ≈1.3 s（渲染+解码 ≈1.0 s+PNG 编码+提交）。v1.0 §0.2 的"渲染+PNG 编码 ≈1.5–2 s/页"推断被证伪——初版 X1 测得 ≈4.6 s/文档非窗口成本的主体是**逐文档模型校验**（已由 BLK-2 消除），不是提交路径。后果：① P1/P2（编码并行/重叠）与 P3（过路 PNG 消除）在多文档场景的剩余收益上界收缩至每文档 ≈0.3–0.5 s（≈15–25% md-small-20 墙钟），金集口径（0.2–0.3 s/页文本页）不受影响仍需 T1 实测；② 三期立项依据从"多文档 82% 非窗口瓶颈"修正为"单文档 9.7 s 非解码段 + 多文档 ≈6 s 残余提交成本"，价值下调但方向不变；③ T1 仍是前置门禁，P1/P2/P3 目标值按实修订后重新评审。
- v1.1（2026-11-04）：按 v1.0 评审（BLOCK）修订——
  - **BLK-1**：§0.2 重写为已核对的真实调用链（PNG 编码在 `ocrx-cli` 识别器 `main.rs:2058-2064`/`2113-2126`、识别返回之后；`commit_page` 只写盘；md-small-20 与金集 depth>1 窗口走 `lib.rs:1009-1126` 内联提交而非 `commit_terminal_pages`）；新增 §3.2 编码流水线设计，冻结落点（编码移入 `ocrx-output` `EncodePipeline`，encoder trait 生产实现在 `ocrx-cli`）、数据拆分（`PageArtifactResult` 去 `png`、generation 身份）、四类淘汰时机、三分离错误面（编码错误=页级 recognize 诊断、panic=fail-loud、断连=panic）、取消收敛与内存全口径；P1 重定义为"数据拆分+流水线地基"，P2 重定义为"触发前移+旧代次失效"；TDD 底线加入真实编码、退化 attempt、批失败回退、panic/断连/取消。
  - **BLK-2**：T1 重设计——runtime `StepProfile` 及其 `encode`（mmproj）语义一字不动；新增 `ocrx-output` `CommitProfile` 与独立 `commit-profile.json`（沿用 `OCRX_STEP_PROFILE` 开关），逐页 schema 黄金串锁定，所有权（ScanService 累计、JobSummary 回传、CLI 落盘）、空值/错误/取消/多文档行为冻结，并要求墙钟闭合校验；T1 落点 crate 修正为 `ocrx-output` 为主。
  - **MAJOR-1**：§2 规则 3 重写——P3 不重基线默认成功与 `--keep-pages` 冻结参照（默认成功产物集合本就不含全页 PNG），门禁改为瞬态断言更新 + resume 回归测试 + 行为评审；figure crop 单独陈述并移出范围。
  - **MAJOR-2**：新增 §3.3 统一 critical-path 模型（max(0, E−D−R) 暴露项），M1–M3 数值唯一来源 = T1 实测回填；撤销 ≤60 s 冲刺线为 M3 决策点输出；P3 收益不得重复计入 P1/P2 已隐藏部分。
