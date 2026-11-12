# OCR-CLI 模型后端可扩展架构重构详细开发方案

**文档状态：** 待实施（修订 v7.6，2026-11-11——六审响应（`docs/review/model-backend-extensibility-refactoring-development-plan.main-v7.5-review.md` RBLK-7/8/9 与注记 2 项）：① display 语义唯一化（RBLK-7，采纳评审选项 2）：`ModelDisplayMeta` 裁定为**报告专用字段**——checkpoint 存储完整身份时原样保存（审计），但预比较与终比较显式排除，展示文案/量化展示名变化不触发重处理；`StaticInferenceIdentity` 维持不含 display 并注明排除理由，"全字段"统一收窄为"全部执行字段"；区分性测试落 T045/T055/T100，T017 补"静态身份 ∪ 实际 backend/批深 = 执行身份 − display"字段完备性断言；② 终比较适用范围限定（RBLK-8，采纳评审首选）：终比较仅适用于**本文档存在待解码页、即将用该 session 执行新解码**的情形；完全复用的后续文档不进入终比较——静态预比较通过且无待解码页即以零解码为终局，不因前文档已建共享 session 而重处理（纯复用规则与"编排与逐个调用产物一致"承诺优先，跨文档各自保持与其单跑一致的执行形态）；"需要解码的后续文档与共享 session 全字段比较"保留（RBLK-6 防护不变）；专项测试重述并新增"前文档已建共享 session + 后文档完全复用且存储实际身份不同 → 零新解码复用且编排/单跑一致"（T055/T063/T100/§9.2/§9.3）；③ verify 绑定校验无环通道（RBLK-9，采纳评审选项 1）：`BindingVerifier` 端口定义于 inference-api（与 manifest DTO 同 crate），`ocrx-inference` 依据静态 registry 提供实现，组合根调整顺序（catalog → probe/设备/runtime factory → registry → verifier 实现 → 注入构造 `ModelManagerImpl` → `InferenceServiceImpl` → `ScanApplication`）注入 `ModelManagerImpl`；models 只调端口（测试用 fake），不反向依赖 inference；prepare 与 `model verify` 共用同一绑定校验语义；端到端测试落 T072（资产正确但 registry 身份任一不匹配 → verify 失败 IncompatibleBinding），T089 断言无 models → inference 依赖；④ 注记 1：session 无独立 `decode_generation()` 查询，代次经 `identity()` 暴露，service 静态查询与 registry 同源（§8.8 修正）；注记 2：后续文档"静态预比较 vs 共享 session 比较"的适用路径已随 ② 统一表述。v7.5，2026-11-11——五审响应（RBLK-5/6）：① result 展示元数据跨层通道（RBLK-5）：`ModelDisplayMeta`（display_name/family/quantization）加入 `InferenceIdentity.display`，prepare 时自**实际解析的那份 manifest** 一次性填充，经 session 身份到达 scan——§7.4 派生表三行改读该通道，scan 不依赖 ocrx-models、无展示映射副本，纯复用零加载无 session 时 result 模型对象为 null 不需此通道；同步 §5.1/§6.3 第七步/§7.4/§8.8 与 T014/T066/T075（四 profile golden）；② auto 改选混用防护（RBLK-6）：终比较升级为**全字段权威复核**——以即将执行解码的 session 完整 `InferenceIdentity` 对候选页存储身份做平台侧全字段比较（模型/profile/逐资产/runtime/adapter/代次/实际 backend/批深），任何不一致（含 auto 在预比较后、prepare 时改选 profile）全部重处理，绝不跨 profile 混用；预比较降格为零加载快速路径（纯复用终局语义不变）；多文档编排下后续文档直接与共享 session 完整身份比较，不经 `static_identity()` 二次解析，消除漂移窗口；§5.5 明示两次 auto 解析不保证相同；同步 §5.5/§6.3 第三/五步/§7.1/§8.8/§9.2 与 T055/T063/T100（auto 改选与共享 session 漂移两项专项测试）。v7.4，2026-11-11——三审响应（RBLK-4）：恢复预比较获得完整静态身份来源——新增 `InferenceService::static_identity()` 只读查询与 `StaticInferenceIdentity` DTO（§5.5）：catalog 解析含 `auto` 按冻结内存推荐规则解析为具体档位（与 prepare 实际解析共用同一推荐实现）、内置 manifest 逐资产元数据、静态 registry 的 runtime/adapter 绑定值与解码代次；明确不校验资产文件、不安装、不创建 runtime、不做设备初始化，实际 backend/批深为终比较专用字段；§6.3 第五步“跳过 profile 实际解析”措辞协调为“跳过资产文件校验/安装/runtime 创建（只读元数据查询不属此列）”；同步 §5.1/§6.3/§7.1/§8.8/§9.2 与 T017/T055/T057/T100，补默认 auto 完全复用零加载、auto 解析结果变化重处理、静态身份变化预比较失败三项测试。v7.3，2026-11-11——重审响应（`docs/review/model-backend-extensibility-refactoring-development-plan.main-v7.1-review.md` 重审节 RBLK-1/2/3 与非阻断 2 项）：① `decode_echo` 数据通道唯一化（RBLK-1）：backend 把实际生效变体填入 `BackendOutput.decode_echo`，adapter decode 原样透传进 `TaskResponse`——不从请求拷贝、不默认填空，fake 以“回执 ≠ 请求变体”证明来源（§5.3/§5.4/§8.4/§8.6/§8.7/T016/T031/T035/T042）；② OCR 恢复两阶段协议（RBLK-2）：预比较（prepare 前，除实际 backend/批深外全字段，代次经 `InferenceService::decode_generation()` 静态查询，不触发加载）+ 终比较（存在待解码页时 prepare 后以 session 实际身份复核候选页，不一致移入重处理）；全部候选通过且无待解码页 → 整文档复用、零 prepare 零加载（零解码无混执行形态风险，存储身份即终局）；“请求 GPU 回退 CPU 所写 checkpoint 在本次实际仍为 CPU 时复用”语义由终比较实现（§5.1/§5.5/§6.3 第三/五步/§7.1/T055/T100）；③ StepProfile 进公共 API（RBLK-3）：类型与 `delta_from` 语义迁至 inference-api（与 `RuntimeTelemetry` 同类诊断 DTO），`InferenceSession::step_profile() -> Option<StepProfile>` 暴露累计快照（env 关闭 None），采集实现仍留在 runtime，scan 一律经该查询做逐文档差分，禁止引用 `ocrx_runtime::StepProfile`（架构检查项）（§4.1/§5.3/§8.6/§8.8/T015/T017/T042/T067/T089）；④ 切栏页合并规则冻结（重审非阻断 1，基线语义）：`raw_text` 按栏序直接连接、`think_blocks_stripped` 求和、退化/截断保守合并、warnings 按栏序追加、条带不产生 figure 裁剪（§2.3/§6.3/§8.10/T063/§9.2）；⑤ “同参重扫逐字节比对”评估原语获得执行落点（重审非阻断 2）：§8.12 内置原语 + T082/T102 验收。v7.2，2026-11-11——评审响应：`docs/review/model-backend-extensibility-refactoring-development-plan.main-v7.1-review.md` BLOCK 6 项与非阻断 3 项全数修订。① 重试契约同步声明基线组合阶梯（BLK-1）：attempts 1..=4（读取兼容更宽）、Q1.4a 冻结阶梯数据表（2048 网格 → repeat_penalty 1.2×/64 → 切栏，扰动 rung 已被实测剔除）、纯截断页入阶梯（Q1.5/D-14）、切栏多子请求与按栏序保守合并、`--column-split-retry` 类型门控，全部补入 §5.3 请求契约与 T015/T063 等任务；② 解码代次与实际批深进入结构化身份（BLK-2/BLK-4）：`InferenceIdentity` 与 `ScanCompatibility.ocr` 增设 `decode_generation`（平台产生、扫描透传）与 `batch_depth`（实际生效值），wrapper/sys/upstream 冻结值统一为 0.1.161/0.1.161/`d81235049384534c167caea52b85a694f6103d14`（经 wrapper repo tag 0.1.161 的 llama.cpp submodule 指针核实），T034/T038/T075/T100 同步验收；③ 响应 DTO 双文本（BLK-3）：`TaskResponse` 同时携带清理后 `text`、剥离前规范化 `raw_text`（raw sidecar 数据源）、`think_blocks_stripped` 计数与解码回执 `decode_echo`（采样元数据），v6 checkpoint/result 聚合与 golden 补齐；④ 批深唯一冻结 8/16 分层（12 GiB 阈值、GPU 回退重选 8），实际批深经 prepare/session 契约传递再由 scan 写入身份；⑤ §5.8 裁定唯一化（BLK-5）：多模型缺口评估整体移出 v2 范围（AWQ 维持 not-now），v2 契约以 §5.1–§5.5 执行变体直载为唯一冻结；⑥ 恢复 X1 跨文档编排与 T1 插桩的迁移与验收条款（BLK-6）：§1.1/§2.1/§4.1/§6.3/§8/§9/§11 与 T003/T007/T052/T053/T058/T063/T066/T074/T075/T078/T099 补齐多文档编排、`scan-summary.json`、StepProfile/CommitProfile、退出码聚合；另按评审修正 §9.3 计数等式、§5.8 待调研注记与 v5 字段盘点（`decode`/`think_blocks_stripped`）。v7.1，2026-11-11——OvisOCR3/OXR 调研补入 §5.8（OvisOCR3 同构升级候选 GGUF 已有；OXR 需核实 llama.cpp 可运行性）；v7 新增 §5.8 多模型接入缺口评估（以 OvisOCR3/Xiaomi-OCR-0/MiniCPM-OCR 为用例：P0 规范中间层/网格能力声明/AWQ 政策，P1 prompt 身份/采样映射/块类型扩展/评估归一化）；v6 §3.4 phase-4/质量轮六项实测模式回灌；v5 迁移基线 main `9e0dc29` + pins 0.1.161；v2，2026-11-03——迁移输入基线推进到 main `9e0dc29`（2026-11-11），自 v2（2026-11-03，main `a02d459`）以来的增量全部并入：① 吞吐优化二期/三期/四期落地与收官（批深解冻至 8 + VRAM 分层 16、跨文档编排 X1、插桩通道 T1、S0/BS/P1/P2/P3 no-go、CUDA 集成 NO-GO、滚动预取、PNG 编码出关键路径）；② 质量优化轮全部落地（组合退化阶梯/decode 代次键/strip_think_blocks/链接降级/纯截断重试，Overall 77.99 → 87.85）；③ wrapper pins 修订 0.1.152/0.1.153 → 0.1.161/0.1.161（质量方案 D-2/Q1.3）；原 2026-10-31 范围协调注记的修订要求已并入正文）  
**迁移输入基线（v5 更新 2026-11-11，main `9e0dc29`；v2 基线为 2026-11-03 main `a02d459`，v2→v5 增量见文档状态）：** 吞吐优化方案（`docs/develop/scan-throughput-optimization-development-plan.md`）的 P3（PdfiumOwner 渲染/解码重叠）与 P4（多页合批：P4.1 推理线程 actor、P4.3 窗口解码、P4.4 窗口化扫描编排）、跨文档编排 X1（`docs/develop/cross-doc-orchestration-2026-11-04.md` 及其设计冻结稿）、插桩通道 T1（`docs/develop/phase3-t1-2026-11-05.md`）、复读退化方案（`docs/develop/repetition-degeneration-detection-recovery-development-plan.md`）与质量优化轮（`docs/develop/omnidocbench-quality-optimization-development-plan.md`：组合退化阶梯 Q1.4a、解码代次键 D-15、strip_think_blocks Q3.1、纯截断重试 Q1.5/D-14、链接降级 Q4）已全部合并进 main。本计划按其头部协调注记完成两处冻结范围修订：§2.1 的"多页串行复用"保留项已改为"以已具备合批能力的 `ocrx-runtime` 为迁移输入（批深、窗口协议以 P4/phase-2 落地形态为准）"；§2.2 的"不引入并发 OCR"已限定为"本重构不新增扫描层并发语义，但须迁移/保留 runtime 已落地的合批能力"。窗口协议机制冻结值见 `docs/develop/batched-inference-2026-11-02.md`（机制 A 窗口原子提交、`;batch=N` 兼容键、可选窗口方法最小接口面），批深以 phase-2 落地为准（VRAM 分层批深默认 8、高显存档 16，`window-depth-8-2026-11-04.md` 与 2026-11-07 深度曲线数据；`;batch=N` 兼容键现值随实际批深）。X1 冻结行为：多输入位置参数、`--output-dir` 多输入必填、`resolve_scan_inputs` 目录展开（排序 `*.pdf`，不递归）/canonical 去重、文档 FIFO、`PdfRequest::SwitchDocument`（清预取缓存并重置 `next_prefetch`；文档级错误为 Err 值跳过、owner 超时/断连 fail-loud）、`ModelSlot::Shared` 每调用恰好一次 verify+load 并跨文档复用、机制 A 与 checkpoint 逐文档独立、取消=整调用 130（已提交文档保留）、`<stem>-<hash8>` 输出目录、`scan-summary.json`（schema_version 1，写失败降级 warning）、退出码聚合 130 > 文档级失败 3 > 部分 10 > 0、`--password` 调用级；单输入路径历史形态零改动（混窗 X2 与常驻进程 B2 已裁决不做）。T1 冻结行为：`OCRX_STEP_PROFILE` 同开关驱动的 runtime `StepProfile`（含 `delta_from` 逐文档增量）与 output `CommitProfile`/`CommitRecorder`（逐页 render_wait/owner_render/png_encode/commit_write + 终态 totals + finalize，generation 键控，`schema_version: 1` 黄金串），`step-profile.json`/`commit-profile.json` 每文档一份，取消/失败零写盘，disabled 全 no-op。本重构以该落地形态为迁移输入，不回退、不重设计窗口协议与跨文档编排。  
**实施范围：** 仅重构现有 OvisOCR2 路径，不接入第二模型  
**目标读者：** 直接承担本仓库 Rust、CLI、模型资产、扫描流程和质量回归改造的开发与测试人员  
**方案性质：** 本文独立给出目标架构、业务规则、代码组织、契约、迁移方式、修改清单、验收方法和执行任务；实施时不需要借助其他设计文档才能理解本方案

---

## 1. 背景、问题与重构结论

### 1.1 当前系统实际怎样工作

当前工作区包含 `ocrx-cli`、`ocrx-core`、`ocrx-models`、`ocrx-runtime`、`ocrx-ovisocr2`、`ocrx-pdf`、`ocrx-output` 和 `ocrx-skill`。与本次重构有关的实际调用链是：

```text
ocrx-cli::run_scan / run_scan_multi（跨文档编排，X1 已落地）
  ├─ 多输入位置参数展开（resolve_scan_inputs：目录展开/canonical 去重）后由 run_scan_multi 编排：
  │    文档 FIFO、PdfiumOwner 跨文档 SwitchDocument、ModelSlot::Shared 每调用一次 verify+load、
  │    每文档 <stem>-<hash8> 输出目录与 scan-summary.json、退出码聚合（130 > 3 > 10 > 0）
  ├─ 单文档 scan_document（历史 run_scan 形态）：
  │    直接读取 PDF、分类页面、计算 native/empty/OCR 队列
  ├─ 直接选择 OvisOCR2 profile 和 CPU/GPU（批深按 VRAM 分层：默认 8、≥12 GiB 档 16）
  ├─ 直接生成 OvisOCR2 manifest、解析缓存路径、下载并校验 model/mmproj
  ├─ 直接创建 LlamaMtmdRuntime（内含 P4.1 推理线程 actor 与 P4.3 窗口解码；OCRX_STEP_PROFILE 插桩 StepProfile）
  ├─ 通过 ocrx-output::ScanService 按窗口（P4.4，批深来自 window_depth_for_vram 的实际值 8/16）渲染和识别
  │    └─ CLI 内的 OvisRecognizer
  │         └─ ocrx-ovisocr2 调用 Runtime（含 generate_window）、解析文本、剥离 <think>、裁剪 bbox、
  │              写图片与 raw sidecar（剥离/裁剪前的规范化原始输出）、累计 think_blocks_stripped
  ├─ 通过 CLI 内 pdfium_owner.rs 实现 P3 渲染/解码重叠（PdfiumOwner 线程 + 预取 + 跨文档 SwitchDocument）
  ├─ 通过 ocrx-output::commit_profile（T1）在提交路径插桩 CommitProfile（OCRX_STEP_PROFILE 同开关）
  ├─ 直接处理 checkpoint、续扫、路由 sidecar、报告和退出码
  └─ 直接合并 Markdown 并生成 result.json（多文档编排另写 scan-summary.json）
```

当前各位置的关键职责和耦合如下：

| 当前位置 | 当前实际职责 | 必须解决的问题 |
|---|---|---|
| `src/ocrx-cli/src/main.rs` | 一个约 5400 行的主文件同时承担参数解析（含多输入位置参数与目录展开）、模型管理、设备诊断、PDF 计划、页面处理、跨文档编排（`run_scan_multi`/`scan_document`/`ModelSlot`/`scan-summary.json`/退出码聚合）、runtime 生命周期、续扫、遥测和报告（另有 `pdfium_owner.rs` 约 770 行承载 P3 渲染/解码重叠与跨文档 `SwitchDocument` 协议） | 换模型或 runtime 会改动扫描业务；模型管理命令与 scan 重复实现资产逻辑；跨文档编排与扫描业务一起困在 CLI；无法独立复用扫描应用 |
| `src/ocrx-core/src/lib.rs` | PDF/页面领域类型、取消、哈希、`ModelId`、llama.cpp 兼容常量、窗口批深/批级错误常量（`WINDOW_DEPTH`/`BATCH_ERROR_PREFIX`）、错误码混放 | 通用技术原语、扫描领域和推理身份没有边界；`ModelId` 是只接受 `ovisocr2` 的枚举 |
| `src/ocrx-models/src/lib.rs` | Ovis 固定哈希/URL、两个固定资产、缓存、下载、安装、校验、设备/profile 推荐 | manifest 固定为 `model_sha256 + mmproj_sha256`，无法表达具名角色、可选组件和通用绑定；设备和模型管理混在一起 |
| `src/ocrx-runtime/src/lib.rs` | `Runtime` trait（含 P4.3 可选窗口方法 `generate_window`）、设备枚举、fake/`LoopFakeRuntime`、遥测、可选 `OCRX_STEP_PROFILE` 插桩（`step_profile.rs`，`StepProfile`/`delta_from` 逐文档增量） | `InferenceRequest` 含扫描页码却没有业务 prompt；接口不是通用模型任务契约 |
| `src/ocrx-runtime/src/llama_mtmd.rs` | llama.cpp/MTMD 加载、P4.1 推理线程 actor、图像编码、逐 token 与多槽位窗口生成 | OCR prompt 被硬编码在 backend 内，backend 因此知道扫描业务意图 |
| `src/ocrx-ovisocr2/src/lib.rs` | 调 runtime（含窗口提交转发）、解释 Ovis 文本、解析公式/表格/bbox、截断、复读退化解析、裁剪 PNG、替换图片链接、生成页 JSON | 模型协议、扫描业务解析和文件输出耦合；该 crate 直接依赖 runtime 和 PDF |
| `src/ocrx-output/src/lib.rs` | 输出、checkpoint、页面生命周期、扫描服务、P4.4 窗口编排与机制 A 原子提交、复读退化重试状态机、渲染/识别抽象（含外部持模型窗口循环入口 `run_pages_with_model`，X1 共享槽使用）、OCR 队列、可选 `CommitProfile`/`CommitRecorder` 插桩（`commit_profile.rs`） | output crate 反向拥有扫描编排（含窗口编排与提交原子性）；兼容身份是字符串拼接，缺少资产、runtime、adapter、prompt 的结构化身份 |
| `src/ocrx-pdf/src/lib.rs` | PDFium、分类、native 提取、渲染、bbox，同时还包含质量清单、评分、发布门禁和报告 | PDF 基础设施拥有质量评估业务，评估无法作为独立顶层能力复用扫描或推理 API |
| `scripts/` 与 `assets/regression/` | 样本、真值、预测、评分、质量门禁和报告 | 能力已经存在，重构应迁移/包装并保持格式与评分语义，不能另造一套评估体系 |

### 1.2 重构结论

重构后形成两个严格分离的子系统，并保留一个独立的 CLI 组合入口和一个独立的质量评估模块：

1. **推理平台**只负责模型目录、资产安装与校验、设备诊断、runtime/adapter 兼容绑定、模型加载和通用任务执行。它不认识 PDF、页码、native-text、OCR 队列、checkpoint、Markdown、bbox 图片或扫描报告。
2. **扫描应用**只负责 PDF 业务：输入校验、页面计划、native/empty/OCR 路由、OCR prompt、页图像任务、响应到页面文档的解释、跨页状态、续扫和最终产物。它只调用推理公共 API，不接触 llama.cpp、缓存文件布局或具体模型 adapter。
3. **CLI**只负责解析、交互、组合具体实现、展示和退出码。CLI 可以依赖具体组合 crate 来创建服务，但不再实现模型管理或扫描业务。
4. **质量评估与工具**拥有真值、评分、评估身份和报告。端到端评估复用扫描应用，模型专项评估复用推理 API；不复制任一流程。
5. 本次只让现有 OvisOCR2 通过新架构运行。模型、adapter、runtime 的注册均为编译期静态注册，不实现动态插件发现，不接入 Qwen3.8，不升级 llama.cpp，不改变 OvisOCR2 权重。

这次重构不是“给旧函数换名字”。完成时必须由 Cargo 依赖方向和源码检查共同保证：扫描 crate 无法引用具体 runtime/model 实现，推理平台无法引用 PDF/扫描/输出/评估类型。

---

## 2. 目标、非目标与行为基线

### 2.1 必须实现的目标

- 在不增加第二模型的情况下，建立可扩展的模型资产、协议 adapter、runtime backend 和兼容绑定机制。
- 模型 `list/info/recommend/install/verify/remove` 与 `doctor` 全部通过独立推理管理 API 完成，不创建扫描任务，也不需要 PDF。
- 扫描应用只依赖 `ocrx-inference-api`；业务 prompt 从扫描应用发出，页码在扫描应用保留，通用推理请求不包含页码。
- OvisOCR2 的四个 profile、固定 revision、URL、哈希、文件大小、缓存共享方式、图像预算、prompt、生成参数和页面解释规则保持不变。
- 保留 native-first、auto 模式 native 校验失败转 OCR、text 模式失败、empty 页、按需加载模型、单次调用单次加载（多文档编排下 `ModelSlot::Shared` 每调用恰好一次 verify+load 并跨文档复用；单输入保持历史 PerCall 形态）、GPU 初始化失败回退 CPU、取消、失败继续、checkpoint 续扫和原子提交语义。
- 以已具备合批能力的 `ocrx-runtime` 为迁移输入，原样保留 P4 落地形态的多页窗口推理语义：VRAM 分层批深（默认 8、高显存档 16，phase-4 tiering，12 GiB 阈值；GPU 回退 CPU 后重选为 8）、按页序填充窗口、不做动态回填、窗口上下文按 Σ(各页图像 token + 提示 + 输出上限) 动态计算、批级失败整窗批深回退 1 重提、机制 A 窗口原子提交（取消的在飞窗口零提交、恢复整窗重扫）、相同（输入、批组合、批深）下输出在同一解码代次内逐字节可复现。多请求能力收敛为推理 API 的可选窗口方法（最小接口面），不重设计窗口协议。
- 保留质量优化轮已落地的组合退化阶梯行为（BLK-1 契约同步）：`repetition_loop` 终止原因、组合退化阶梯为扫描应用数据表驱动（`DegenerationRetryStep`，Q1.4a 冻结：attempt 1 纯贪心 1024 网格 → `image_tokens=2048` → `repeat_penalty 1.2×/window 64` → 切栏末位档；扰动渲染 rung 已被实测 0 逃逸剔除出冻结表）、attempts 状态机 1..=4（= 1 + 阶梯长度；checkpoint 读取兼容更宽值如 5，Q1.4b）、attempt 1 随窗口批处理而退化页以变体请求单独按批深 1 重试、纯截断页同样进入阶梯（Q1.5/D-14）、切栏档把一页分解为 N 个条带子请求并按栏序合并、页级退化/截断状态取各子请求的保守合并（任一子请求环/截断 → 页未脱环）、切栏页合并规则冻结（基线语义）：`raw_text` 按栏序直接连接（无分隔符）、`think_blocks_stripped` 求和、退化/截断状态保守合并（OR）、warnings 按栏序追加，切栏条带不产生 figure 裁剪（bbox 标签丢弃）、切栏档受 `--column-split-retry` 类型门控（默认开；公式密集文档集实测有害应关闭）、`degenerated` checkpoint 字段与合并文档不完整标记、`page-NNNN.raw` 原始输出 sidecar（内容为 `<think>` 剥离与尾部裁剪之前的规范化原始输出，Q3.1）与 `think_blocks_stripped` 清理计数。
- 保留跨文档编排（X1）已落地行为：多输入位置参数（选项可交错）、多输入必填 `--output-dir`、`resolve_scan_inputs` 目录展开（排序 `*.pdf`，不递归）/canonical 去重/缺失或目录无 PDF 报用法错误、文档 FIFO 编排、`PdfRequest::SwitchDocument`（清预取缓存并重置 `next_prefetch`，文档级错误为 Err 值跳过、owner 超时/断连维持 fail-loud）、机制 A 与 checkpoint 逐文档独立（窗口按文档组成，不跨文档混窗）、取消=整调用 130（已提交文档保留）、`<stem>-<hash8>` 输出目录、`scan-summary.json`（schema_version 1，写失败降级 warning）、退出码聚合 130 > 文档级失败 3 > 部分 10 > 0、`--password` 调用级；单输入路径历史形态零改动（缺失文件仍为页数失败 exit 3）。
- 保留可选插桩通道（T1）：`OCRX_STEP_PROFILE` 同开关驱动的 runtime `StepProfile`（含 `delta_from` 逐文档增量）与 output `CommitProfile`/`CommitRecorder`（逐页 render_wait/owner_render/png_encode/commit_write + 终态 totals + finalize，generation 键控，`schema_version: 1` 黄金串），`step-profile.json`/`commit-profile.json` 每文档一份，取消/失败零写盘，disabled 全 no-op；插桩随各自所有者迁移，黄金串与语义不变。
- 保留 P3 渲染/解码重叠：PdfiumOwner 线程所有权、预取协议和扰动渲染转发随渲染路径迁入扫描应用，语义不变（扰动渲染 rung 不在 Q1.4a 冻结阶梯表内，但渲染能力随 P3 原样保留，供阶梯表未来调整）。
- 保持 `document.md`、页 Markdown/JSON、图片命名、`result.json`（含 `pages.think_blocks_stripped_total`）与 `scan-summary.json` 的对外 schema、CLI 参数（含多输入位置参数、`--output-dir` 语义与 `--column-split-retry`）、stdout/stderr 分工和退出码兼容。
- 将 checkpoint 兼容身份升级为结构化、可审计身份；对旧 v5 checkpoint 采取安全且明确的迁移行为。
- 默认开发测试完全可使用 fake runtime、受控本地资产和仓库内 fixture 完成，不下载模型、不访问外网、不要求 GPU。

### 2.2 明确不做的事项

- 不增加 Qwen3.8 或任何第二模型，不验证 Qwen 的 GGUF、mmproj、chat template 或 llama.cpp 兼容性。
- **wrapper pins 已修订（2026-11-10，质量方案 D-2/D-15/Q1.3）**：`llama-cpp-2 = 0.1.161`、`llama-cpp-sys-2 = 0.1.161`（原冻结 0.1.152/0.1.153）。解除依据：per-sequence sampler API 为退化重试采样链的唯一载体；里程碑/终验实测 Overall 87.85（超 GO 线 +2.85），公式 CDM 0.7925 → 0.9443（+15.2 分量点）。正常页跨代次漂移按解码代次契约处置（`;decode=2` 兼容键写入 checkpoint 兼容参数，旧代次 checkpoint 一律 Reprocess，不混扫）。迁移以 0.1.161 落地形态为输入；**上游提交基点冻结为 llama.cpp `d81235049384534c167caea52b85a694f6103d14`**（2026-10-05 "bump version to 0.6.0"，经 wrapper 仓库 tag `0.1.161` 的 `llama-cpp-sys-2/llama.cpp` submodule 指针核实；旧值 `9e3b928` 属 sys 0.1.153 代次，随 pins 修订一并退役，见 phase3-s0-wrapper-audit-2026-11-05.md）。
- 不实现动态模型注册、第三方插件加载、任意脚本执行或模型热插拔。
- 不改变 PDF 分类算法、native 文本校验算法、OCR 质量阈值和评分公式。
- 本重构不新增扫描层并发语义：不引入分布式任务、异步服务进程或新的网络服务；runtime 已落地的窗口合批（P4，批深 8/16 分层）与跨文档 FIFO 编排（X1，串行非并发）按原语义迁移保留，不重设计、不改变批深分层，也不新增用户批深旋钮；混窗（X2）与常驻进程（B2）均已裁决不做，本重构不重新打开。
- 不重写既有 Python 评分器，不伪造真值，不重新生成历史预测来掩盖行为差异。
- 不把真实模型下载、真实 GPU、外部 CI、外部模型评测或 Qwen 冒烟设为本机开发和测试的前置门禁。

### 2.3 必须冻结的 OvisOCR2 行为

- 模型 ID 为 `ovisocr2`，profile 为 `compact/balanced/quality/reference`，`auto` 的内存推荐规则和设备优先级 `metal > cuda > vulkan > cpu` 不变。
- 固定社区 revision 为 `8a22290a0c42dbcf84739d6d4c2763f877494ae0`；现有四个主模型和共享 mmproj 的 SHA-256、字节数、URL 逐项迁移，不能在重构时重新从网络“发现”元数据。
- 现有缓存布局继续使用：主模型位于 `<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf`，共享视觉资产位于 `<cache_root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf`。重构不能迫使用户重复下载已校验资产。
- 默认输出 token 上限 16384（参数范围 128–16384，复读退化方案 D5.2 冻结值），图像 token 默认 1024，context 默认 20480，DPI 默认 300，模型像素预算 `2880 × 2880`，贪心解码、thinking 关闭。
- 多页窗口推理参数冻结为当前落地值：VRAM 分层批深（默认 8、高显存档 16，phase-4 tiering，阈值 VRAM ≥ 12 GiB；GPU 回退 CPU 后重选为 8），实际批深与 `;decode=2`（D-15 代次键）参与 checkpoint 兼容身份，跨批深/跨代次不互恢复；单流路径沿用 `context_size`（默认 20480），窗口上下文独立创建。
- 组合退化阶梯冻结为扫描应用数据表（Q1.4a）：attempt 1 纯贪心（1024 网格）→ rung 1 `image_tokens=2048` → rung 2 `repeat_penalty 1200 milli / window 64`（采样链顺序：penalty 先改 logits、greedy 终选）→ rung 3 切栏（仅末位）；attempt 1 之外的每次重试以批深 1 单独提交；纯截断页与退化页同等进入阶梯；切栏档受 `--column-split-retry` 门控（默认开）。
- OCR prompt 必须从 backend 原样移动到扫描应用，版本固定为 `ocr-markdown-v1`。其完整文本为：

```text

Extract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src="images/bbox_{left}_{top}_{right}_{bottom}.jpg" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.
Preserve the original text without translation or paraphrasing.
```

代码中保留开头和中间的换行，不得重新排版 prompt 字符串。测试必须用字节相等断言防止无意间改变模型输入。

---

## 3. 重构后的总体架构

### 3.1 架构图

```text
┌──────────────────────────── ocrx CLI ────────────────────────────┐
│ 参数/配置优先级 │ 终端交互 │ 组合根 │ 文本/JSON 展示 │ 退出码 │
└──────────────┬───────────────────────┬──────────────────────────┘
               │ ScanApplication       │ ModelManager / DeviceManager
               ▼                       ▼
┌──────────────────────────┐   ┌───────────────────────────────────┐
│ 扫描应用 ocrx-scan       │   │ 推理组合 ocrx-inference           │
│ 输入与页面计划           │   │ 静态 catalog/binding/factory 注册 │
│ native/empty/OCR 处理    │   │ ModelManagerImpl                  │
│ OCR prompt 与页面解析    │   │ DeviceManagerImpl                 │
│ 页面状态、恢复、最终报告 │   │ InferenceServiceImpl              │
└───────┬──────────┬───────┘   └───────────────┬───────────────────┘
        │          │ InferenceService           │ 内部组合
        │          ▼                            ▼
        │  ┌──────────────────────────────────────────────────────┐
        │  │ ocrx-inference-api：通用、稳定、无 PDF 的公共契约   │
        │  │ TaskRequest/TaskResponse/Identity/管理接口/错误      │
        │  └───────────────┬──────────────────────────────────────┘
        │                  │ 由推理实现和扫描共同依赖
        │                  ▼
        │  ┌──────────────────────────────────────────────────────┐
        │  │ 推理平台内部实现                                    │
        │  │ ocrx-models：manifest/catalog/cache/install/verify   │
        │  │ ocrx-ovisocr2：Ovis 消息协议 adapter，无 OCR 解析   │
        │  │ ocrx-runtime：llama.cpp/MTMD backend 与 fake         │
        │  └──────────────────────────────────────────────────────┘
        ▼
┌──────────────────────────┐   ┌───────────────────────────────────┐
│ ocrx-pdf                 │   │ ocrx-output                       │
│ PDFium/分类/native/渲染 │   │ checkpoint/原子产物/合并/报告    │
└──────────────────────────┘   └───────────────────────────────────┘

┌──────────────────────────── ocrx-eval ───────────────────────────┐
│ 样本/真值契约 │ 评分与门禁 │ 评估身份 │ 端到端或模型专项执行 │
│ 端到端调用 ocrx-scan；专项调用 ocrx-inference-api                 │
└───────────────────────────────────────────────────────────────────┘
```

### 3.2 依赖方向铁律

允许的主方向是：

```text
ocrx CLI → ocrx-scan → ocrx-inference-api
    │           ├────→ ocrx-pdf / ocrx-output / ocrx-core
    │           └────→ ocrx-primitives
    ├───────────→ ocrx-inference → models + runtime + ovis adapter + inference-api
    └───────────→ ocrx-output（展示）/ ocrx-skill

ocrx-eval → ocrx-scan / ocrx-inference-api / ocrx-core
```

以下依赖禁止出现：

- `ocrx-inference-api`、`ocrx-models`、`ocrx-runtime`、`ocrx-ovisocr2`、`ocrx-inference` 不得依赖 `ocrx-core`、`ocrx-pdf`、`ocrx-output`、`ocrx-scan`、`ocrx-eval` 或 CLI。
- `ocrx-scan` 不得依赖 `ocrx-runtime`、`ocrx-models`、`ocrx-ovisocr2` 或具体组合 crate `ocrx-inference`。
- `ocrx-output` 不得依赖 PDF、runtime、models、adapter 或扫描应用；它只保存扫描应用交给它的产物与身份。
- `ocrx-pdf` 不得依赖推理、输出、扫描或评估 crate。
- `ocrx-eval` 可以依赖扫描和 PDF 领域类型，但任何被评估模块都不得反向依赖 `ocrx-eval`。
- CLI 是唯一生产组合根。测试可以在自己的测试模块中组合 fake，不需要把具体实现重新暴露给扫描应用。

### 3.3 运行时对象边界

- `ModelManager` 是无 PDF 的管理服务，负责目录和资产。
- `DeviceManager` 是无 PDF 的设备服务，负责编译能力、硬件探测、选择和诊断。
- `InferenceService::prepare` 返回一个已经绑定模型、adapter 和 runtime 的 `InferenceSession`。
- `InferenceSession` 在一次扫描调用中只创建一次（跨文档编排下全部文档共享同一 session，对应现 `ModelSlot::Shared` 每调用一次 verify+load 的语义；单输入保持历史 PerCall 形态），连续处理多个 `TaskRequest`；每次请求都不携带 PDF 或页面上下文。窗口合批是 session 的可选批量入口：`submit_window` 一次提交一批请求，默认实现逐个串行 `submit`（数值等价单流），真实 backend 覆盖为多槽位连续合批；多请求能力收敛为 runtime 内部实现细节 + 最小接口面（`batched-inference-2026-11-02.md` §⑤）。
- `ScanApplication` 持有的是 `dyn InferenceService` 或泛型 API trait，不知道 session 内是 llama.cpp 还是 fake。
- `ProtocolAdapter` 只负责文本/图像如何进入模型协议以及通用响应如何解码；`OcrPageParser` 只负责把通用文本变成扫描领域页面，二者不是同一个对象。

### 3.4 架构反馈修订（v6，2026-11-11——phase-4 吞吐轮与质量优化轮的实测模式回灌）

两轮优化在现行代码上验证了六个真实模式，本方案据此修订（均为契约级，非重设计）：

1. **执行变体进入通用请求**（质量轮：2048 网格档 / penalty 采样链）：`TaskRequest` 必须承载与 PDF 无关的执行变体
   ——`image_tokens_override`（MTMD 网格绑定）与 `retry_sampling`（有序链规格，D-13）。二者是 runtime 层概念（网格绑定与
   sampler 链接），属于推理 API 而非扫描业务；§5.3 的请求规格据此扩展。
2. **重试策略归扫描应用，原语归推理平台**：退化/截断重试阶梯是数据表（`DegenerationRetryStep`）驱动的扫描应用策略，
   推理平台不出现"重试"概念——扫描应用以带变体的新请求重新 submit。v2 不得把阶梯下沉进推理平台。
3. **请求与页面非一一对应**：切栏档把一页分解为 N 个条带请求再由扫描应用按栏序合并。§5.3"请求不含页面上下文"
   的既有裁定升格为"请求与页面不存在数量对应关系"——推理平台不得假设一次提交等于一页。
4. **响应无栅格**（phase-4：`Option<Vec<u8>>` 编码出关键路径）：页面 PNG 编码是扫描应用的提交管线关注点；
   通用响应只携带原始文本、finish reason 与采样元数据，不携带栅格。§5.3 响应规格据此声明。
5. **解码代次为平台自有身份**（D-15）：`;decode=<gen>` 的代次值由推理平台暴露（wrapper 时代的函数），扫描应用写入
   checkpoint 兼容串时**从身份 API 取得**、不得硬编码——v2 依赖铁律下 ocrx-scan 不依赖 ocrx-runtime，代次值经
   `InferenceIdentity` 流动（§5.1 相应扩展）。
6. **协议级文本归一化单源化 + 双文本保留**（质量轮 Q3：think 剥离；评审 BLK-3 修订）：`<think>` 泄漏是 Ovis 协议
   行为，剥离属于 `ProtocolAdapter` 的响应解码职责，在响应离开 adapter 前完成（单源），保证扫描应用三类下游
   （.md/page.json/bbox）消费同一清理文本。响应必须同时携带三样：清理后文本（下游消费）、剥离与尾部裁剪之前的
   规范化原始输出（`page-NNNN.raw` sidecar 的唯一数据源，基线语义）与结构化清理计数 `think_blocks_stripped`
   （checkpoint/result 统计的数据源）——若响应只留清理文本，raw sidecar 不再是原始输出；若只留原始文本，则违反
   adapter 单源清理要求。非法图片链接降级则属 `OcrPageParser`/产物组装侧（§6.4 分界实例新增这两例）。

另：两轮实测反向验证了 §2.2 既有非目标——重试全部采用"新请求从头解码"，"从既有前缀续写"（runtime 无安全回退）
维持不做；Fresh-context per attempt 是唯一被实测验证的重试形态。

### 3.5 实验基础设施的架构影响（v6 补充，2026-11-11）

质量轮的问题内容实验（语料复扫/2048 探针/切栏/4032/金集双扫）除 §3.4 六项外，另暴露四项架构要求：

7. **checkpoint 格式获得脚本类消费者**：语料构建、逃逸评估、证伪分析脚本全部直接解析 v5 checkpoint 的
   hex 编码字段——§7 兼容策略须将"**工具可机读性**"列为格式兼容的正式要求（格式变更须评估脚本生态）。
8. **子解码的页级保守合并**：多请求分解一页（切栏）时，页级退化/截断状态 = 各子请求状态的**保守合并**
   （任一子请求环/截断 → 页未脱环），不得取最优子结果——切栏实测（学术/试卷文本反降）证明乐观合并会高估。
9. **策略旋钮位于扫描应用层**：切栏类型门控（`--column-split-retry`）证明按内容类型启停某个档位的旋钮
   属扫描应用/CLI 面，推理平台零感知。
10. **确定性是可机检契约**：213/213 与 91/91 的逐字节复扫比对把确定性从口号变为验收工具——`ocrx-eval`
    crate 应内置"同参重扫逐字节比对"作为标准评估原语，服务代次自一致与回归双场景。

---

## 4. 重构后的 Cargo crate 与代码组织

### 4.1 目标目录树

```text
src/
├── ocrx-cli/
│   └── src/
│       ├── main.rs                 # 仅 main、命令分发和组合根调用
│       ├── args.rs                 # CliCommand、ScanOptions（含多输入位置参数、--output-dir 与 --column-split-retry）、参数校验
│       ├── config.rs               # CLI > 环境 > 用户配置 > 默认值
│       ├── compose.rs              # 创建 catalog、manager、inference、scan
│       ├── interaction.rs          # 下载确认、非交互和终端策略
│       ├── present.rs              # stdout/stderr、文本/JSON、退出码
│       ├── platform_paths.rs       # 保留现有路径和 skill 支持
│       ├── pdfium_owner.rs         # 迁出：P3 PdfiumOwner 线程随渲染路径迁至 scan，CLI 不保留
│       └── system_probe.rs         # 若不再被 CLI 使用则删除，不保留空壳
├── ocrx-primitives/
│   └── src/
│       ├── lib.rs
│       ├── cancellation.rs         # CancellationToken
│       ├── progress.rs             # GenerationProgress
│       └── hash.rs                 # SHA-256 流式与内存实现
├── ocrx-core/
│   └── src/
│       ├── lib.rs                  # 仅扫描共享领域导出和通用 AppError
│       ├── page.rs                 # PageDocument/PageBlock/bbox/termination
│       ├── pdf.rs                  # 分类、路由、处理方法和 provenance
│       ├── error.rs                # AppError/ErrorKind/退出码基础定义
│       ├── resource.rs             # 扫描资源报告；不保存 runtime 兼容常量
│       └── resource_sampler.rs     # 保留现有通用资源采样实现

├── ocrx-inference-api/
│   └── src/
│       ├── lib.rs
│       ├── id.rs                   # ModelId/ProfileId/AdapterId/BackendId 等值类型
│       ├── asset.rs                # 资产角色、描述、manifest、安装 DTO
│       ├── identity.rs             # Model/Runtime/Adapter/InferenceIdentity
│       ├── task.rs                 # ImageInput/TaskRequest/TaskResponse/Usage/StepProfile；窗口批深常量
│       ├── service.rs              # InferenceService/InferenceSession（含可选 submit_window）
│       ├── management.rs           # ModelManager/DeviceManager 及 DTO；BindingVerifier 端口（RBLK-9，inference 实现、组合根注入 models）
│       ├── backend.rs              # ProtocolAdapter/RuntimeBackend/Factory 内部扩展契约（含可选 generate_window）
│       └── error.rs                # 结构化推理、资产和设备错误（含批级窗口失败类别）
├── ocrx-models/
│   └── src/
│       ├── lib.rs                  # ModelManagerImpl 与通用导出
│       ├── catalog.rs              # 静态 ModelCatalog
│       ├── manifest.rs             # v2 通用 manifest 校验
│       ├── cache.rs                # 具名资产路径、安全校验、现状检测
│       ├── install.rs              # 组事务、锁、回滚、receipt
│       ├── verify.rs               # 必需角色、大小、哈希和绑定校验
│       ├── remove.rs               # profile 删除与共享资产引用检查
│       ├── transport.rs            # Transport trait 与 HTTPS curl 实现
│       ├── recommend.rs            # 模型 profile 推荐；不再负责硬件探测
│       └── builtin/
│           ├── mod.rs
│           └── ovisocr2.rs         # 四 profile 的固定资产与来源数据
├── ocrx-runtime/
│   └── src/
│       ├── lib.rs                  # backend factory 导出
│       ├── backend.rs              # RuntimeBackend 通用实现辅助
│       ├── llama_mtmd.rs           # 只执行 EncodedTask（P4.1 actor 与多槽位窗口解码保留为内部实现），不含 OCR prompt/page
│       ├── fake.rs                 # 可编程 fake，不再用页码生成响应；含复读退化响应队列（现 LoopFakeRuntime 语义）
│       ├── step_profile.rs         # 保留现有 OCRX_STEP_PROFILE 插桩（StepProfile/delta_from 语义与导出不变）
│       ├── capability.rs           # 编译后端与实际 backend 信息
│       └── bin/mtmd-generate-probe.rs
├── ocrx-ovisocr2/
│   └── src/
│       ├── lib.rs                  # adapter ID/version 与注册描述
│       └── adapter.rs              # 图像 marker 位置、协议包装、通用响应解码
├── ocrx-inference/
│   └── src/
│       ├── lib.rs                  # 生产组合 API
│       ├── registry.rs             # 静态 model/adapter/runtime/binding 注册
│       ├── binding.rs              # 三元组兼容校验
│       ├── service.rs              # prepare/session/加载一次/执行
│       ├── device.rs               # DeviceManagerImpl、选择与诊断
│       ├── probe.rs                # 从 CLI 迁入的系统探测
│       └── telemetry.rs            # 通用遥测和可选 GPU 采样
├── ocrx-scan/
│   └── src/
│       ├── lib.rs                  # ScanApplication/ScanRequest/ScanOutcome
│       ├── plan.rs                 # 输入、页范围、初始/最终页面计划
│       ├── native.rs               # native-first、校验、fallback、empty
│       ├── ocr/
│       │   ├── mod.rs              # OCR 页面协调
│       │   ├── prompt.rs           # ocr-markdown-v1 完整 prompt
│       │   ├── parser.rs           # Ovis 文本到 PageDocument 的业务解析
│       │   ├── markdown.rs         # bbox 占位符替换与链接校验
│       │   └── artifacts.rs        # 页 PNG、bbox 裁剪、raw sidecar 和 PageArtifactResult
│       ├── window.rs               # P4.4 窗口编排：固定批深填充/排水、批级失败批深回退 1、机制 A 原子提交
│       ├── render.rs               # PdfScanRenderer/WorkerScanRenderer/PdfiumOwner 线程与预取（P3）
│       ├── resume.rs               # 恢复计划、pending OCR 恢复
│       ├── state.rs                # 页面生命周期、队列、进度、attempts 状态机与失败策略
│       ├── ladder.rs               # 组合退化阶梯（DegenerationRetryStep 数据表）、切栏分解/按栏序合并/保守合并（Q1.4a/Q1.5/D-10）
│       ├── multi_doc.rs            # 跨文档编排（X1）：文档 FIFO、共享 session、SwitchDocument、退出码聚合、scan-summary
│       ├── compatibility.rs        # 从扫描配置构造身份；匹配规则归 output
│       ├── report.rs               # result.json DTO 和路由报告
│       └── telemetry.rs            # 推理遥测到扫描报告的映射
├── ocrx-pdf/
│   └── src/
│       ├── lib.rs                  # 只导出 PDF 基础设施
│       ├── classify.rs             # 分类与路由基础算法，语义不变
│       ├── native.rs               # native 提取、校验和产物
│       ├── render.rs               # PDFium、页图、像素预算、worker
│       ├── image.rs                # bbox 裁剪/IoU/PNG
│       └── error.rs                # PDF 错误分类
├── ocrx-output/
│   └── src/
│       ├── lib.rs
│       ├── checkpoint.rs           # v6 读写、v5 安全读取、resume 判定
│       ├── commit_profile.rs       # 保留现有 CommitProfile/CommitRecorder 插桩（schema_version:1 黄金串不变）
│       ├── artifact.rs             # 原子页提交、哈希、render audit
│       ├── document.rs             # 页排序、Markdown 合并与清理
│       ├── path.rs                 # 输出路径保护
│       └── report.rs               # 通用 Reporter/ErrorReport/ResultReport
├── ocrx-eval/
│   └── src/
│       ├── lib.rs                  # 两种评估执行入口
│       ├── manifest.rs             # 质量清单、真值和输入 hash 校验
│       ├── metrics.rs              # NED/block/bbox/阅读顺序/路由指标
│       ├── gate.rs                 # native/performance/release 门禁
│       ├── report.rs               # 评估报告与完整执行身份
│       └── execute.rs              # 复用 ScanApplication 或 InferenceService
└── ocrx-skill/                     # 本次不改变业务
```

文件拆分可以按实际行数小幅合并，但 crate 边界、模块职责和禁止依赖不得后退。尤其不得为了少建文件而把 `ocrx-scan` 再并回 CLI，或把 OCR parser 留在推理 adapter 中。

### 4.2 每个 crate 的所有权

| Crate | 拥有的业务 | 明确不拥有 |
|---|---|---|
| `ocrx-primitives` | 取消、通用进度、哈希 | 任何 OCR/PDF/模型概念 |
| `ocrx-core` | 页面和 PDF 领域对象、应用错误 | 模型 ID、runtime 版本、设备选择、推理请求 |
| `ocrx-inference-api` | 推理平台公共 DTO、接口和身份 | 资产下载实现、llama.cpp、扫描 prompt、页码 |
| `ocrx-models` | manifest、目录、缓存、安装、校验、删除 | 设备硬件探测、PDF、页面输出 |
| `ocrx-runtime` | backend 执行（含窗口合批内部实现）、可选 StepProfile 插桩和 fake | 模型安装、业务 prompt、业务文本解析 |
| `ocrx-ovisocr2` | Ovis 模型协议适配 | OCR prompt 内容、PageDocument、bbox 裁剪、Markdown 链接 |
| `ocrx-inference` | 静态绑定、设备管理、session 组合和遥测 | 扫描编排、模型评分 |
| `ocrx-scan` | 完整扫描应用、窗口编排、跨文档编排（X1）、OCR prompt 和业务解析 | 具体资产路径、llama.cpp 类型、模型质量评分 |
| `ocrx-pdf` | PDF 事实获取和图像处理 | 质量真值、checkpoint、推理 |
| `ocrx-output` | 持久化、checkpoint、通用输出与可选 CommitProfile 插桩 | 页面计划、识别、模型加载 |
| `ocrx-eval` | 样本、真值、评分、评估执行与报告 | 扫描内部实现、runtime 内部实现 |

---

## 5. 关键公共契约设计

以下类型定义规定字段和语义，实施时可以使用等价的 Rust 所有权形式，但不得重新加入页码、PDF 路径或扫描输出目录。

### 5.1 标识与结构化执行身份

模型 ID 不再是只允许一个枚举值的 `ocrx-core::ModelId`。改成经过 catalog 解析的值类型；当前内置 catalog 仍只注册 `ovisocr2`，因此外部行为仍是单模型。

```rust
pub struct ModelId(String);
pub struct ProfileId(String);
pub struct AdapterId(String);
pub struct RuntimeId(String);
pub struct BackendId(String);
pub struct AssetRole(String);

pub struct AssetIdentity {
    pub role: AssetRole,
    pub sha256: String,
    pub byte_len: u64,
}

pub struct ModelIdentity {
    pub model: ModelId,
    pub profile: ProfileId,
    pub source_revision: String,
    pub assets: Vec<AssetIdentity>, // 按 role 排序，禁止重复 role
}

pub struct RuntimeIdentity {
    pub runtime: RuntimeId,          // 新定义的内部 ID，固定为 "llama.cpp"
    pub wrapper_version: String,     // 固定为 "0.1.161"（Q1.3 pins 修订）
    pub sys_version: String,         // 固定为 "0.1.161"（wrapper 硬依赖同版本）
    pub upstream_revision: String,   // 固定为 "d81235049384534c167caea52b85a694f6103d14"
                                     // （sys 0.1.161 内嵌 llama.cpp，2026-10-05；旧值 9e3b928 属 0.1.153 代次，已退役）
}

pub struct AdapterIdentity {
    pub adapter: AdapterId,          // 当前为 "ovisocr2"
    pub version: String,             // 当前为 "0.1.0"
}

/// 已解析 profile 的展示元数据（RBLK-5）：result.json 的 name/family/
/// quantization 唯一来源。字段值来自 prepare 时**实际解析的那份 manifest**，
/// 经 `InferenceIdentity` 随 session 到达扫描应用——scan 不依赖 ocrx-models、
/// 不复制展示映射（架构铁律 + §7.4 唯一派生规则）。**报告专用字段（RBLK-7
/// 裁定）：不参与任何兼容匹配**——checkpoint 存储完整身份时原样保存（审计），
/// 但预比较与终比较都排除它；展示文案或量化展示名变化不触发重处理，只有
/// 执行字段变化才触发（区分性测试见 T055/T100）。
pub struct ModelDisplayMeta {
    pub display_name: String,        // 当前为 "OvisOCR2"
    pub family: String,              // 当前为 "Ovis"
    pub quantization: String,        // compact=Q4_K_M，balanced=Q5_K_M，quality=Q8_0，reference=BF16
}

pub struct InferenceIdentity {
    pub model: ModelIdentity,
    pub runtime: RuntimeIdentity,
    pub adapter: AdapterIdentity,
    pub backend: BackendId,           // 实际执行的 backend，而非仅请求值
    pub decode_generation: u32,       // 解码代次（D-15）：当前冻结为 2；平台产生，扫描只透传
    pub batch_depth: usize,           // 实际生效批深：8/16 分层（12 GiB 阈值），GPU 回退 CPU 后重选为 8
    pub display: ModelDisplayMeta,    // 展示元数据（RBLK-5）：与执行身份同源同返回，杜绝二次解析漂移
}
```

`InferenceIdentity` 由推理平台在 prepare 成功后返回。扫描应用不得手工伪造 runtime、adapter 或资产哈希；`decode_generation` 与 `batch_depth` 由平台产生（代次是 wrapper 时代的函数，批深由设备分层与回退重选决定），扫描应用写入 checkpoint 兼容身份时**从身份 API 读取**，不得硬编码（§3.4 第 5 条落实）；`display` 展示元数据由平台在 prepare 时从**实际解析的那份 manifest** 填充（RBLK-5：result 展示字段的唯一合法来源，scan 不依赖 ocrx-models、不硬编码展示映射，纯复用零加载扫描无 session、result 模型对象本就为 null，不需要该通道；RBLK-7 裁定：**display 属报告专用字段，不参与兼容匹配**——静态身份不含它，匹配规则显式排除它）；扫描应用只追加自己的 prompt、渲染和 PDF 路由身份。除实际 backend、实际批深与 `display`（报告专用）外的执行身份字段构成静态身份子集（含 auto 解析后的 profile、逐资产 manifest 元数据、runtime/adapter 绑定值与代次），另有 prepare 前的只读查询通道 `InferenceService::static_identity()`（§5.5，RBLK-4：不校验资产文件、不安装、不加载 runtime）；实际 backend 与批深只在 prepare 后存在（RBLK-2），恢复流程据此分两阶段比较（§6.3 第三步/第五步）。

### 5.2 通用资产 manifest v2

```rust
pub enum AssetScope { Profile, Revision }

pub struct AssetDescriptor {
    pub role: AssetRole,             // Ovis 使用 "model" 与 "mmproj"
    pub required: bool,
    pub scope: AssetScope,
    pub relative_path: String,       // 只能是安全的相对路径
    pub sha256: String,
    pub byte_len: u64,
    pub source_url: String,          // 当前只允许固定 HTTPS URL
}

pub struct ModelManifest {
    pub schema_version: u32,         // 当前为 2
    pub model: ModelId,
    pub profile: ProfileId,
    pub display_name: String,        // 当前为 "OvisOCR2"
    pub family: String,              // 当前为 "Ovis"
    pub quantization: String,        // compact=Q4_K_M，balanced=Q5_K_M，quality=Q8_0，reference=BF16
    pub source_revision: String,
    pub assets: Vec<AssetDescriptor>,
    pub runtime_binding: BindingRequirement,
    pub adapter_binding: AdapterRequirement,
    pub license_id: String,
    pub provenance: AssetProvenance,
}
```

规则如下：

- 角色名在同一 manifest 内唯一；必需角色缺一不可，不能假设一定恰好是 model + mmproj。
- Ovis 的 `model` 是 `Profile` 作用域，`mmproj` 是 `Revision` 作用域，所以四 profile 仍共享同一 mmproj。
- `relative_path` 与现有缓存路径一致，拒绝绝对路径、`..`、路径分隔逃逸和空段。
- chat template、OCR prompt、页码和渲染 DPI 不属于资产描述。tokenizer 若未来确为独立必需文件，才以增加具名资产角色的方式表达。
- 当前 manifest 由编译期内置数据构造，不开放任意目录扫描和动态注册。通用结构是为后续显式新增内置模型服务，不等于插件系统。

### 5.3 通用推理任务与响应

```rust
pub struct ImageInput {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub rgb8: Vec<u8>,
}

pub struct GenerationParams {
    pub max_output_tokens: u32,
    pub temperature_milli: u32,
    pub thinking: bool,
}

/// 有序采样链规格（质量轮 Q1.2/D-13）：链中 penalty 先改 logits、greedy 终选；
/// 链是确定性的（无随机源），同参重扫逐字节可复现。普通页不携带（纯贪心）。
pub struct RetrySampling {
    pub repeat_penalty_milli: u32,   // 1000..=2000（1.0x..=2.0x），milli 保持 Eq 可断言
    pub penalty_window: u32,         // >= 1
}

pub struct TaskRequest {
    pub prompt: String,
    pub images: Vec<ImageInput>,
    pub generation: GenerationParams,
    /// 执行变体（§3.4 第 1 条，基线 `9e0dc29` 请求形态）：与 PDF 无关的 runtime 层
    /// 执行语义。`None` = 扫描级贪心预算。网格覆盖（Q1.4c）把该请求的解码绑定到
    /// 指定 MTMD token 网格（如 2048）；采样链（Q1.2）按 RetrySampling 执行。
    /// 扫描应用按阶梯 rung 填充（§2.3 冻结表），推理平台不解释其业务含义。
    pub image_tokens_override: Option<u32>,
    pub retry_sampling: Option<RetrySampling>,
}

pub enum FinishReason { Eos, StopToken, MaxTokens, Invalid, RepetitionLoop }

pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: u64,
}

/// 实际生效解码变体回执（§3.4 第 4 条采样元数据）：由 backend 按实际执行值
/// 填充（§5.4 `BackendOutput.decode_echo`，adapter 原样透传），不由扫描应用
/// 自行假设，也不从请求值拷贝；`None` 字段 = 对应覆盖未生效。
pub struct DecodeEcho {
    pub image_tokens: Option<u32>,
    pub sampling: Option<RetrySampling>,
}

pub struct TaskResponse {
    /// 供下游消费的规范化文本：`<think>` 已剥离（adapter 解码职责，Q3.1 单源）。
    pub text: String,
    /// 剥离与尾部裁剪之前的规范化原始输出（换行规范化后、无清理）：`page-NNNN.raw`
    /// sidecar 的唯一数据源（基线语义，评审 BLK-3）。
    pub raw_text: String,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
    /// adapter 解码期清理计数：完整 think 块（含静默空剥离）+ 散落标签；聚合进
    /// v6 checkpoint 字段与 result `pages.think_blocks_stripped_total`。
    pub think_blocks_stripped: u32,
    /// 透传自 `BackendOutput.decode_echo` 的实际生效回执（RBLK-1 修订：
    /// 数据通道为 backend → BackendOutput → adapter decode → response）。
    pub decode_echo: DecodeEcho,
    pub warnings: Vec<String>,
}

pub trait InferenceSession {
    fn identity(&self) -> &InferenceIdentity;
    fn telemetry(&self) -> RuntimeTelemetry;
    /// 可选诊断快照（T1，`OCRX_STEP_PROFILE` 同开关）：推理各阶段的累计结构化
    /// 计数，字段与基线 `step-profile.json` 黄金串一致；未开启返回 `None`。
    /// `StepProfile` 类型与 `delta_from` 语义定义在公共 API（与 `RuntimeTelemetry`
    /// 同类的诊断 DTO，评审 RBLK-3 修订）：scan 经本方法取得累计快照并做逐文档
    /// 差分（§8.10），禁止引用具体 runtime 类型（架构检查项，T089）。
    fn step_profile(&self) -> Option<StepProfile>;
    fn submit(
        &mut self,
        request: TaskRequest,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<TaskResponse, InferenceError>;

    /// 可选窗口入口（P4.2 §⑤ 最小接口面）：一次提交一批请求。
    /// 默认实现按序逐个 `submit`，与单流解码数值等价；真实 backend
    /// 覆盖为多槽位连续合批。进度回调携带窗口内槽位序号（不是页码），
    /// 扫描应用自行映射页码。结果按请求顺序排列；批级失败（解码/
    /// OOM/上下文溢出）以 `InferenceError::WindowFailed` 填充全部条目，
    /// 由扫描应用整窗批深回退 1 重提。
    fn submit_window(
        &mut self,
        requests: Vec<TaskRequest>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<TaskResponse, InferenceError>> {
        let mut results = Vec::with_capacity(requests.len());
        for (slot, request) in requests.into_iter().enumerate() {
            results.push(self.submit(request, cancellation, &mut |event| {
                progress(slot, event)
            }));
        }
        results
    }
}
```

约束：

- `TaskRequest` 没有 `page`、`output_root`、`PdfMode`、`PageClassification`、checkpoint 或 Markdown 字段；请求与页面不存在数量对应关系（切栏档一页可产生多个子请求，§3.4 第 3 条）。
- 执行变体校验在 API 构造器完成：`retry_sampling.repeat_penalty_milli` 限 `1000..=2000`、`penalty_window >= 1`、`image_tokens_override` 必须是受支持的 MTMD 网格绑定值（冻结阶梯仅使用 2048；`None` = 扫描级图像 token 预算 1024）；网格覆盖只影响该次解码的上下文绑定，不改变扫描级兼容参数（基线 Q1.4c 语义）。
- 响应双文本分工：扫描应用仅以 `text` 驱动 `.md`/page.json/bbox 解析；`raw_text` 只写入 `page-NNNN.raw` sidecar 并与 `think_blocks_stripped` 一同进入 checkpoint 与 result 聚合。`raw_text` 必须等于 backend 原始输出经换行规范化后的结果——adapter 不得在剥离前修改其内容；两个文本字段的来源与不变式均有合约测试。
- 当前扫描每页只传一张图，窗口是多个单图请求的批量提交；图像尺寸和 RGB8 长度在 API 边界校验。
- 取消不是正常 `FinishReason`。backend 遇到取消统一返回 `InferenceError::Cancelled`，扫描应用据此停止取新页；被取消的在飞窗口零提交（机制 A）。
- `finish_reason = max_tokens` 只表示生成预算耗尽；扫描应用把它映射为页 `Termination::MaxTokens` 和 `truncated` warning。`repetition_loop` 由扫描应用进入复读退化处理（组合阶梯重试、`degenerated` 标记），不视为普通质量失败；纯截断页（`max_tokens`）同样按 §2.3 冻结阶梯进入重试（Q1.5/D-14）。
- `FinishReason` 枚举与现有 `ocrx-core::Termination` 全集一致（含 `Invalid`、`RepetitionLoop`），不得在迁移中收窄。
- 窗口确定性契约沿用 P4 冻结语义：相同（输入、批组合、批深）下输出在同一解码代次内逐字节可复现，不同批组合允许分叉；扫描应用的窗口填充策略（固定批深、按页序、不动态回填）保证断点前后批组合一致，实际批深经 `InferenceIdentity.batch_depth` 进入 checkpoint 兼容身份。
- telemetry 仍是 session 累计值（窗口内全序列聚合累加），扫描报告每页结束后读取当前累计值，不能重复累加；多文档编排下每文档的 telemetry/StepProfile 报告取“当前累计 − 该文档起点基线”的增量（X1 既有语义，§8.10）。

### 5.4 adapter 与 backend 的内部扩展契约

```rust
pub enum ImagePlacement { BeforePrompt }

pub struct EncodedTask {
    pub prompt: String,
    pub images: Vec<ImageInput>,
    pub image_placement: ImagePlacement,
    pub generation: GenerationParams,
    // 执行变体随请求透传（§5.3）：backend 据此执行网格绑定与采样链。
    pub image_tokens_override: Option<u32>,
    pub retry_sampling: Option<RetrySampling>,
}

pub struct BackendOutput {
    pub text: String,   // 模型原始输出（未做任何协议清理）
    pub native_finish_reason: NativeFinishReason,
    pub usage: TokenUsage,
    /// 实际生效解码变体回执（RBLK-1 修订）：由 backend 按实际执行值填充——
    /// 默认串行实现等于请求变体原样，llama backend 填实际网格绑定与采样链
    /// （存在回退/降级时与请求值不同）。adapter 在 decode 中原样透传，
    /// 不得从请求拷贝或填空。
    pub decode_echo: DecodeEcho,
}

pub trait ProtocolAdapter {
    fn identity(&self) -> &AdapterIdentity;
    fn encode(&self, request: TaskRequest) -> Result<EncodedTask, InferenceError>;
    // 解码职责（§3.4 第 6 条）：换行规范化原始输出 → 剥离 <think> 并计数 →
    // 产出 TaskResponse（text/raw_text/think_blocks_stripped）；decode_echo
    // 自 BackendOutput 原样透传（RBLK-1 数据通道：backend → BackendOutput
    // → adapter decode → TaskResponse）。
    fn decode(&self, output: BackendOutput) -> Result<TaskResponse, InferenceError>;
}

pub trait RuntimeBackend {
    fn generate(
        &mut self,
        task: EncodedTask,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<BackendOutput, InferenceError>;

    /// 可选窗口方法（P4.2 §⑤ 最小接口面，映射现有 `Runtime::generate_window`）：
    /// 默认实现逐任务串行 `generate`；llama.cpp backend 覆盖为推理线程
    /// actor 的多槽位连续合批（P4.1/P4.3 内部实现，按槽位转发进度）。
    /// 批级失败返回 `InferenceError::WindowFailed`，session 层将其填充到
    /// 窗口全部条目；单页失败只占其槽位条目。
    fn generate_window(
        &mut self,
        tasks: Vec<EncodedTask>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<BackendOutput, InferenceError>> {
        let mut results = Vec::with_capacity(tasks.len());
        for (slot, task) in tasks.into_iter().enumerate() {
            results.push(self.generate(task, cancellation, &mut |event| {
                progress(slot, event)
            }));
        }
        results
    }
}
```

Ovis adapter 只声明“图像位于 prompt 前，使用 runtime 默认多模态图像 marker，保持生成参数，解码通用结束原因，剥离 `<think>` 并保留原始文本与清理计数（§3.4 第 6 条），透传 `BackendOutput.decode_echo`（RBLK-1）”。llama.cpp backend 在图像位置调用 `mtmd_default_marker()`，拼接 marker 与 adapter 已给出的 prompt；对 `image_tokens_override` 请求以覆盖值绑定该次解码的 MTMD token 网格，对 `retry_sampling` 请求按“penalty 先改 logits、greedy 终选”的链序挂采样器，并把实际生效变体填入 `BackendOutput.decode_echo`。这样既保留当前精确输入顺序，又不让 adapter 依赖 llama.cpp 类型。P4 已落地的推理线程 actor、多序列槽位、逐序列环检测与窗口上下文容量计算保留为 backend 内部实现，不暴露给 adapter 或扫描应用。

Ovis 的公式、HTML 表格、bbox、重复尾部、Markdown 图片链接等解释全部移到 `ocrx-scan/src/ocr/`。这些不是模型传输协议，而是 OCR 扫描产物规则。

### 5.5 推理服务与准备流程

```rust
pub struct PrepareRequest {
    pub model: ModelId,
    pub requested_profile: String,   // auto 或具体 profile
    pub requested_device: String,    // auto 或具体 backend
    pub max_image_tokens: u32,
    pub context_size: u32,
    pub install: InstallPolicy,
}

pub struct PreparedInference {
    pub session: Box<dyn InferenceSession>,
    pub requested_backend: BackendId,
    pub warnings: Vec<String>,       // 包括 GPU 初始化失败后回退 CPU
}

/// prepare 前可得的静态身份子集（RBLK-4，恢复预比较的数据源）：完整执行
/// 身份中除实际 backend、实际批深与 `display`（报告专用，RBLK-7 裁定不参与
/// 匹配，故不入静态身份）外的全部**执行**字段。`auto` 在此按 §2.3 冻结的
/// 内存推荐规则解析为具体 profile——与 prepare 使用同一推荐实现（同输入
/// 同结果），但内存输入可能变化，两次解析**不保证相同**；该漂移不由本查询
/// 消除，由 §6.3 第五步的全执行字段终比较闭环（RBLK-6）。
pub struct StaticInferenceIdentity {
    pub model: ModelIdentity,        // 含解析后 profile、source_revision、逐资产 role/sha256/byte_len（内置 manifest 元数据）
    pub runtime: RuntimeIdentity,    // runtime id + wrapper/sys/upstream（静态 registry）
    pub adapter: AdapterIdentity,    // adapter id + version（静态 registry）
    pub decode_generation: u32,      // D-15 代次（编译期常量）
}

pub trait InferenceService {
    fn prepare(
        &mut self,
        request: PrepareRequest,
        interaction: &mut dyn InstallInteraction,
    ) -> Result<PreparedInference, InferenceError>;

    /// 解码代次静态查询（D-15，RBLK-2 修订）：wrapper 时代的编译期常量，
    /// prepare 前可调用、不触发资产校验或模型加载；`static_identity()` 的
    /// 单字段便捷形式，两者同源。实际 backend/批深无此通道——它们只在
    /// prepare 后确定（设备分层与回退重选依赖运行时探测与初始化）。
    fn decode_generation(&self) -> u32;

    /// 只读静态身份查询（RBLK-4，恢复预比较的完整入口）：解析 catalog
    /// （含 `auto` → 具体档位）、读取内置 manifest 的逐资产元数据与静态
    /// registry 的 runtime/adapter 绑定值。明确约束：不校验资产文件哈希、
    /// 不安装、不创建 runtime、不做设备初始化；资源探测仅用于 auto 档位的
    /// 内存推荐。返回值不含实际 backend/批深——它们是终比较专用字段，
    /// 仅在 prepare 成功后存在（§6.3 两阶段协议）。
    fn static_identity(
        &self,
        model: &ModelId,
        requested_profile: &str,
    ) -> Result<StaticInferenceIdentity, InferenceError>;
}
```

`prepare` 的顺序固定为：解析 catalog → 选择 profile → 校验绑定 → 选择设备 → 检查资产 → 按需安装并复检 → 创建 runtime → 必要时 GPU 回退 CPU → 创建 adapter/session → 返回实际身份。任何一步失败都不能留下“看似可用”的 session。

批深不在 `PrepareRequest` 中指定（无用户旋钮，§2.2）：平台在选择设备时按 VRAM 分层决定（≥ 12 GiB → 16，否则 8；CPU → 8），GPU 初始化失败回退 CPU 后**重选为 8**；实际生效批深写入返回身份的 `batch_depth`，解码代次写入 `decode_generation`（当前冻结 2）。扫描应用组装窗口与 checkpoint 兼容身份只读取这两个身份字段，不自行从设备信息推导（评审 BLK-4：实际批深必须跨层传递，避免以“GPU 档位猜测”替代实际值——基线以 `SharedRuntime.window_depth` 承载同一语义）。

`InstallInteraction` 是推理 API 的通用交互端口，只表达“需要下载多少字节、来源和许可证是什么、是否允许继续”。CLI 实现终端询问；`--non-interactive` 和离线策略由请求明确控制。扫描应用和模型 install 命令复用同一准备/安装实现，不再各自拼 URL 和 hash。

### 5.6 模型与设备管理 API

`ModelManager` 至少提供：

- `list()`：列出 catalog 中模型，不触碰 PDF。
- `info(model)`：列出所有 profile、安装状态和来源 revision。
- `recommend(model, resources)`：返回 profile 推荐，不隐式安装。
- `install_plan(model, profile)`：返回资产、字节数、许可证和当前状态，供 CLI 确认。
- `install(model, profile, policy, interaction)`：安装、校验、写 receipt；重复安装已验证资产时无网络且成功。
- `verify(model, profile)`：检查所有必需角色、大小、SHA-256 和静态 runtime/adapter 绑定。绑定校验经**注入的 `BindingVerifier` 端口**执行（RBLK-9）：端口 trait 定义在 inference-api（与 manifest DTO 同 crate，models → inference-api 既有方向，无环），由 `ocrx-inference` 依据静态 registry 提供实现，组合根在构造 `ModelManagerImpl` 时注入（§6.1 组合顺序相应调整）；models 层不读取 inference 内部 registry，仅调用端口。端口为纯数据比较，不创建 runtime、不加载模型，`verify` 的“不下载、不加载”语义不变。测试注入 fake 端口，生产由组合根接线。
- `remove(model, profile)`：只删除目标 profile 的私有资产；共享资产只有在同 revision 不再被其他 profile 使用时才删除。

`DeviceManager` 至少提供：

- `snapshot()`：返回内存、硬件可用性和编译能力。
- `select(requested)`：显式设备不可用时报错，不静默改选；auto 按冻结优先级。
- `window_depth(vram)`：批深分层选择（VRAM ≥ 12 GiB → 16，否则 8；CPU/探测不可用 → 8），供 `prepare` 调用并在 GPU→CPU 回退后重选；分层函数与阈值随批深契约迁移（现 `ocrx-core::window_depth_for_vram`），不向用户暴露批深旋钮。
- `diagnose()`：返回结构化设备、推荐 profile、缓存和安装概况，供 CLI doctor 展示。
- `compiled_backends()`：CPU 始终可用，Metal/CUDA/Vulkan 按 target/feature 判定，硬件存在与编译能力必须取交集。

### 5.7 错误边界

推理错误至少区分：

```text
UnsupportedModel / UnsupportedProfile / InvalidManifest /
MissingAsset / HashMismatch / IncompatibleBinding /
Offline / LicenseRequired / InsufficientDisk / Transport /
DeviceUnavailable / RuntimeInitialization / InvalidTask /
BackendExecution / WindowFailed / AdapterDecode / Cancelled
```

扫描错误至少区分 PDF 输入、PDF 路由、native 提取、渲染、模型准备、推理、响应解析、产物完整性、输出和取消。CLI 统一把结构化错误映射为现有退出码：用法 2、输入 3、模型 4、下载 5、runtime 6、解析 7、完整性 8、输出 9、部分完成 10、取消 130。现有个别命令分支的历史退出码如有差异，以既有 CLI golden 为准保留，不能在重构中顺便“修正”为另一套行为。多文档编排的退出码聚合保持现有 severity 阶梯：130 > 文档级失败 3 > 部分 10 > 0（`scan-summary.json` 记录每文档 status 与 exit_code，X1.4 冻结语义）。

错误消息可以补充结构化 cause，但 stdout/stderr 通道、JSON 外层 schema 和敏感信息脱敏规则不能改变。密码、prompt 内容、token、授权信息和代理凭据不写入报告。

---

### 5.8 多模型接入缺口评估（v7 新增，2026-11-11；v7.2 范围裁定——本节整体为评估记录，不在 v2 范围）

**范围裁定（v7.2，评审 BLK-5 修订，取代 v7 的“v2 范围内须解决”标记）：** 本节只是以候选模型审架构得出的缺口评估记录，全部条目**均不在本次重构（v2）范围**：实施范围（§头部）仍是“仅重构现有 OvisOCR2 路径，不接入第二模型”，§2.2 非目标不变。v2 的唯一冻结契约是 §5.1–§5.5 的请求/响应/身份形态——执行变体 `image_tokens_override`/`retry_sampling` 直载于 `TaskRequest`/`EncodedTask`，与声明基线 `9e0dc29` 的请求形态一致。下列“能力引用/规范中间层/per-adapter 身份”等演化方向仅在真正立项接入第二模型时作为该立项的设计输入重新裁决；本计划的任务图（§12）与完成判定（§11）不含本节任何条目，不得据本节改写 §5.1–§5.5 或 T007 系列（P0-2/P0-3 与 §3.4 第 1 条执行变体直载契约的分歧，以 §3.4/§5.3 为准）。

调研事实（2026-11-11）：Xiaomi-OCR-0（0.8B，Qwen3.5-0.8B 底座——与 OvisOCR2 同底座，KIE/VQA 能力，HF/GitHub 开源）；MiniCPM-OCR 1.0（1.3B，MiniCPM-V 架构，官方量化为 **AWQ W4A16**，另有 qwen3_5 底座 Jev 变体，印章/表格/公式/KIE）；OvisOCR3 与 OXR 已调研（2026-11-11 补）：**OvisOCR3**（ATH-MaaS，2026-10-20 发布）：0.8B，Qwen3.5-0.8B 底座（与 OvisOCR2 同底座同量化家族），统一 checkpoint 覆盖文档解析/OCR/KIE/图表描述，OmniDocBench **96.74**，社区 GGUF 已有（prithivMLmods/ATH-MaaS-OvisOCR3-GGUF），技术报告 arXiv 2610.33066——接入路径与我们管线完全同构（MTMD + GGUF + mmproj），属**升级候选**而非新家族；**OXR**（SoMarkAI，"Optical Everything Recognition"）：1.3B，输出 Agent-Friendly Markdown/JSON（含表格/公式/版面坐标），自称 olmOCR-Bench/OmniDocBench/DocParsingBench 三榜第一；需单独核实其 llama.cpp 可运行性与输出 schema（无 mmproj 信息）。对缺口清单的修正：P0-1（量化格式）对 OvisOCR3 直接消解（GGUF 已有）；P0-2/P0-3（规范中间层/网格能力）对两者仍然成立（各模型输出 schema 与网格语义不同）；OXR 的版面坐标输出与我们 bbox 语义相近但格式待核实。

以“接入这批模型”审 v6 架构，缺口按优先级：

**P0（阻断接入第二模型；v2 范围外，留作后续立项前置条件记录）**

1. **量化格式单一**：runtime 仅消费 GGUF+mmproj；MiniCPM-OCR 官方量化为 AWQ W4A16。决策点：
   manifest 强制 GGUF（要求上游/社区转换）或引入非 llama.cpp runtime。v2 **维持既有 not-now 裁决、
   不重新裁决**——接入 AWQ 模型属独立立项，届时必须先裁决转换路径 vs 第二 runtime，再动 manifest/绑定。
2. **规范中间层未形式化**：现行"通用响应"实为 Ovis 形状 markdown 的隐式契约（bbox_{l,t,r,b} [0,1000) 缩放、
   `<think>` 泄漏、HTML 表格约定）。每个新模型的协议约定不同——接入新模型时须先定义形式化规范中间层：
   `ProtocolAdapter` 输出**规范几何（归一化坐标）+ 规范块类型**的中间表示，`OcrPageParser` 消费规范表示而非
   Ovis 形状文本；否则每个新模型都会 fork parser。
3. **token 网格契约 Ovis 专属**：`image_tokens_override` 假设 min==max 的 MTMD 网格绑定；MiniCPM-V 为
   per-image 切片预算机制。§3.4 第 1 条的执行变体直载契约仅服务 MTMD 族模型；接入非 MTMD 网格语义的模型时
   须改写为 **adapter 声明的网格能力**（声明支持的预算语义枚举），请求引用能力而非硬编码语义。

**P1（接入会痛；v2 范围外，随接入立项解决）**

4. **prompt schema 身份全局化**：现行 `prompt_schema_version=1` 为全局值——接入自有 prompt/版本的模型时须改为
   per-adapter 身份（每模型自有 prompt + 版本号），执行身份（§5.1）与 checkpoint 兼容串随动；v2 维持全局值 1。
5. **采样链跨 runtime 映射**：`RetrySampling` 规格基于 llama.cpp sampler API；跨 runtime 需映射层
   （单 llama.cpp 期内无成本，新增 runtime 时生效）。
6. **规范块类型可扩展**：MiniCPM 的印章识别是新块类型——接入时规范块 schema 须允许 adapter 注册新块类型或
   声明降级为 markdown 的规则，避免 parser 逐模型枚举。
7. **评估导出的模型归一化**：评测导出（think 剥离/链接处理）是 Ovis 特有补救——接入时各 adapter 须自带
   导出归一化钩子，评测管线不复制模型专属逻辑。

**P2（v6 架构已覆盖，列出为验证点）**

8. 多模型 manifest 注册（§5.2 设计本为此设）；9. 非合批 runtime 走 `submit_window` 默认逐个串行实现；
10. 阶梯策略按模型数据驱动（无 `<think>` 泄漏的模型无需相应档位）；11. 静态注册扩至多 adapter。

**范围裁定补充**：KIE/VQA 是结构化抽取任务，不是"页→Markdown"——推理平台的通用 TaskRequest 可承载，
但扫描应用无此管线；属独立立项，v2 仅须确认平台不阻断（本节已核）。调研状态：OvisOCR3 已核（同构升级候选，
GGUF 已有）；OXR 剩余核实项为 llama.cpp 可运行性与输出 schema（含 mmproj 缺位的确认），核实完成后补记
本节，不阻塞 v2 实施。

## 6. 重构后的业务逻辑

### 6.1 应用启动与组合

CLI 启动后只创建一个生产组合上下文：

1. 创建内置 Ovis manifest 集合和 `ModelCatalog`。
2. 创建系统 probe、`DeviceManagerImpl` 和 llama.cpp runtime factory。
3. 注册 Ovis adapter 以及 `ovisocr2 + llama.cpp + 固定版本` 的兼容 binding（静态 registry，纯数据、无加载）；其中 `llama.cpp` 是本次新定义的内部 RuntimeId，不是对现有 result.json 展示字符串的照抄。
4. 创建 `BindingVerifier` 实现（依据静态 registry，RBLK-9），再创建 HTTPS transport、资产存储和 `ModelManagerImpl`（构造时注入 verifier——registry 先于 manager 存在，models 层经 inference-api 端口取得绑定校验，无反向依赖）。
5. 创建 `InferenceServiceImpl`。
6. 用 `dyn InferenceService`、PDF 基础设施和输出存储创建 `ScanApplication`。
7. `scan` 调用扫描应用（单/多输入统一入口；跨文档编排属扫描应用职责，CLI 不再自行编排文档）；`model` 调用模型管理（`verify` 经注入端口完成资产 + 绑定校验）；`doctor` 调用设备和模型管理；`skill` 继续走原模块。

未知模型仍在参数/配置解析阶段被拒绝；当前 catalog 只有 Ovis，因此不会意外接受第二模型。

### 6.2 模型管理和 doctor 流程

```text
CLI model list/info/recommend/install/verify/remove
  → 解析命令和 policy
  → 调用 ModelManager
  → 如需交互，由 CLI 实现 InstallInteraction
  → 格式化管理 DTO
  → 输出并映射退出码
```

整个流程没有 PDF renderer、`ScanRequest` 或 `InferenceSession`。`install` 的交互确认和实际安装分离；`verify` 不下载；`remove` 不依赖 runtime；`list/info` 不强制进行大文件哈希，只有 `verify` 做完整内容校验。

```text
CLI doctor
  → DeviceManager 获取编译/硬件/内存/推荐设备
  → ModelManager 获取缓存与 profile 安装概况
  → CLI 合并原有 skill_installed 信息
  → 保持 doctor 文本和 JSON 字段兼容
```

### 6.3 扫描流程总览

扫描仍采用“先分析，再 native/empty，再 OCR，最后汇总”的两阶段流程，不改成逐页无计划地调用模型。

#### 第一步：输入和输出预检

1. 安装 Ctrl+C handler，第一次请求协作取消，第二次强制退出 130。
2. 解析输入、输出目录、页范围、PDF 模式、质量级别、模型/profile/device、渲染和生成参数。
3. 用 PDFium 读取页数；不存在、非 PDF、损坏、加密和动态库错误仍在模型处理前报告。
4. 校验页范围、`--resume` 与 `--overwrite` 互斥、数值范围、输出路径与输入/模型缓存/工作目录不重叠。
5. 显式请求不可用设备时仍按当前行为报错，即使最终可能是 native-only；auto 不在此处提前加载 runtime。

#### 第二步：建立页面计划

1. 对所选页提取页面信号并分类。
2. `pdf-mode=text` 时，非 empty 页必须具备可靠 native 路径，否则终止。
3. `pdf-mode=ocr` 时，所有非 empty 页进入初始 OCR 队列；empty 页仍走 empty。
4. `pdf-mode=auto` 时按分类形成 native candidate、初始 OCR 和 empty 计划。
5. 队列按页码排序、去重，保留 native fallback 原因。

#### 第三步：恢复已有状态

1. 计算输入 SHA-256 和 PDF/渲染公共身份。
2. 读取 v6 checkpoint；对 v5 使用迁移规则，不把缺少新身份的 OCR 结果误判为兼容。
3. OCR 复用判定分两阶段（RBLK-2 修订，解除“prepare 前比较实际身份”与“完全复用不加载”的时序冲突；RBLK-6 修订，终比较为全字段权威；RBLK-8 修订，终比较仅限有待解码页的文档）：**预比较**是零加载快速路径，在模型准备前执行，比较 checkpoint 存储身份中除实际 backend 与实际批深外的全部 OCR 执行字段（`display` 报告专用字段不参与，RBLK-7）——当前静态身份经 `InferenceService::static_identity()` 只读查询取得（RBLK-4：catalog 解析含 `auto` → 具体档位、内置 manifest 逐资产元数据、静态 registry 的 runtime/adapter 绑定值与解码代次；不校验资产文件、不安装、不加载 runtime），扫描侧字段（prompt/parser/渲染/生成参数）由扫描应用自持；预比较结果只决定“是否可以省去 prepare”，不是复用的最终裁决——**终比较**（第五步）才是权威，凡本文档存在待解码页，候选页一律以即将执行解码的 session 实际身份做全执行字段复核。全部候选页通过预比较且无任何待解码页时，整文档复用，不 prepare、不加载模型——本次零解码，不产生混批深/混代次/混 profile 文档，checkpoint 存储身份即终局，无需当前实际身份；**多文档编排下该规则逐文档独立适用：完全复用的后续文档不因前文档已创建共享 session 而改变其纯复用终局（RBLK-8）**。
4. 已提交且预比较通过、产物哈希有效的页进入候选复用集合，不重复渲染或识别；终比较（全执行字段）不通过的候选页由第五步移入重处理——含 auto 在预比较后、prepare 时改选 profile 的情形（RBLK-6），不一致候选全部重处理，绝不跨 profile 混用。
5. 未完成且属于 pending OCR 的页回到 OCR 队列；其他不匹配或损坏状态进入重处理。
6. native/empty 和 OCR 使用不同 route 身份，避免同一页被两种恢复逻辑重复收集。

#### 第四步：执行 native 与 empty

1. native candidate 先提取并校验。
2. auto 模式校验失败时写 pending OCR checkpoint 并加入 fallback；text 模式同类失败直接终止。
3. native 成功页生成原有 `PdfPageDocument` 和 Markdown，原子提交，不生成伪造的整页 PNG。
4. empty 页生成空结果并原子提交。
5. 此阶段不读取模型权重、不创建 runtime。

#### 第五步：确定最终 OCR 队列并按需准备推理

1. 合并初始 OCR、native fallback 和恢复的 pending OCR，再次排序去重并排除已复用页。
2. 最终队列为空时，跳过资产文件校验、安装和 runtime 创建，直接汇总——第三步预比较使用的 `static_identity()` 是只读元数据查询（catalog/registry，含 auto 档位解析），不属于被跳过的“实际解析”（RBLK-4 措辞协调）。
3. 队列非空时调用一次 `InferenceService::prepare`。
4. 准备成功后执行第三步预比较候选页的**终比较**（RBLK-2/RBLK-6，复用的最终裁决；**仅适用于本文档存在待解码页、即将用该 session 执行新解码的情形，RBLK-8**）：以**即将执行本次解码的 session 的完整 `InferenceIdentity`** 对候选页存储身份做平台侧全部**执行**字段复核——模型与解析后 profile、逐资产哈希、runtime（wrapper/sys/upstream）、adapter、解码代次、实际 backend、实际批深（`display` 展示元数据不参与，RBLK-7）——全部一致才保留复用；任何执行字段不一致（含 auto 在预比较后、prepare 时改选 profile 的漂移，GPU→CPU 回退改变批深分层等）移入重处理，保证同一文档内复用页与新解码页处于单一实际执行形态，绝不跨 profile 混用。扫描侧字段（prompt/parser/渲染/生成参数）已在预比较验证且本次运行内不变，不在终比较重复。OCR 回退专项语义（T055）：请求 GPU 但回退 CPU 所写的 checkpoint 在本次实际仍为 CPU 时，即使请求设备不同也复用；实际执行形态变化时重处理。多文档编排（RBLK-6/RBLK-8）：**需要解码的后续文档**若复用已存在的共享 session，其候选页直接与共享 session 的完整执行身份做全执行字段比较（不经过 `static_identity()` 二次解析，消除静态预比较与共享 session 之间的漂移窗口）；**完全复用的后续文档不进入终比较**——静态预比较通过且无待解码页即以零解码为终局，不因前文档已创建 session 而借用其身份重处理（§6.3 第三步纯复用规则与“编排与逐个调用产物一致”承诺优先；跨文档各自保持与其单跑一致的执行形态）；本调用尚无 session 时走预比较 + prepare 后终比较。
5. 准备失败时，为未处理 OCR 页保留可恢复的 pending 状态和 `ocr_required_but_unavailable` 诊断，已提交 native/empty 页不丢失，按现有模型错误路径输出部分结果。
6. 准备成功后保存 `InferenceIdentity` 和 session；同一 session 处理所有 OCR 页（多文档编排下同一调用的全部文档共享该 session，对应 X1 `ModelSlot::Shared`）。

#### 第六步：窗口化 OCR

1. 若已取消，停止取新页。
2. 按冻结策略组装窗口：批深取自 `InferenceIdentity.batch_depth` 的实际值（默认 8、VRAM ≥ 12 GiB 档 16、GPU 回退 CPU 后 8；`WINDOW_DEPTH = 8` 为分层的 floor 常量）、按页序填充、不做动态回填（窗口不满按实际页数运行）；恢复时已复用页在窗口前逐页排水，不改变在飞窗口的批组合；窗口按文档独立组成，不跨文档混窗（X2 已裁决不做）。
3. 对窗口内每页渲染 RGB8，保持 DPI、PdfiumOwner 线程与预取（P3）、worker 分段、像素缩放和 render audit 不变；扫描应用取得 `ocr-markdown-v1` prompt，构造只含 prompt、图像、生成参数的 `TaskRequest`（attempt 1 不携带执行变体——纯贪心扫描级预算）；页码留在扫描局部变量。
4. 一次 `submit_window` 提交整窗（attempt 1 全窗批处理）；进度回调携带槽位序号，扫描 callback 映射页码后再交给 Reporter，通用进度事件本身不含页码。
5. adapter 编码各任务，backend 按槽位解码生成；每个解码秒至少发一次进度，阻塞图像编码开始时发一次事件。窗口解析规则：单页失败只占该槽位条目；批级失败（`WindowFailed`）整窗以批深 1（等价单流）重提；退化页与纯截断页按 attempts 状态机走 §2.3 冻结阶梯，以带执行变体的新请求单独按批深 1 重试——attempt 2 `image_tokens_override=2048`，attempt 3 `retry_sampling = repeat_penalty 1200 milli / window 64`，attempt 4 切栏档（受 `--column-split-retry` 门控）把一页分解为 N 个条带子请求，按 §2.3 冻结合并规则合成页产物：`raw_text` 按栏序直接连接、`think_blocks_stripped` 求和、页级退化/截断状态取保守合并、warnings 按栏序追加、条带不产生 figure 裁剪。
6. 每页返回 `TaskResponse` 后，扫描应用将 `max_tokens` 映射为截断、`repetition_loop` 映射为退化处理，仅以 `text` 将文本解析为 `PageDocument`，保留公式、表格、视觉区域和重复尾部处理；`think_blocks_stripped` 计入页统计。
7. 扫描应用从原 RGB8 裁剪 bbox，按 `page-NNNN-figure-MMM.png` 落盘，替换 bbox 占位符并验证所有图片链接存在且不逃逸；`raw_text`（剥离与尾部裁剪之前的规范化原始输出）按 `page-NNNN.raw` 写 sidecar。
8. 机制 A 窗口原子提交：窗口内全部页到达终态（成功或失败）后，按页序执行既有逐页落盘——整页 PNG、页 Markdown、页 JSON、render audit 和 v6 checkpoint，沿用“产物先 durable，checkpoint 最后 complete”的顺序；取消的在飞窗口零提交，恢复时整窗重扫。
9. 单页渲染/识别/解析失败记录诊断并继续同批其他页；取消不记为质量失败并停止取新页；截断页/退化页提交但 job 为 partial。

#### 第七步：汇总与报告

1. 只收集身份兼容、checkpoint complete、Markdown 及其引用图片哈希匹配的页。
2. native、empty、恢复页和 OCR 页统一按页码排序合并，重复页或 0 页号拒绝。
3. 全部所需页成功后生成 `document.md`；按 `--keep-pages` 决定是否删除整页 PNG，figure 裁剪保留。
4. 无可合并页时删除陈旧 `document.md`，不输出伪造成功文档。
5. 写每页 routing sidecar 和 `result.json`，模型字段从实际 `InferenceIdentity` 生成（`name/family/quantization` 读 `display` 展示元数据，RBLK-5 公共通道；纯复用零加载无 session 时模型对象为 null），backend 使用实际执行值，GPU→CPU 回退后必须报告 CPU。
6. 保持文本/JSON stdout、进度 stderr、routing summary、warnings、diagnostics、图片计数和退出码行为。
7. 多输入时以上 1–6 按文档逐份执行（文档 FIFO、共享同一 session，对应现 `ModelSlot::Shared`），每文档独立窗口、机制 A、checkpoint 与产物目录（`<output-root>/<doc-stem>-<hash8>/`）；全部文档结束后由扫描应用输出 `scan-summary.json`（schema_version 1：`mode/inputs/output_root/documents[{input,output_dir,status,exit_code}]/elapsed_ms`，写失败降级 warning）并按 130 > 文档级失败 3 > 部分 10 > 0 聚合退出码；扫描中取消=整调用 130，已提交文档保留；`--report json` 时编排 stdout 恰好一个聚合 JSON 对象，逐文档结果只在各自目录文件中（X1 冻结 stdout 契约）。

### 6.4 模型协议与 OCR 业务解释的分界实例

对于 OCR 页，推理平台只看到：

```text
prompt = 扫描应用给出的完整 OCR 指令
images = 一张 RGB8 图像
max_output_tokens = 16384（或用户有效值）
temperature_milli = 0
thinking = false
image_tokens_override / retry_sampling = 仅阶梯重试请求携带（§2.3 冻结表）
```

它不知道这是 PDF 第几页，也不知道输出要有 checkpoint。它只返回响应（清理后文本、原始文本、结束原因、用量、think 清理计数与解码回执）。随后扫描应用独立完成：

```text
TaskResponse.text
  → 去除/解释 bbox 占位符
  → Markdown / Formula / HtmlTable / VisualRegion
  → PageDocument.page_number = 扫描局部页码
  → 图片裁剪与链接替换
  → PdfPageDocument / checkpoint / document.md
TaskResponse.raw_text  → page-NNNN.raw sidecar（剥离/裁剪前的规范化原始输出）
TaskResponse.think_blocks_stripped → v6 checkpoint 字段与 result pages 计数聚合
```

后续接入另一 OCR/VLM 时，需要变化的是内置 manifest、协议 adapter、兼容 binding 及必要的扫描响应解释策略；PDF 路由、页面队列、取消、checkpoint 和合并机制不因模型名称而改变。

---

## 7. checkpoint、缓存和报告兼容策略

### 7.1 结构化扫描兼容身份

新的 `ScanCompatibility` 至少包含：

```text
schema_version = 1
input_sha256
route = native_text | empty | ocr
pdf = {
  pdf_mode, quality_tier, classifier_version, extractor_version,
  platform
}
ocr = null 或 {
  inference = 完整 InferenceIdentity,
  prompt_id = "ocr-markdown",
  prompt_version = "ocr-markdown-v1",
  prompt_schema_version = 1,
  dpi,
  max_image_tokens, max_output_tokens, context_size,
  batch_depth,                    # 实际生效值：默认 8、16 档或回退 8；透传自 InferenceIdentity.batch_depth
  decode_generation,              # 解码代次（当前 2，D-15）；透传自 InferenceIdentity.decode_generation
  pixel_width_budget = 2880, pixel_height_budget = 2880,
  page_parser_version = "ovis-page-v1"
}
```

`prompt_schema_version` 是独立的数值字段，当前固定为 1；展示层直接读取该字段，不从 `prompt_version` 字符串解析数字。`batch_depth` 与 `decode_generation` 由平台身份透传写入，扫描应用不得硬编码或从设备信息重新推导。dpi 只属于 OCR 渲染身份：native/empty 不渲染页面，因此不把 dpi 放入其兼容身份。

匹配规则：

- native/empty 精确比较输入、route、`pdf_mode`、`quality_tier`、`classifier_version`、`extractor_version` 和 `platform`；dpi、批深、profile、device、token、context 等 OCR-only 配置变化不影响复用，与 v5 native 的有效匹配范围（前五个公共参数，含 `platform`）一致。
- OCR 匹配分两阶段（RBLK-2/RBLK-4/RBLK-6/RBLK-7/RBLK-8）：**预比较**（prepare 前的零加载快速路径）覆盖除实际 backend、实际批深与 `display`（报告专用，RBLK-7）外的全部执行字段——dpi、解析后 profile 与逐资产哈希、runtime 版本和提交、adapter 版本、解码代次（以上经 `InferenceService::static_identity()` 只读查询取得，含 `auto` → 具体档位解析；不校验资产文件、不安装、不加载 runtime）、prompt 版本和 schema 版本、生成参数、渲染参数和页面解析版本（scan 自持）；**终比较**（prepare 后、仅当**本文档存在待解码页**并将用该 session 执行新解码；复用的最终裁决）以即将执行解码的 session 的完整 `InferenceIdentity` 对候选页存储身份做平台侧全部**执行**字段复核（模型/profile/逐资产/runtime/adapter/解码代次/实际 backend/实际批深；`display` 显式排除，RBLK-7），一致复用、任何执行字段不一致重处理——批深或代次不同即不兼容（跨批深/跨代次不互恢复，P4.2 §② 与 D-15 冻结），profile 或任一静态执行字段因 auto 漂移而不一致同样重处理（RBLK-6），且同一文档不混执行形态；多文档编排下**需要解码的后续文档**直接与共享 session 完整执行身份比较，不经 `static_identity()` 二次解析，而**完全复用的后续文档不进入终比较**，以静态预比较为终局零解码复用（RBLK-8：与其单跑行为一致，不因共享 session 存在而重处理）；全部候选通过预比较且无待解码页的纯复用扫描以预比较为终局，不加载模型（零解码无混形态风险）。预比较比较的是 checkpoint 存储身份与**内置 manifest/registry 元数据**，不是当前磁盘资产文件——资产文件损坏只影响新解码，由 prepare 的资产校验把关；已提交页的产物完整性由产物哈希复核（§6.3 第三步）。checkpoint 存储完整 `InferenceIdentity` 含 `display`（审计用途），但匹配函数对 `display` 显式排除——展示文案或量化展示名变化不触发重处理（RBLK-7 区分性测试见 T055/T100）。
- 窗口提交是编排层策略，不进入 checkpoint 身份；批组合的确定性由固定批深与按页序填充保证，身份只需携带批深。
- 不用字符串前缀或“前五个参数”表达匹配优先级，不用 `parameters.contains(...)` 判断 route；v6 通过具名字段实现与上述规则相同的语义，而不是继续依赖字符串布局。
- `pending_ocr` 是 checkpoint 状态字段，不再拼进 compatibility 字符串。
- 身份序列化使用确定性字段顺序；资产按 role 排序，保证同配置生成相同字节。

### 7.2 v6 checkpoint 与 v5 迁移

v6 使用版本化结构化内容，至少保存：

```text
schema = "ocrx-checkpoint-v6"
page_number
status = complete | pending_ocr
compatibility = 完整 ScanCompatibility
output_sha256
truncated
degenerated
attempts
raw_sidecar
think_blocks_stripped
warnings
render_audit
```

`degenerated`（bool）、`attempts`（写入范围 1..=4，= 1 + 冻结阶梯长度 Q1.4a；读取兼容更宽值如 5，镜像基线 Q1.4b 的加宽读取）、`raw_sidecar`（`page-NNNN.raw` 指针，内容为剥离与尾部裁剪之前的规范化原始输出）和 `think_blocks_stripped`（u32，adapter 解码期清理计数）承接现有 v5 字段的语义：退化页即使其余身份兼容也不复用；raw sidecar 指针与产物一同纳入完整性收集；think 计数聚合进 result `pages.think_blocks_stripped_total` 并随 checkpoint round-trip。

写入仍使用临时文件、flush/sync、原子 rename；complete 只能在窗口内全部页到达终态后、且该页 Markdown、JSON、图片、raw sidecar 和 render audit 成功后写入（机制 A：取消的在飞窗口零提交，恢复整窗重扫）。

v5 读取和迁移规则必须保守（v5 现状：checkpoint 已含 `degenerated`/`attempts`/`raw`/`think_blocks_stripped` 字段，兼容串为 `pdf-mode;quality-tier;classifier;extractor;platform;dpi;tokens;max_image_tokens;context_size;device;batch;route=native;decode=<gen>` 加可选 `;ocr-profile=`/`;ocr-device=`/`;pending-ocr=true` 后缀；`decode=<gen>` 与 `think_blocks_stripped` 为质量优化轮加入基线的既有字段，迁移盘点必须显式处置，评审非阻断修正 3）：

- v5 文件可以解析，不能仅因版本旧而崩溃。
- 旧 `complete: bool` 和 compatibility 参数中的 `;pending-ocr=true` 后缀按固定规则映射：`complete=true` 且无 pending 后缀对应 v6 `status=complete`；`complete=false` 且有 pending 后缀对应 v6 `status=pending_ocr`；`complete=false` 且无 pending 后缀表示未完成但不是可恢复的 pending OCR，进入 reprocess；`complete=true` 却带 pending 后缀属于矛盾状态，按无效数据处理并进入 reprocess，不能映射为 complete。
- v5 native/empty 页只有在输入、route 和可映射的 PDF/native 字段完全匹配，且产物哈希有效时才允许复用；dpi、批深变化不影响 native/empty 复用（与 v5 native 宽松匹配的有效范围一致），其他无法映射的必需字段按不兼容处理。
- v5 `degenerated=true` 的页在任何身份组合下都不复用，进入重处理。
- v5 兼容串中的 `batch=N` 参数映射为 v6 `batch_depth`；值不同即不兼容，跨批深不互恢复。
- v5 兼容串中的 `decode=<gen>` 与 checkpoint `think_blocks_stripped` 字段按保守策略处置：v5 OCR complete 页一律重处理（见下），两字段不参与其复用判定；v5 native/empty 复用不受 `decode` 影响（宽松匹配分支只比较前五个公共参数，含 `platform`）；`think_blocks_stripped` 缺行缺省 0，解析器不得因该字段缺失而失败——避免 v5 既有解析/统计信息在迁移中无声丢失。
- v5 OCR complete 页缺少资产哈希、runtime/adapter/prompt 的完整结构化身份，**不在新格式下直接复用**，重处理一次并写 v6。这样避免跨模型或跨 prompt 错用旧结果。
- v5 pending OCR 在公共 PDF 身份兼容时可以恢复为待处理队列，执行时使用新的 v6 OCR 身份。
- v4 及更旧文件继续按不支持处理并重处理，不自动删除用户文件。
- 只有用户明确 `--overwrite` 时才按既有安全规则删除输出目录；版本迁移本身不触发整目录删除。

### 7.3 缓存与安装 receipt

- 现有已安装文件路径保持不变，旧缓存迁移逻辑继续可用。
- manifest v2 是内置元数据结构升级，不要求把现有 GGUF 改名。
- 每个 profile 安装完成后最后写一个确定性 `install-receipt.json`，记录 manifest、资产角色、哈希、大小、来源和绑定。receipt 不是信任依据，`verify` 仍重算文件哈希。
- 对没有 receipt 的现有安装，只要内置 manifest 的全部文件大小和哈希通过，就视为已安装；下次显式 install/ensure 可补写 receipt，不强制下载。
- 安装锁放在 revision 事务范围内，覆盖主模型和共享资产；所有文件先写 `.part`，全部验证后提交，失败按逆序恢复备份并清理临时文件。
- 删除以 catalog 和其他 profile 的实际状态判断共享资产引用，不能只数一个假设固定命名的文件。

### 7.4 对外报告

`result.json` 保持 `schema_version = 3`、既有字段和字段顺序（`pages` 计数含 `total/completed/failed/truncated/degenerated/think_blocks_stripped_total`，其中 `degenerated` 为复读退化方案、`think_blocks_stripped_total` 为质量优化轮加入的既有字段——后者聚合各页 `TaskResponse.think_blocks_stripped`，评审 BLK-3）。`ocr_model_loaded=false` 时 `model` 仍为 `null`；为 true 时，模型对象按下表逐字段派生，不允许实施者另造展示值：

| result.json 模型字段 | 唯一派生规则 | 当前 Ovis 的确定输出 |
|---|---|---|
| `id` | `InferenceIdentity.model.model` | `ovisocr2` |
| `name` | `InferenceIdentity.display.display_name`（prepare 时自实际解析的 manifest 填充，RBLK-5 公共通道） | `OvisOCR2` |
| `family` | `InferenceIdentity.display.family` | `Ovis` |
| `profile` | `InferenceIdentity.model.profile`，使用 auto 解析后的具体值 | `compact/balanced/quality/reference` 之一 |
| `revision` | `InferenceIdentity.model.source_revision` | `8a22290a0c42dbcf84739d6d4c2763f877494ae0` |
| `mmproj_revision` | 当前 manifest 的 revision 级共享资产所属 `source_revision` | 与 `revision` 完全相同 |
| `adapter` | `InferenceIdentity.adapter.adapter` | `ovisocr2` |
| `adapter_version` | `InferenceIdentity.adapter.version` | `0.1.0` |
| `prompt_schema_version` | `ScanCompatibility.ocr.prompt_schema_version`，直接序列化数值 | 数值 `1`，不是字符串 |
| `runtime` | 字符串拼接 `{InferenceIdentity.runtime.runtime}@{InferenceIdentity.runtime.wrapper_version}`；内部 RuntimeId 固定为 `llama.cpp`，wrapper_version 固定为 `0.1.161`（Q1.3 pins 修订） | 精确为 `llama.cpp@0.1.161` |
| `quantization` | `InferenceIdentity.display.quantization`，不得按 display name 猜测 | compact=`Q4_K_M`，balanced=`Q5_K_M`，quality=`Q8_0`，reference=`BF16` |

顶层 `backend` 使用 `InferenceIdentity.backend` 的既有小写展示值，GPU 回退后为 `cpu`。`runtime` 的展示规则只用于 result.json；checkpoint 和评估身份仍分别保存 RuntimeId、wrapper/sys 版本和上游提交，不使用拼接字符串代替结构化身份。

- 不为内部重构强行升级对外报告 schema。新增的详细资产身份已经保存在 v6 checkpoint 和评估执行身份中，不必破坏现有 result 消费者。
- doctor 保持现有 `schema_version = 2` 和字段顺序 golden；内部改为管理 DTO 后，展示层生成相同结果。
- 评估报告额外记录完整 `InferenceIdentity`、prompt/parser 版本、渲染参数、truth/set 版本，确保同一评估可复跑。

---

## 8. 具体修改内容

### 8.1 根工作区与构建配置

修改 `Cargo.toml`：

- workspace members 增加 `ocrx-primitives`、`ocrx-inference-api`、`ocrx-inference`、`ocrx-scan`、`ocrx-eval`。
- 保留现有 Rust 2024、Rust 1.85、lint 和 serde 工作区设置。
- 不新增模型插件框架、HTTP 框架或异步 runtime 依赖；继续使用现有 `curl` 进程 transport。
- 由 Cargo 更新 `Cargo.lock`，最终用 `--locked` 验证可重复构建。

修改各 crate 的 `Cargo.toml`，使直接依赖严格符合第 3 节。CLI 的 `vulkan/cuda` feature 继续转发到 `ocrx-runtime`；硬件 probe 所需依赖随 probe 移到 `ocrx-inference`，CLI 不再仅为探测直接依赖 GPU 库。

### 8.2 `ocrx-primitives`：抽出无业务技术原语

- 从 `ocrx-core` 移动 `CancellationToken`、`GenerationProgress`、SHA-256 流式/内存实现及测试。
- 保持第一次/第二次 Ctrl+C 计数语义和 NIST/分块哈希测试。
- 让 inference API、models、output、scan 按需依赖该 crate。
- 迁移完成后删除 core 中的重复实现和兼容性 re-export，避免长期出现两个取消类型或两个哈希实现。

### 8.3 `ocrx-core`：收敛为扫描领域

- 保留 `PageDocument`、`PdfPageDocument`、`PageBlock`、bbox、termination（含 `Invalid`/`RepetitionLoop`）、分类、路由、provenance、应用错误和资源报告。
- 删除 `ModelId` 枚举、`AS_STR`、`RUNTIME_COMPATIBILITY` 和 `Device`；模型与 runtime 身份由 inference API 拥有。`WINDOW_DEPTH`（floor=8）、`window_depth_for_vram`（16 档分层与 12 GiB 阈值，随 DeviceManager 分层选择）与 `BATCH_ERROR_PREFIX` 随窗口契约迁到推理侧（API 常量与设备分层），core 不再为 runtime 协议提供常量。
- `Profile` 若仅是扫描配置展示用途则移到 scan/CLI；模型 profile 标识统一使用 inference API 值类型。
- 原有页面 JSON 序列化 golden 不变。

### 8.4 `ocrx-inference-api`：建立稳定公共契约

- 新建第 5 节所有 DTO、trait、错误和校验。
- 请求构造器校验图像宽高、RGB8 长度、token 上限、空 prompt 策略和图像 ID 唯一性。
- 执行变体与响应回执按 §5.3/§5.4 校验：`RetrySampling`（penalty 1000..=2000 milli、window ≥ 1）、`image_tokens_override` 受支持网格值；`DecodeEcho` 只能由 backend 填入 `BackendOutput` 并经 adapter 原样透传（RBLK-1 通道）；`TaskResponse` 双文本不变式（`raw_text` 为剥离/裁剪前规范化原始输出、`think_blocks_stripped` 非负计数）有合约测试。
- 当前 OCR prompt 非空；通用 API 是否允许空 prompt 由 `TaskRequest::validate` 明确，不允许 backend 各自判断出不一致结果。
- fake 测试证明公共类型中不存在页码和 PDF 字段，并覆盖序列化身份的稳定顺序（含 `decode_generation`、`batch_depth`）。

### 8.5 `ocrx-models`：通用化资产管理

- 把 `ModelManifest` 从两个哈希字段改为具名资产列表，保留 Ovis 数据到 `builtin/ovisocr2.rs`。
- 把 `community_manifest/community_asset_urls/community_asset_sizes` 替换为 catalog 查询；旧函数不保留为绕过 manager 的第二入口。
- 把缓存、安装、校验、锁、回滚、legacy 迁移和删除按模块拆开。
- 用 `Transport` 抽象下载；生产实现仍执行 HTTPS-only curl，测试使用本地 fake transport，不访问网络。
- 将 `recommend_device` 和硬件资源探测移出；profile 推荐仍在此基于模型元数据和内存输入执行。
- 将 runtime/adapter 兼容检查从硬编码全局常量改为 manifest binding 与 registry 实际身份的严格比较；比较经 inference-api 的 `BindingVerifier` 端口执行（RBLK-9）——models 不依赖 inference，端口实现在组合根注入，models 内只保留端口调用与 fake 测试。
- 所有现有缓存安全、幂等安装、磁盘不足、许可证、共享 mmproj 和损坏 hash 测试迁移并扩展到多角色资产。

### 8.6 `ocrx-runtime`：变成纯 backend

- `InferenceRequest/InferenceResponse` 由 API 的通用任务类型替代，删除 `page`；窗口提交以槽位序号标识页，不再依赖业务页码。
- `Runtime` 改为内部 `RuntimeBackend`，输入是 adapter 已编码的 `EncodedTask`；P4.1 推理线程 actor 与 P4.3 多槽位窗口解码（多序列 batch、逐序列 EOS/环检测、窗口上下文容量计算、批深回退 1 语义）保留为 backend 内部实现，暴露为可选 `generate_window` 覆盖。
- 执行变体落地（§5.4）：`image_tokens_override` 请求以覆盖值绑定该次解码的 MTMD token 网格（重试路径按批深 1 单独执行）；`retry_sampling` 请求按 “penalty 先改 logits、greedy 终选” 的链序挂 per-sequence 采样器（链序进单测，实现为相反顺序则惩罚不生效）；backend 把实际生效变体填入 `BackendOutput.decode_echo`（RBLK-1 通道，adapter 原样透传）；`DECODE_GENERATION` 代次常量随身份迁入推理侧单一来源。
- 从 `llama_mtmd.rs` 删除 `PROMPT`，只根据 `ImagePlacement` 插入 MTMD 默认 marker 并拼接任务 prompt。
- 保留 GPU 层数、CPU `main_gpu=-1`、context（单流 20480 默认 + 窗口动态容量）、图像 token、EOS、贪心、逐 token telemetry（窗口内全序列聚合累加）、阻塞编码进度和取消检查。
- 保留 `step_profile.rs` 采集实现（`OCRX_STEP_PROFILE` 开关、累计快照与黄金行为不变）；`StepProfile` 类型与 `delta_from` 语义定义在公共 API（RBLK-3 修订：与 `RuntimeTelemetry` 同类的可选诊断 DTO，§5.3），backend 经 `InferenceSession::step_profile()` 暴露累计快照，env 关闭返回 `None`；scan 据此做逐文档差分，不得引用具体 runtime 类型。
- 将字符串错误改为结构化错误，但保留 CLI 可识别的设备初始化类别；批级窗口失败映射为 `WindowFailed`，替代现 `BATCH_ERROR_PREFIX` 字符串协议。
- fake 改成可编程响应队列，可检查接收到的 prompt/图像/参数并模拟 EOS、max_tokens、`repetition_loop`（现 `LoopFakeRuntime` 语义）、backend error、批级窗口错误和 cancel；不再返回 `page {request.page}`。
- runtime factory 返回实际 backend 和 runtime identity；不接触模型 catalog 或安装。

### 8.7 `ocrx-ovisocr2`：只保留协议 adapter

- 新建 `OvisProtocolAdapter`，ID/version 保持 `ovisocr2/0.1.0`。
- 移出 `write_page_artifacts*`、`parse_page*`、`adapt_response`、bbox、Markdown 和图片逻辑。
- decode 职责按 §5.4 实现：换行规范化 backend 原始输出 → 剥离 `<think>` 并按 Q3.1 口径计数（完整块含静默空剥离 + 散落标签）→ 产出双文本（`text` 清理后 / `raw_text` 剥离与裁剪前）；`decode_echo` 自 `BackendOutput` 原样透传（RBLK-1 通道，不从请求拷贝）；尾部裁剪归扫描应用产物组装（页面文档解释规则），不属 adapter。
- 删除对 `ocrx-core`、`ocrx-runtime`、`ocrx-pdf` 和 `image` 的依赖，只依赖 inference API。
- 添加协议测试：图像在 prompt 前、prompt 字节不变、生成参数不变、native finish 正确映射、think 剥离 golden（完整块/散落标签/静默空剥离三类）、`raw_text` 与 backend 原始输出（规范化后）逐字节相等、`think_blocks_stripped` 计数正确、`decode_echo` 透传相等。
- 原 real-model 测试迁移到 scan 或 inference 的可选集成测试，不再让协议 crate 知道 PDF 输出。

### 8.8 `ocrx-inference`：实现平台组合与管理

- 建立静态 registry，注册当前唯一模型、adapter、runtime factory 和 binding。
- binding 同时校验模型所需资产角色、adapter ID/version、runtime wrapper/sys/upstream 版本（0.1.161/0.1.161/`d81235049384534c167caea52b85a694f6103d14`）和支持的图像输入。
- 依据静态 registry 提供 `BindingVerifier` 端口实现（RBLK-9：inference-api 定义 trait，本 crate 实现，组合根注入 `ModelManagerImpl`），供 `model verify` 与 prepare 共用同一绑定校验语义；纯数据比较，不创建 runtime。
- `prepare` 实现完整的资产就绪、设备选择（含批深分层与 GPU→CPU 回退后重选 8，§5.5）、runtime 加载、GPU→CPU 回退和 session 创建；实际批深与解码代次（当前冻结 2）由平台写入 `InferenceIdentity`。
- 将 CLI 的 `RealSystemProbe`、设备可用性交集、doctor 支撑和 GPU 采样迁入。
- 保留“显式设备不可用直接失败”和“已选 GPU 在初始化时失败则回退 CPU并 warning”两种不同阶段的不同语义。
- session 持有 backend、adapter、identity（含实际 `batch_depth`/`decode_generation` 与 `display` 展示元数据——prepare 时自实际解析的 manifest 一次性填充，RBLK-5）、累计 telemetry 与可选 StepProfile 快照；每次 submit 只做 encode → generate → decode。代次经 `identity().decode_generation` 暴露 prepare 时身份——session 没有独立的 `decode_generation()` 查询入口（v7.5 评审注记 1），`InferenceService::decode_generation()` 静态查询与静态 registry 同源；`step_profile()` 按 §5.3 契约透传 backend 快照。
- 实现 `static_identity()` 只读查询（RBLK-4）：经 catalog 解析模型与 profile（`auto` 按 §2.3 冻结的内存推荐规则解析，与 prepare 实际解析共用同一推荐实现，保证同输入同结果；内存变化可使两次解析不同，RBLK-6——不依赖两次解析必然相等，由终比较全执行字段复核闭环）、拼装内置 manifest 逐资产元数据与静态 registry 的 runtime/adapter 绑定值及解码代次；任何路径都不得在该查询中校验资产文件、安装或创建 runtime（架构检查与单测断言）。

### 8.9 `ocrx-output`：去掉扫描编排，升级持久化身份

- 移出 `ScanService`、`ModelLoader`、`PageRecognizer`、`PageArtifactRecognizer`、OCR 队列和页面计划逻辑到 scan；P4.4 窗口编排（窗口组装/恢复页排水/槽位解析/批级失败批深回退 1/机制 A 窗口原子提交）与复读退化重试状态机随 `ScanService` 与识别抽象（含 `recognize_window_cancelled`）一并迁到 scan。
- 保留 `PageLifecycle` 或移到 scan/state；output 只关心持久化状态，不决定下一页走哪条路线，也不决定窗口怎么组装。
- 用 `ScanCompatibility` 替换 `CompatibilityKey` 和字符串 `parameters`。
- 新写 v6，按第 7 节读取 v5；保留 complete/output hash/truncated/degenerated/attempts/raw sidecar/think_blocks_stripped/warnings/audit 和 pending 状态。
- 保留 `commit_page`、`commit_native_page`、原子写、输出路径保护、产物哈希、Markdown 合并、Reporter 和现有 golden。
- 保留 `commit_profile.rs` 插桩（`CommitProfile`/`CommitRecorder`、`schema_version: 1` 黄金串、generation 键控、env 开关、取消/失败零写盘）；记录调用点在扫描/识别/提交路径上，经共享 recorder sink 传递，output 不因此反向依赖 scan。
- output 可以依赖 inference API 来保存 `InferenceIdentity`，但不得依赖具体 models/runtime/adapter。

### 8.10 `ocrx-scan`：形成真正的扫描应用

- 从 CLI 迁移 `run_scan` 的业务部分，对外提供不调用 `std::process::exit` 的 `ScanApplication::run`。
- 从 CLI 迁移分类队列、native-first、fallback、pending 恢复、最终 OCR 计划、renderer（含 P3 `pdfium_owner.rs` 的 PdfiumOwner 线程所有权、预取协议、扰动渲染转发和跨文档 `SwitchDocument` 协议：清预取缓存、重置 `next_prefetch`、文档级错误为 Err 值不中毒 owner）、recognizer、路由 sidecar、结果汇总和遥测。
- 在 `window.rs` 实现窗口编排：批深读取 session 身份的 `batch_depth`（不自行从设备信息推导）、按页序填充、不做动态回填、恢复页排水、批级失败（`WindowFailed`）整窗批深回退 1、机制 A 窗口原子提交；窗口策略是扫描编排语义，不进入推理 API 请求类型。
- 在 `ladder.rs` 实现组合退化阶梯与切栏编排：§2.3 冻结的 `DegenerationRetryStep` 数据表（2048 网格 → penalty 采样链 → 切栏）、纯截断页入阶梯、切栏档把一页分解为 N 个条带子请求并按 §2.3 冻结合并规则合成页产物（`raw_text` 按栏序直接连接、`think_blocks_stripped` 求和、状态保守合并、warnings 顺序追加、无 figure 裁剪）、`--column-split-retry` 类型门控（默认开）；窗口常量 `WINDOW_DEPTH`（floor=8）与网格值来自 inference API。
- 在 `multi_doc.rs` 实现跨文档编排（X1）：文档 FIFO、全部文档共享一次 prepare/session、`SwitchDocument` 调度边界、逐文档机制 A 与 checkpoint、取消=整调用、退出码聚合（130 > 3 > 10 > 0）与 `scan-summary.json` 生成（含 `<stem>-<hash8>` 输出目录解析）；单输入路径保持历史形态。
- 遥测与插桩映射：runtime StepProfile 经共享 session 时逐文档取 `delta_from` 基线增量（X1 既有语义）；CommitRecorder sink 从扫描/提交路径传入 output 插桩（§8.9），`step-profile.json`/`commit-profile.json` 每文档一份、黄金串与取消/失败零写盘语义不变。
- 从 runtime 迁移完整 prompt 到 `ocr/prompt.rs`。
- 从 `ocrx-ovisocr2` 迁移页解析、bbox 裁剪、Markdown 替换和链接验证到 `ocr/`；页产物组装消费 `TaskResponse` 双文本：`text` 驱动解析、`raw_text` 写 `page-NNNN.raw` sidecar、`think_blocks_stripped` 进页统计。
- 定义 `ScanRequest/ScanOutcome/ScanEvent`，由 CLI 将事件映射为现有 Reporter；扫描库不直接打印终端，也不自行安装进程级 signal handler。
- 用 API session 替代 `PreloadedLoader<LlamaMtmdRuntime>` 和 `OvisRecognizer<LlamaMtmdRuntime>`（多文档共享槽对应同一 session 复用）。
- 所有页错误返回结构化 outcome，由 CLI 决定退出码，保证测试可以直接调用完整业务而无需启动子进程。

### 8.11 `ocrx-pdf`：保留 PDF 基础设施，移出评估

- 保留 PDFium renderer、worker、页面信号、分类、native 提取和校验、empty/native 产物、像素预算、bbox 和错误分类。
- 将 `QualityManifestEntry`、质量 manifest 加载/校验、NED、block/bbox/阅读顺序指标、路由混淆、native release gate、performance gate 和 release report 移到 `ocrx-eval`。
- 不修改分类和 native 校验函数输入输出；迁移测试必须逐项同名或建立清晰的一一对应。
- 移除 PDF crate 对质量真值和发布报告的所有权。

### 8.12 `ocrx-eval`：建立独立质量模块

- 接收从 PDF 移出的 Rust 质量契约、指标和门禁，保持数值与报告 golden。
- `execute.rs` 提供两种明确入口：完整 PDF 质量评估调用 `ScanApplication`；模型/runtime 专项评估调用 `InferenceService` 并由评估方提供 prompt、图像和真值。
- 内置“同参重扫逐字节比对”评估原语（§3.5 第 10 条执行落点）：同一输入以相同参数经 `ScanApplication` 双跑，逐字节比对页产物（时间戳等非确定字段除外），服务代次自一致与迁移回归双场景，fake 输入即可执行、不依赖真实模型。
- 评估身份包含 set/truth 版本、输入 hash、完整 inference identity、prompt/parser 版本和渲染参数。
- 既有 `assets/regression/` 的 manifest、truth、predictions、reports 路径和 JSON schema 不因重构改变。
- 既有 `scripts/score-regression.py`、`run-quality-gate.py`、native 评分和验证脚本继续作为可执行工具；将脚本调用对象收敛到公开 CLI 或版本化文件契约，不让脚本复制 Rust 扫描/资产/设备流程。
- 用同一批小型输入比较迁移前后评分结果，不能通过改阈值或改真值获得通过。

### 8.13 `ocrx-cli`：收缩为入口和展示

- `main.rs` 只保留参数到 command 的分发和组合上下文调用。
- 删除 `run_scan` 及其辅助业务函数（含跨文档编排代码 `run_scan_multi`/`scan_document`/`DocScanEnv`/`ModelSlot`/`resolve_scan_inputs`/`scan_summary_json`/退出码聚合，全部迁至 scan `multi_doc.rs`），删除 `OvisRecognizer`、`PreloadedLoader`、`PdfScanRenderer`、`WorkerScanRenderer`、`pdfium_owner`（迁至 scan 渲染路径）、`load_runtime_with_fallback`、模型安装实现和资产路径拼接。
- 删除对 `ocrx-runtime`、`ocrx-models`、`ocrx-ovisocr2`、`ocrx-pdf` 的直接依赖；CLI 通过 `ocrx-inference` 组合，通过 `ocrx-scan` 执行。
- 保留参数、help、默认模型 `ovisocr2`、配置优先级、输出格式、doctor 展示、退出码和 skill 命令。
- 将 `serialize_scan_result` 改为基于 scan outcome 的展示序列化，字段和顺序 golden 不变。
- CLI 不直接打印扫描库内部错误；统一由 present 层输出，避免重复报告。

### 8.14 测试、脚本和说明的同步修改

- 现有单元测试随所有者迁移，不允许通过删除测试解决类型变化；X1 编排单测（同一线程切换、预取重置、Err 值跳过、poison fail-loud、目录展开/去重、退出码聚合）与 T1 插桩测试（StepProfile/CommitProfile 黄金串、generation 键控、disabled no-op、取消/失败零写盘、`delta_from` 逐文档增量隔离）随所有者迁移并通过。
- 新增 inference API 合约测试、静态绑定测试、多角色资产测试、扫描应用 fake 端到端测试（含多文档编排）和架构依赖检查。
- 新增 `scripts/validate-architecture-boundaries.py`，只用 Python 标准库读取 Cargo manifest 和源码，检查禁止依赖与禁止类型名；本机可直接运行。
- `scripts/bootstrap-check.sh` 增加架构检查和 Python 单元测试，但不增加网络、真实模型或 GPU 命令。
- README 中的用户命令保持有效；仅在开发说明需要时更新 crate 职责，不改变支持第二模型的宣传。
- 清理所有描述旧行为的注释，例如“Ovis adapter 写页产物”“runtime 请求含页码”“output 提供扫描服务”“core 固定 runtime adapter”。

---

## 9. 测试与验收方案

### 9.1 本机必跑门禁

以下门禁全部不访问外网、不下载真实模型、不要求 GPU：

```bash
cargo fmt --all -- --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python -m unittest discover -s scripts -p "test_*.py"
python scripts/validate-architecture-boundaries.py
bash scripts/test-release-scripts.sh
bash scripts/bootstrap-check.sh
cargo run --locked -p ocrx -- version
cargo run --locked -p ocrx -- doctor
cargo run --locked -p ocrx -- doctor --report json
cargo run --locked -p ocrx -- model list
cargo run --locked -p ocrx -- model info
```

如果本机已有仓库内 PDFium 和 fixture，运行已有 CLI 扫描测试即可覆盖 native/empty 路径；缺少 PDFium 时，现有测试按自身条件跳过，不能改为联网下载依赖来制造门禁。

### 9.2 必测契约矩阵

| 领域 | 必须通过的测试 | 完成判定 |
|---|---|---|
| 架构 | Cargo 依赖白名单、scan 禁止具体类型、推理禁止扫描/PDF/output/eval | 检查脚本返回 0，违规用例会被测试 fixture 检出 |
| API | 图像形状、token 参数、TaskRequest 无页码、身份稳定序列化 | 合法/非法输入均有断言 |
| prompt/adapter | prompt 完整字节、marker 在 prompt 前、生成参数、finish 映射 | Ovis 编码结果与冻结契约一致 |
| 资产 | 四 profile 元数据、具名角色、共享 mmproj、路径逃逸、大小/hash | 与冻结值逐项相等，非法资产被拒绝 |
| 安装 | fake transport、离线、许可证、磁盘、幂等、失败回滚、临时文件清理、receipt | 无网络；失败不暴露半套可用资产 |
| 删除 | 删一个 profile 不删仍被引用的 mmproj，最后一个才回收 | 文件存在性逐项断言 |
| runtime | fake 可编程响应、取消、截断、backend 错误、session 多次 submit | 无真实模型；加载次数与调用次数明确 |
| 窗口合批 | 实际批深（身份 `batch_depth`）填充/不动态回填、恢复页排水、批级失败批深回退 1、机制 A 窗口原子提交、取消窗口零提交、同输入同批深自一致逐字节、跨批深/跨代次不互恢复、默认 8/16 档/回退 8 三种实际值 | fake 与受控测试证明，行为与 P4.4 门禁语义一致；无真实模型 |
| 退化阶梯 | 冻结阶梯逐 rung 变体请求（2048 网格/penalty 链序/切栏）、纯截断页入阶梯、切栏多子请求按栏序合并与页级保守合并（raw 连接/think 求和/warnings 顺序）、`--column-split-retry` 门控、attempts 写入 1..=4 且读取兼容更宽 | fake 以阶梯表驱动证明，与 Q1.4a/Q1.5/D-10/D-14 冻结语义一致 |
| 跨文档编排 | 多输入解析/目录展开/去重、文档 FIFO、共享 session 一次 verify+load、`SwitchDocument` 协议、逐文档独立窗口/机制 A/resume、`<stem>-<hash8>` 目录、`scan-summary.json` golden、退出码聚合 130>3>10>0、编排 vs 逐个调用产物一致、取消=整调用 | 现有 multi_doc 测试随迁 + fake 端到端证明 |
| 插桩 | `OCRX_STEP_PROFILE` 开启时 StepProfile/CommitProfile 各段非空、`schema_version: 1` 黄金串、generation 键控、取消/失败零写盘、disabled no-op、`delta_from` 逐文档增量隔离 | 现有插桩测试随迁并通过，黄金串不变 |
| binding | 正确三元组通过，runtime/sys/adapter/角色任一不符失败 | 错误类别为 IncompatibleBinding |
| native/empty | 不准备模型、不创建 session、产物无整页伪造 PNG | fake 计数为 0，产物 golden 不变 |
| OCR | prompt 从 scan 到 fake、文本解析、公式/表格/bbox、PNG 和链接、raw sidecar 为剥离/裁剪前规范化原始输出、think_blocks_stripped 计数 | 页 JSON/Markdown/PNG/raw golden 不变 |
| 路由 | auto/text/ocr、native fallback、empty、队列排序去重 | 现有路由测试全部保留并通过 |
| checkpoint | v6 完整身份（含 `decode_generation` 与实际 `batch_depth`）、任一执行字段变化、完全复用零加载（含默认 `auto` 解析路径，多文档下后续文档同样适用，RBLK-8）、全执行字段终比较且仅限待解码文档（auto 预比较后改选/共享 session 漂移时候选全部重处理，RBLK-6/RBLK-8）、`display` 变化不触发重处理（RBLK-7）、静态身份查询只读性（不校验文件/不安装/不加载）、产物 hash、pending、degenerated 不复用、`think_blocks_stripped` round-trip、v5 迁移（含 `decode`/`think_blocks_stripped` 字段显式处置） | OCR 不跨模型/prompt/runtime/批深/解码代次复用；auto 解析变化与静态执行字段变化触发重处理；同文档不混 profile；完全复用文档与其单跑行为一致；v5 OCR 安全重做且解析/统计信息不丢失 |
| 取消/失败 | 首/二次取消、页间停止、失败继续、已提交页保留 | 退出码和诊断保持，取消不算质量失败 |
| 报告 | doctor、result（name/family/quantization 经 `display` 公共通道，四 profile golden，RBLK-5）、错误、进度、路由 sidecar、路径 escaping/redaction | 现有 golden 和 JSON 解析断言通过；scan 无 models 依赖、无展示映射副本 |
| 评估 | manifest、NED、block、bbox、阅读顺序、路由、release/performance gate | 同一输入迁移前后数值和 JSON 一致 |

### 9.3 行为对比验收

在同一台机器、同一 fixture、同一有效配置下，对重构前后结果做结构化比较：

- 页分类、初始/最终 OCR 队列、fallback 原因一致。
- 页 Markdown、页 JSON、document.md、figure PNG、`page-NNNN.raw` sidecar、render audit 一致；`think_blocks_stripped` 计数一致；时间戳等非确定字段不参与逐字节比较。
- `result.json` 排除 `elapsed_ms` 后一致（含 `pages.think_blocks_stripped_total`）；真实模型文本若存在环境差异，用固定 fake 输出验证业务产物，再用已有真实资产做非门禁观察。
- 首次扫描、完整复用续扫、pending OCR 续扫、模型/prompt 变化后的重处理符合预期。
- native-only 扫描中模型安装和 runtime 加载调用均为 0。
- 多页 OCR 中 prepare/load 为 1；窗口提交次数与窗口组装策略一致；请求计数按类别断言——总提交请求数 = attempt 1 页数 + 批级回退重提数 + 各阶梯 rung 重试/切栏子请求数（attempt 1 请求数本身已等于需处理页数，任何重试只会增加请求，等式不再收敛回页数；窗口数与每类请求数分别断言，评审修正）。
- 同一输入以相同批深连跑两遍，页产物逐字节相同（P4 自一致性语义在重构后保持）。
- 多输入编排与逐个调用对比：每文档产物一致（含完全复用文档零新解码、不借用共享 session 身份，RBLK-8）、`scan-summary.json` 字段/状态/退出码聚合符合 X1 golden、共享 session 每调用恰好一次 verify+load、逐文档 telemetry/StepProfile 为增量口径。
- 所有 CLI 命令的 stdout/stderr 和退出码符合现有 golden（多输入编排 `--report json` stdout 恰好一个聚合对象）。

### 9.4 非门禁的本机可选验证

只有在本机已经具备真实 Ovis 资产、PDFium 和相应硬件时才执行；缺少任一条件记录“未执行及原因”，不阻塞重构完成：

```bash
OCRX_E2E_MODEL=1 cargo test --locked -p ocrx --test real_scan_fault_injection -- --ignored --test-threads=1
OCRX_E2E_MODEL=1 cargo test --locked -p ocrx-scan -- --ignored --test-threads=1
```

可选观察项包括真实模型加载、RGB 图像生成、EOS/max_tokens、多页复用、CPU/GPU、取消和资源采样。它们不是 Qwen 接入证明，也不是本次单模型架构重构的默认交付前提。

---

## 10. 实施顺序与风险控制

### 10.1 保持可编译的迁移顺序

1. 先冻结基线和公开契约，再建 primitives/API 新 crate。
2. 先让通用模型资产和 runtime/adapter 在各自 crate 内通过单元测试，再组合 inference。
3. 先升级 output 的身份和持久化，再迁移 scan；不要同时从 CLI 大段删除代码而没有新调用目标。
4. 新 scan 用 fake 完整跑通后，CLI 才切换到新组合根。
5. 旧 CLI 路径删除后迁移 eval 和脚本，最后清理兼容 re-export、旧注释和无用依赖。
6. 每一阶段都运行本阶段相关测试；最终再跑完整门禁，不能把所有编译错误留到最后一次解决。

**T009–T077 采用“新增实现先行、旧导出最后删除”的过渡策略。** 在 CLI 于 T074 切换到 `ScanApplication::run`、并于 T077 删除旧依赖之前，旧 CLI 及其集成测试必须始终可以编译。具体规则如下：

- T019 的旧 `community_*` 函数可以委托新 catalog；T030 的旧 `recommend_device` 必须自含保留现有实现，不得从 `ocrx-models` 调用 `ocrx-inference`，以免形成 models → inference → models 的循环依赖。旧函数不得在旧 CLI 切换前删除，均由 T086 最终删除；新生产代码不得新增对这些函数的调用。
- T031–T035 新增通用 backend 时，暂时保留旧 `Runtime`、请求/响应和 fake 的兼容包装。旧包装可以把 legacy 请求转换为 `EncodedTask` 并暂时保存冻结 prompt，供未迁移 CLI 使用；旧 `Runtime::generate_window`（P4.3 落地的窗口方法）同样经由兼容包装委托新 backend，未迁移 CLI 的窗口行为与批级错误协议（`BATCH_ERROR_PREFIX`）不变；新 `RuntimeBackend` 路径本身不得含 prompt/page。CLI 切换后，runtime 的 legacy 包装由 T087 删除。
- T036/T037 的新协议 adapter 只依赖 inference API；旧 Ovis 页解析和产物函数在原 crate 中暂时原样保留，T037 采用“复制到 scan、保留旧实现”的方式，不让 adapter crate 反向依赖 scan。旧导出和旧依赖由 T087 最终删除。
- T044–T051 新增 v6 与扫描状态类型时，暂时保留 output 的旧 `CompatibilityKey`、v5 写入和 `ScanService`/loader/recognizer 导出。T051 同样采用“复制到 scan、保留旧实现”，不得提前要求 output 只剩持久化职责；旧实现由 T088 最终删除。
- CLI 的 probe、模型管理、doctor 等旧调用在各自命令切换前保留过渡委托；T069–T073 运行 golden 时，旧 scan 路径仍可通过上述兼容层编译。T074/T077 之后才允许删除旧扫描调用。
- 评估函数迁移也采用 copy-first：T079–T082 可暂留 PDF re-export，T083 切换完全部调用方后删除，避免阶段 H 中间状态破坏 workspace 构建。
- 过渡兼容层不是目标架构的一部分，不得让新 `ocrx-scan`、新推理 API 或新生产组合根调用它。T086–T088 删除后，§3.2 的最终禁止依赖必须在没有任何例外的情况下成立。
- T037、T051、T073、T077、T083、T088 均必须执行 `cargo check --workspace`，以实际编译证明兼容层的保留或删除时机正确。

### 10.2 主要风险及处理

| 风险 | 防止方式 |
|---|---|
| 新 API 名义通用，实际仍含页码或 OCR 字段 | API 类型审查、架构脚本、无 PDF 的独立 crate 测试 |
| prompt 移动后模型输入发生细微变化 | 完整字节 golden、adapter 顺序测试、fake 捕获请求 |
| Ovis 业务解析误留在 adapter | adapter 禁止依赖 core/image/pdf；PageDocument 只能在 scan 构造 |
| 缓存重构导致重复下载或共享文件误删 | 路径不变、旧安装无 receipt 也可验证、共享引用测试 |
| 结构化 checkpoint 错用旧 OCR 结果 | v5 OCR 一律重处理；v6 全身份匹配和产物 hash |
| 实际批深/解码代次被扫描侧硬编码或从设备信息重新推导 | 身份字段只由平台产生并经 `InferenceIdentity` 透传；T055/T064/T100 专项断言回退重选与透传，禁止扫描侧常量副本 |
| native-only 因新 prepare 流程提前加载模型 | fake 计数测试和最终队列后 prepare |
| GPU 回退后身份仍显示 GPU | identity 使用实际 backend，回退专项测试 |
| CLI 迁移丢失历史错误码/输出通道 | 子进程 golden 不变，错误集中映射但保留历史分支 |
| 大迁移同时破坏评分能力 | 评估函数纯移动、同输入数值对比、脚本格式不改 |
| 真实外部条件阻塞开发 | 默认全部 fake/本地测试，真实模型和 GPU 仅可选非门禁 |

---

## 11. 重构完成定义

以下条件全部满足才可将重构标记为完成：

- 目标 crate 和模块边界已经落地，禁止依赖检查通过，旧 CLI 巨型 orchestrator 和旧绕路入口已删除。
- 生产环境仍只注册 OvisOCR2，版本、资产、prompt、生成参数和 PDF 行为未改变。
- 扫描应用只依赖推理 API；推理平台没有 PDF/扫描/output/eval 依赖；OCR 页面解析只在扫描应用。
- 模型管理和 doctor 可独立运行，scan 与模型管理复用相同资产和设备实现。
- native/empty/OCR、fallback、按需加载、单 session 多页窗口合批（P4 冻结语义不回退：批深 8/16 分层、机制 A 原子提交、自一致确定性）、组合退化阶梯（含纯截断重试、切栏合并与门控）、跨文档编排（X1 语义不回退：文档 FIFO、每调用一次加载、`scan-summary.json`、退出码聚合）、可选插桩通道（StepProfile/CommitProfile 黄金串不变）、复读退化处理、GPU 回退、取消、失败继续、续扫和最终合并均通过测试。
- v6 checkpoint 保存完整身份（含解码代次与实际批深），v5 迁移符合安全规则，不跨模型/prompt/runtime/批深/解码代次复用 OCR。
- 现有 CLI、页产物、doctor、result 和质量报告契约保持兼容。
- 第 9.1 节本机门禁全部通过，且没有通过忽略、删除或放宽既有测试来换取通过。
- 可选真实模型/GPU 验证未执行时已经注明原因，但不影响上述完成判定。
- 代码、注释和开发说明不再描述旧的调用关系；没有为“以后可能用到”而保留空 trait、重复兼容层或无用 crate 依赖。

---

## 12. 详细执行 TODO List

### 12.1 状态维护规则

- `-[ ]` 表示未开始；`-[~]` 表示正在执行；`-[x]` 表示已完成并满足该项完成判定。
- 执行者每开始一项就把对应项改为 `-[~]`；每完成一项立即改为 `-[x]`，再开始下一项。同一时间只保留一个主要任务为 `-[~]`。
- 不得仅因代码已写就标记完成；该项指定的测试、文件检查或行为断言必须已经满足。
- 任务按依赖顺序排列。括号内列出直接前置任务；前置任务未完成时不得开始，除非该项明确写“无依赖”。
- 真实模型、GPU、外部 CI 和第二模型验证不属于以下必做任务的依赖，不得被执行者自行添加为门禁。

### 阶段 A：基线与契约冻结

- [x] T001（无依赖）记录当前工作区状态和已有未提交修改，建立只修改本重构所需文件的清单；完成判定：不会覆盖或回滚用户已有修改，基线记录可供后续对照。
- [x] T002（T001）运行 `cargo build --workspace` 和 `cargo test --workspace`，保存通过/失败清单；完成判定：每个既有失败被区分为重构前问题或环境问题，不把既有失败误记为重构回归。
- [x] T003（T001）运行现有 CLI doctor/model/参数错误及扫描失败分支 golden，逐分支记录 stdout、stderr 和退出码特例；覆盖单输入与多输入编排分支（多输入缺 `--output-dir`、目录无 PDF、坏文档跳过、`scan-summary.json` 形状、`--report json` 聚合 stdout、退出码聚合 130>3>10>0）；完成判定：清单显式包含 routing sidecar 提交失败为 `code:5, kind:"output"` 的历史行为与单输入缺失文件的页数失败 exit 3，后续展示测试有明确期望值且不得顺手统一为 9。
- [x] T004（T001）用本地 fixture 记录 native-only、empty、auto fallback、OCR fake 的页面计划和产物 golden；完成判定：每类至少有一个可重复输入和预期输出，缺 PDFium 的项目明确标为环境未执行而非失败。
- [x] T005（T001）逐项核对 Ovis 四 profile 的 revision、URL、模型文件名、共享 mmproj、SHA-256、字节数及展示元数据；完成判定：形成内置 manifest 字段清单，display_name=OvisOCR2、family=Ovis 和四档 quantization 映射明确，所有值与当前代码一致且无需网络。
- [x] T006（T001）冻结完整 `ocr-markdown-v1` prompt、MTMD 图像在 prompt 前的顺序和生成默认值；完成判定：有字节级预期字符串和参数清单，包括开头换行。
- [x] T007（T001）盘点 v5 checkpoint、result schema 3、doctor schema 2、`scan-summary.json`、routing sidecar 和质量报告字段；完成判定：每种格式都有字段级兼容/迁移决定，明确 runtime=`{RuntimeId}@{wrapper_version}`=`llama.cpp@0.1.161`、prompt_schema_version=数值 1、native/empty 忽略 dpi/批深、v5 状态映射（含 `degenerated`/`attempts`/`raw`/`think_blocks_stripped` 字段与 `platform`/`batch`/`decode=<gen>` 等兼容串参数、`;pending-ocr=true` 后缀，`decode` 与 `think_blocks_stripped` 的显式处置——不参与 v5 OCR 复用判定且缺省解析不得失败）、result `pages.think_blocks_stripped_total` 聚合口径，无“实施时再定”的字段。
- [x] T008（T001）盘点 `ocrx-pdf` 中质量清单、指标、门禁与 `scripts/`/`assets/regression/` 的对应关系；完成判定：每个待迁移 Rust 函数都有目标模块和现有测试/脚本验证方式。

### 阶段 B：工作区骨架、技术原语和公共 API

- [x] T009（T002、T003、T004、T005、T006、T007、T008）在根 `Cargo.toml` 注册五个新 crate 并创建最小 package；完成判定：workspace metadata 能解析，新 crate 尚无业务反向依赖。
- [x] T010（T009）实现 `ocrx-primitives` 的取消与进度类型并移动相关测试；完成判定：第一次/第二次取消和进度字段测试通过，类型不含 OCR/PDF 概念。
- [x] T011（T009）将 SHA-256 流式和内存实现移动到 `ocrx-primitives`；完成判定：NIST 向量、分块和文件哈希测试通过，core/models/output 可改用同一实现。
- [x] T012（T009）在 `ocrx-inference-api/id.rs` 实现模型、profile、adapter、runtime、backend、资产角色值类型及校验；完成判定：非法空值/重复角色有测试，当前 catalog 不需要核心枚举。
- [x] T013（T012）实现资产 manifest v2、作用域、来源和绑定 DTO；完成判定：多必需/可选角色、重复角色、非法 hash、非法路径均有单元测试。
- [x] T014（T012）实现 `InferenceIdentity` 及确定性序列化；完成判定：资产顺序固定，模型、runtime（wrapper/sys/upstream 三值冻结为 0.1.161/0.1.161/`d81235049384534c167caea52b85a694f6103d14`）、adapter、实际 backend、`decode_generation`（平台产生，当前 2）、`display` 展示元数据（RBLK-5：display_name/family/quantization 自已解析 manifest；RBLK-7：序列化保存但标注报告专用，匹配函数不得消费）和逐资产 hash 均可序列化比较。
- [x] T015（T010、T012）实现 `TaskRequest/TaskResponse/ImageInput/GenerationParams/RetrySampling/DecodeEcho/Usage/FinishReason/StepProfile`；完成判定：公共请求无页码/PDF/output 字段，RGB8 和 token 非法输入、`retry_sampling` 越界（penalty 出 1000..=2000 milli、window 0）测试通过；`TaskResponse` 双文本（`text`/`raw_text`）、`think_blocks_stripped` 与 `decode_echo` 字段齐全且有合约断言；`StepProfile` 为与 `RuntimeTelemetry` 同类的公共诊断 DTO（`delta_from` 语义随类型迁移，字段名与基线黄金串一致）。
- [x] T016（T015）实现 `ProtocolAdapter/RuntimeBackend/EncodedTask/BackendOutput` 扩展契约；完成判定：EncodedTask 能表达图像在 prompt 前并透传执行变体（`image_tokens_override`/`retry_sampling`），`BackendOutput.decode_echo` 由 backend 填充、adapter decode 原样透传进 `TaskResponse`（RBLK-1 通道），且不包含 Ovis PageDocument、bbox 或 Markdown 类型。
- [x] T017（T010、T014、T015）实现 `InferenceService/InferenceSession/PrepareRequest/InstallInteraction`；完成判定：trait 对象可用于 fake，session 支持同一实例多次 submit 和累计 telemetry，身份携带实际 `batch_depth`/`decode_generation`（fake 提供受控值），扫描侧消费只经身份字段；`decode_generation()` 静态查询 prepare 前可用且不触发加载（RBLK-2 预比较通道）；`static_identity()` 只读查询返回 `StaticInferenceIdentity`（RBLK-4：`auto` 按冻结内存规则解析为具体档位、含逐资产元数据与 runtime/adapter 绑定值，测试断言该查询不校验资产文件、不安装、不创建 runtime；RBLK-7：结构不含 `display`，并有一条测试断言“静态身份 ∪ 实际 backend/批深 = 执行身份 − display”的字段完备性）；`InferenceSession::step_profile()` 返回 `Option<StepProfile>` 累计快照（RBLK-3 通道，env 关闭为 None）。
- [x] T018（T013、T014）实现 `ModelManager/DeviceManager` DTO、结构化错误与 `BindingVerifier` 端口（RBLK-9：inference-api 定义 trait，输入 manifest 输出结构化绑定错误，纯数据比较语义）；完成判定：所有管理和推理失败都有明确类别，CLI 可映射到既有退出码而无需解析任意字符串，端口可被 fake 实现。

### 阶段 C：模型目录、资产与安装管理

- [x] T019（T013）把 Ovis 四 profile 固定数据迁移到 `ocrx-models/builtin/ovisocr2.rs`，旧 `community_*` 函数可暂时委托新实现；完成判定：T005 清单逐项相等，旧常量不再成为第二份事实源，旧函数签名在 T086 前保持不变且旧 CLI 仍编译。
- [x] T020（T019）实现静态 `ModelCatalog` 的 list/info/resolve；完成判定：只列出 `ovisocr2`，profile 去重排序，未知模型/profile 返回结构化错误。
- [x] T021（T013、T019）实现具名资产生成的缓存路径和现有 Ovis 布局映射；完成判定：新旧 Ovis 路径逐字一致，绝对路径和 `..` 逃逸测试通过。
- [x] T022（T021）迁移默认缓存根和 legacy 缓存探测/迁移；完成判定：Windows/macOS/temp fallback、旧布局、篡改文件不迁移等现有测试全部通过。
- [x] T023（T021）实现本地安装状态检测与全资产大小/hash 校验；完成判定：缺任一必需角色、错误大小或错误 hash 均失败，无 receipt 的合法旧安装可通过。
- [x] T024（T018、T021）实现 `Transport` 与 HTTPS-only curl 生产实现；完成判定：offline 在创建进程前失败，HTTP/file/降级重定向被拒绝，测试不访问网络。
- [x] T025（T023、T024）实现 fake transport 和受控本地小文件安装 fixture；完成判定：可模拟字节、网络失败和错误 hash，所有测试输入都在临时目录生成。
- [x] T026（T025）实现 revision 范围安装锁、`.part`、备份和资产组提交；完成判定：主模型与共享资产在同一事务中，锁覆盖全部目标，异常路径清理临时文件。
- [x] T027（T026）实现失败回滚、幂等安装、磁盘与许可证策略；完成判定：任一步失败不暴露半套有效状态，已验证重复安装无网络且成功。
- [x] T028（T027）实现最后提交的 `install-receipt.json`；完成判定：receipt 含完整来源和资产身份，损坏 receipt 不会绕过实际文件 hash 校验。
- [x] T029（T023）实现 profile 删除与共享资产引用检查；完成判定：删一个 Ovis profile 保留仍共享 mmproj，最后一个 profile 删除后回收 mmproj 和空目录。
- [x] T030（T018、T020、T023、T026、T028、T029）实现 `ModelManagerImpl` 全部方法并将设备探测职责移到推理设备服务；完成判定：list/info/recommend/install/verify/remove 均可无 PDF 独立测试（verify 的绑定校验经注入的 fake `BindingVerifier` 驱动，RBLK-9），新生产路径不调用旧 `community_*` 或旧 `recommend_device`；旧 `community_*` 仅作为未迁移 CLI 的过渡委托保留到 T086，旧 `recommend_device` 自含保留现有实现到 T086，`ocrx-models` 不得为委托它而依赖 `ocrx-inference`（架构检查断言 models → inference-api 合法、models → inference 禁止）。

### 阶段 D：runtime、Ovis adapter 与推理组合

- [x] T031（T015、T016）新增接收 `EncodedTask` 的 llama.cpp `RuntimeBackend`（迁移 P4.1 推理线程 actor 与 P4.3 多槽位窗口解码，暴露可选 `generate_window`），执行 `image_tokens_override` 的 MTMD 网格绑定与 `retry_sampling` 的采样链（penalty 先改 logits、greedy 终选，链序单测锁定），并把实际生效变体填入 `BackendOutput.decode_echo`（RBLK-1：含请求值≠实际生效值的断言），同时保留旧 `Runtime`/`generate_window` 的 legacy 包装供未迁移 CLI 使用；完成判定：新 backend 路径无 OCR prompt、页码、PageDocument 或输出目录，重试请求按批深 1 单独解码（2048 网格绑定 / penalty 链生效），legacy 包装能把旧请求与旧窗口调用委托到新路径且旧 CLI 仍编译（批级错误协议保持 `BATCH_ERROR_PREFIX` 行为），固定 binding 版本未升级。
- [x] T032（T031）实现 MTMD 默认 marker 与 prompt 的协议拼接；完成判定：图像在 prompt 前，prompt 字节原样进入 tokenize，RGB8/context/image-token 行为保持；冻结 prompt 只允许暂存于待 T087 删除的 legacy 包装，新 backend 不含该常量。
- [x] T033（T010、T031）迁移逐 token 生成、取消、进度和累计 telemetry；完成判定：EOS/max_tokens/backend error/cancel 映射结构化，编码开始与每秒进度测试通过，legacy 与新 backend 的取消行为一致。
- [x] T034（T031）实现 runtime factory 和实际 backend/runtime identity；完成判定：CPU/GPU 请求与实际 identity 分开，RuntimeId=`llama.cpp`、wrapper=`0.1.161`、sys=`0.1.161`、upstream=`d81235049384534c167caea52b85a694f6103d14` 均准确（Q1.3 pins 修订；解码代次 `decode_generation=2` 由平台身份产生，旧代次 checkpoint 由 `;decode=2` 隔离，不混扫）。
- [x] T035（T015、T016）实现可编程 `FakeRuntimeBackend` 并暂时保留旧 fake 包装；完成判定：新 fake 可捕获请求并排队返回 EOS、max_tokens、`repetition_loop`（现 `LoopFakeRuntime` 语义）、批级窗口错误、错误和取消，`BackendOutput.decode_echo` 可编程且与请求变体独立（供 T042 证明回执来自 backend 而非请求拷贝），`raw_text`/`think_blocks_stripped` 输出可编程，不再用页码构造文本，旧 fake 的既有测试在迁移完成前仍通过。
- [x] T036（T016）实现本身只依赖 inference API 的 `OvisProtocolAdapter` 新模块；完成判定：图像顺序、prompt、generation、finish、`<think>` 剥离 golden（完整块/散落标签/静默空剥离）、`raw_text` 为剥离/裁剪前规范化原始输出、`think_blocks_stripped` 计数测试通过，新模块无 core/runtime/pdf/image 依赖；旧业务模块及其依赖可暂时保留到 T087，但新 adapter 不得调用旧模块。
- [ ] T037（T036）把 Ovis 页解析、bbox、Markdown 和图片代码复制到预定 scan 模块，暂不删除 adapter crate 的旧实现和旧导出；完成判定：新旧 parser/artifact 单元测试均通过，旧导出签名不变且 CLI 仍编译，`cargo check --workspace` 返回 0，旧代码明确限定由 T087 删除。
- [x] T038（T014、T019、T034、T036）实现静态兼容 binding registry 及其 `BindingVerifier` 端口实现（RBLK-9）；完成判定：正确三元组通过，runtime/sys/upstream/adapter/资产角色任一不符均返回 IncompatibleBinding（upstream 冻结值 `d81235049384534c167caea52b85a694f6103d14` 参与校验），端口实现供 prepare 与注入 `ModelManagerImpl` 共用。
- [x] T039（T018、T034）迁移系统 probe 和编译能力交集到 `ocrx-inference`；完成判定：Metal/CUDA/Vulkan 编译与硬件取交集，auto 优先级和显式设备错误测试通过。
- [x] T040（T030、T038、T039）实现 `InferenceServiceImpl::prepare` 的资产就绪和绑定路径；完成判定：只准备一次，缺资产走统一安装，离线/无效 hash/绑定失败不会创建 session。
- [x] T041（T040）实现 runtime 加载及 GPU 初始化失败回退 CPU；完成判定：显式不可用设备仍直接失败，加载阶段 GPU 失败回退并 warning，identity 和报告 backend 为实际 CPU，实际批深按回退重选规则（16→8）写入身份。
- [x] T042（T035、T036、T040）实现 session 的 encode → generate → decode 和多次 submit；完成判定：fake 连续请求共享一次加载，响应和累计 telemetry 不重复累加；`decode_echo` 端到端来自 `BackendOutput`（fake 编程回执 ≠ 请求变体时响应回执仍等于 backend 值，RBLK-1）；`step_profile()` 经 session 返回 backend 快照且 env 关闭为 `None`（RBLK-3）。
- [x] T043（T039、T041）迁移 doctor 支撑和可选 GPU 采样；完成判定：无 OCR 不启动采样，GPU→CPU 后停止并无意义 VRAM 数据，doctor 所需 DTO 完整。

### 阶段 E：结构化 checkpoint 与输出持久化

- [x] T044（T014、T015）在 output 新增 `ScanCompatibility` 和 route/native/OCR/批深/解码代次字段，暂时保留旧 `CompatibilityKey`；完成判定：新身份不用字符串前缀或参数位置判断语义，OCR 可保存完整 InferenceIdentity（含透传的 `batch_depth`/`decode_generation` 与 `display`——后者存储但标记报告专用）、prompt/parser/render 版本，旧 CLI 仍可使用旧类型编译。
- [x] T045（T044）实现 native/empty/OCR 匹配函数；完成判定：native/empty 忽略 dpi、批深及全部 OCR-only 改动，OCR 对 dpi、批深、解码代次和任一资产/runtime/adapter/backend/prompt/render 执行字段改动失效，`display` 展示元数据变化（RBLK-7）不影响复用，route 不交叉匹配。
- [x] T046（T045）实现 v6 checkpoint 确定性读写和状态字段，暂时保留旧 v5 写入入口；完成判定：complete/pending_ocr、output hash、truncated、degenerated、attempts（写入 1..=4、读取兼容更宽值如 5）、raw sidecar、`think_blocks_stripped`、warnings、audit 全部 round-trip，complete 写入发生在窗口全部页终态之后（机制 A），旧 CLI 的 v5 读写测试仍通过。
- [x] T047（T046）实现 v5 读取和安全迁移；完成判定：`complete=true` 且无 pending 后缀映射为 `status=complete`，`complete=false` 且有 `;pending-ocr=true` 映射为 `status=pending_ocr`，其他 false 状态为 reprocess，`complete=true` 与 pending 后缀的矛盾组合按无效/reprocess；兼容 native/empty 可复用，v5 OCR complete 必重处理，v5 `degenerated=true` 不复用，v5 `batch=N` 映射 `batch_depth` 且值不同即不兼容，v5 `decode=<gen>`/`think_blocks_stripped` 按保守策略处置且缺字段解析不失败，v4 不崩溃且不直接复用。
- [x] T048（T046）迁移原子页提交、native 提交、产物 hash 和 render audit；完成判定：complete checkpoint 最后写，引用图片缺失或篡改不可复用，无整页 PNG 的 native 行为不变。
- [x] T049（T046）迁移 pending 队列恢复并按 route 去重；完成判定：排序稳定、输入不兼容不恢复、同一页不会被 native 和 OCR 重复收集。
- [x] T050（T009）将 output 的取消/进度/哈希切换到 primitives；完成判定：无重复实现，现有 output 测试通过，对外 legacy re-export 可暂留并由 T086 统一删除。
- [x] T051（T044、T048）把 ScanService（含 P4.4 窗口编排、机制 A 原子提交与外部持模型窗口循环入口 `run_pages_with_model`，X1 共享槽使用）、loader、recognizer（含 `recognize_window_cancelled`）和 OCR 队列复制到 scan 新模块，暂不删除 output 旧实现和旧导出；完成判定：新 scan 不调用 output 旧扫描类型，旧导出签名不变且 CLI 仍编译，`cargo check --workspace` 返回 0，旧代码明确限定由 T088 删除。

### 阶段 F：扫描应用迁移

- [x] T052（T017、T051）建立 `ScanApplication/ScanRequest/ScanOutcome/ScanEvent`（单/多输入统一入口）；完成判定：库入口不打印、不退出进程、不安装 signal handler，CLI 可订阅事件并映射报告。
- [ ] T053（T052）迁移输入、页范围、数值参数和输出预检；完成判定：PDF 先于模型报错、路径保护、resume/overwrite 互斥、历史退出类别、多输入位置参数（选项可交错）、目录展开（排序 `*.pdf` 不递归）/canonical 去重/缺失或目录无 PDF 报用法错误、多输入 `--output-dir` 必填测试通过。
- [ ] T054（T052）迁移页面分析、auto/text/ocr 初始计划和路由诊断；完成判定：现有分类输入对应相同队列和 planned/actual/fallback 元数据。
- [x] T055（T045、T047、T052）实现扫描恢复计划（含两阶段 OCR 复用协议）；完成判定：v6/v5、完整复用、pending、重处理和 selected pages 限制测试通过；两阶段专项（RBLK-2/RBLK-4/RBLK-6/RBLK-8）——完全复用（全部候选通过预比较且无待解码页）不 prepare、不加载模型，**默认 `auto` 请求同样零加载**（预比较经 `static_identity()` 按 §2.3 冻结内存规则解析 auto 为具体档位后与 checkpoint 比对），多文档编排下后续文档同样适用、不因共享 session 存在而重处理（RBLK-8）；`auto` 解析结果变化（内存规则输出不同档位）时预比较失败、重处理；manifest 资产元数据/runtime 绑定/adapter 版本任一静态身份变化时预比较失败、重处理；存在待解码页时 prepare 后以 session 完整执行身份做终比较（仅限该文档，RBLK-8），**auto 在预比较后、prepare 时改选 profile 的专项测试：不一致候选全部重处理，同文档绝不混用两个 profile 的页面（RBLK-6）**；**display 区分性测试（RBLK-7）：仅展示元数据变化（display_name 文案/量化展示名）不触发重处理，执行字段变化才触发**；请求 GPU 但回退 CPU 所写的 checkpoint 在本次实际仍为 CPU 时即使请求设备不同也复用，实际执行形态变化（backend 或批深分层）时重处理；跨批深/跨解码代次不互恢复专项通过。
- [x] T056（T052、T055）迁移 native-first、text 失败、auto fallback 和 empty 提交；完成判定：native/empty 路径不调用 prepare，产物 golden 与 T004 一致。
- [x] T057（T056）实现最终 OCR 队列合并和按需 prepare；完成判定：队列为 0 时模型/设备 prepare 为 0（含默认 `auto` 的完全复用路径——仅执行只读 `static_identity()` 预比较，零资产校验/零安装/零加载，RBLK-4），非 0 时只 prepare 一次。
- [x] T058（T052）迁移 PDFium 与 worker renderer、PdfiumOwner 线程所有权、预取协议、扰动渲染转发（P3）与跨文档 `SwitchDocument` 协议（清预取缓存、重置 `next_prefetch`、文档级 Err 值不中毒 owner）、像素缩放和 render audit；完成判定：DPI、分段、尺寸校验、2880² 预算、scale ratio、预取与故障注入测试保持，PDFium 入口安全不变式不回退，SwitchDocument 既有单测（同一线程、预取重置、Err 值跳过、poison fail-loud）随迁通过。
- [ ] T059（T006、T052）在 `ocr/prompt.rs` 写入完整 `ocr-markdown-v1` prompt 和构造器；完成判定：T006 字节 golden 通过，扫描生成的 TaskRequest prompt 与冻结文本一致。
- [ ] T060（T037、T059）迁移 Ovis 文本到 PageDocument 的解析；完成判定：公式、表格、bbox、CRLF、重复尾部、空输出、EOS/stop/max_tokens golden 全通过。
- [ ] T061（T037、T060）迁移 bbox 裁剪、整页 PNG、图片替换和链接校验；完成判定：文件命名、数量、路径逃逸拒绝和缺失链接错误与现状一致。
- [ ] T062（T017、T058、T059、T060、T061）实现单页 OCR 流程；完成判定：fake 收到无页码任务，scan 用局部页码生成产物，`raw_text` 写 `page-NNNN.raw`、`think_blocks_stripped` 进页统计，截断和解析警告正确映射。
- [x] T063（T062）实现多页窗口化编排（实际批深身份填充/恢复页排水、attempt 1 批处理、批级失败批深回退 1、机制 A 窗口原子提交、退化/纯截断页走冻结阶梯的变体重试：2048 网格 → penalty 采样链 → 切栏分解/按栏序合并/保守合并/门控，切栏页产物按 §2.3 冻结合并规则合成——raw 连接、think 求和、warnings 顺序，golden 无歧义）、跨文档编排（文档 FIFO、共享 session、`SwitchDocument`、逐文档独立窗口与机制 A、取消=整调用、退出码聚合、`scan-summary.json` 与 `<stem>-<hash8>` 输出目录）、失败继续、取消停止和进度槽位→页码映射；完成判定：prepare/load 每调用一次（多文档共享同一 session），窗口提交与页数关系准确（请求按类别计数断言），取消的在飞窗口零提交且恢复整窗重扫，后续已提交页保留，同批深自一致，多文档编排与逐个调用产物一致且 `scan-summary`/退出码聚合正确；**共享 session 的 auto 漂移专项（RBLK-6/RBLK-8，仅限有待解码页的后续文档）：该文档候选页与共享 session 完整执行身份做全执行字段比较后，不一致候选全部重处理，绝不跨 profile 混用，其余文档不受影响；前文档已建共享 session + 后文档完全复用且存储实际身份不同（RBLK-8 专项）：后文档零新解码复用、不借用共享 session 身份，逐文档产物与其单跑一致**。
- [x] T064（T045、T062、T063）把实际 InferenceIdentity 写入 OCR v6 checkpoint（`batch_depth`/`decode_generation` 只从身份透传，扫描侧无硬编码副本）；完成判定：GPU 回退、模型资产、prompt/parser/render/generation、批深、解码代次任一变化均按规则处理。
- [x] T065（T048、T049、T056、T063）实现跨路线产物收集和 document.md 汇总；完成判定：只收集已验证页，排序/去重、空结果 stale 清理和 keep-pages 行为与基线一致。
- [x] T066（T052、T065）迁移 routing sidecar、ScanOutcome 和 result 所需字段；完成判定：模型 identity（含经 `display` 公共通道取得的 name/family/quantization——scan 源码无 `ocrx_models` 依赖、无展示映射副本，RBLK-5）、实际 backend、页计数（含 `think_blocks_stripped_total` 聚合与 `degenerated`）、warnings、diagnostics、images 和 routing 数据完整，多文档编排另含 `scan-summary.json` 字段与状态映射。
- [x] T067（T052、T063）迁移扫描遥测与可选 GPU 报告映射及 T1 插桩接线（StepProfile 逐文档 `delta_from` 增量、CommitRecorder 共享 sink）；完成判定：StepProfile 一律经 `InferenceSession::step_profile()` 公共查询取得，scan 源码无 `ocrx_runtime::StepProfile` 引用（RBLK-3）；累计值不双算，报告写失败只 warning，实际 CPU 回退不生成误导 GPU 数据，`step-profile.json`/`commit-profile.json` 每文档一份、黄金串不变、取消/失败零写盘、disabled no-op。
- [x] T068（T052、T057）实现模型准备失败后的 pending 状态和部分结果；完成判定：native/empty 已提交结果保留，OCR 页可续扫，错误和结果输出与原分支一致。

### 阶段 G：CLI 收缩与生产组合

- [x] T069（T030、T038、T039、T042、T052）实现 `compose.rs` 生产组合根；完成判定：只注册 Ovis，scan 接收 API trait，model/doctor 使用同一 manager，无第二套注册数据；组合顺序为 catalog → probe/设备/runtime factory → 静态 registry → `BindingVerifier` 实现 → 注入 verifier 构造 `ModelManagerImpl` → `InferenceServiceImpl` → `ScanApplication`（§6.1，RBLK-9）。
- [ ] T070（T069）拆分参数、配置和交互模块；完成判定：现有参数取值、默认值、配置优先级、offline/non-interactive 和下载确认测试通过。
- [x] T071（T069）将 `model list/info/recommend` 切换到 ModelManager；完成判定：无 PDF/scan 对象，输出与 T003 golden 一致。
- [x] T072（T069、T070）将 `model install/verify/remove` 切换到 ModelManager；完成判定：CLI 无 URL/hash/cache 拼接，交互和错误码 golden 通过；`verify` 端到端专项（RBLK-9）：资产文件全部正确但注入端口的 registry 身份（runtime/sys/upstream/adapter 任一）不匹配时 verify 失败且错误类别为 IncompatibleBinding，全部匹配时通过；架构检查断言无 models → inference 依赖。
- [ ] T073（T043、T069）将 doctor 切换到 DeviceManager/ModelManager DTO；完成判定：文本、schema 2 JSON、缓存 escaping 和 skill_installed 合并与基线一致，旧 scan 经兼容层仍编译且 `cargo check --workspace` 返回 0。
- [ ] T074（T052、T069）将 scan 命令切换到 `ScanApplication::run`（单/多输入统一入口，编排判定随迁）；完成判定：CLI 不再包含 run_scan/run_scan_multi orchestrator，fake 与本机 fixture 结果符合基线。
- [ ] T075（T066、T074）实现扫描事件到 Reporter、result schema 3 和退出码映射；完成判定：stdout/stderr、字段顺序、错误对象、partial/cancel 代码与 T003/T004 一致，逐字段 golden 明确验证 runtime=`llama.cpp@0.1.161`、adapter_version=`0.1.0`、prompt_schema_version=数值 1、经 `display` 通道的四档 name/family/quantization（四 profile 各一条 golden，RBLK-5）、`pages.degenerated` 与 `pages.think_blocks_stripped_total` 计数及多输入编排的 `scan-summary.json`/聚合退出码。
- [ ] T076（T074）保留 Ctrl+C 两次信号策略并连接扫描取消；完成判定：handler 在慢阶段前安装，token 类型全链一致，第二次仍强制 130。
- [ ] T077（T069、T074）删除 CLI 对 runtime/models/ovis/pdf/image 的直接依赖和旧辅助类型（含 `run_scan_multi`/`scan_document`/`ModelSlot` 等旧编排代码）；完成判定：Cargo manifest 与源码均无旧 import，CLI 仅保留组合/展示/交互/skill 职责，`cargo check --workspace` 证明后续兼容层已没有 CLI 消费方。
- [ ] T078（T071、T072、T073、T074、T075、T076、T077）运行全部 CLI 子进程测试；完成判定：doctor、model、scan 参数、native resume、输出保护、JSON、退出码与多文档编排（`scan-summary.json`、退出码聚合、编排 vs 逐个调用一致）测试全部通过。

### 阶段 H：质量评估独立化与脚本适配

- [ ] T079（T008、T009）将质量 manifest 类型、加载和输入 hash 校验复制到 `ocrx-eval/manifest.rs`，暂留 PDF 过渡 re-export；完成判定：原合法/非法 TSV 用例结果一致，新实现归 eval 所有，旧导出在 T083 前保持编译兼容。
- [ ] T080（T079）将 NED、block、bbox、阅读顺序和路由指标复制到 `ocrx-eval/metrics.rs`，暂留 PDF 过渡 re-export；完成判定：同一组数值 fixture 的每个浮点/计数结果与迁移前一致。
- [ ] T081（T079、T080）将 native/performance/release 门禁和报告复制到 eval，暂留 PDF 过渡 re-export；完成判定：阈值、失败列表和 JSON golden 完全不变。
- [ ] T082（T014、T052、T079）实现评估执行身份、两种公开执行入口和“同参重扫逐字节比对”原语（§3.5 第 10 条/§8.12）；完成判定：端到端入口调用 ScanApplication，专项入口调用 InferenceService，均记录 set/truth/inference/prompt/render 版本；原语以 fake 双跑通过（同输入同参逐字节一致、注入差异时能检出）。
- [ ] T083（T079、T080、T081）调整 Rust 调用方和测试到 eval 新路径并删除 PDF 过渡 re-export；完成判定：无 `ocrx_pdf::Quality*` 等旧引用，未通过删除质量测试解决编译，`cargo check --workspace` 返回 0。
- [ ] T084（T082）核对并适配 Python 工具的公开 CLI/文件契约；完成判定：现有 regression manifest/truth/prediction/report schema 和路径不变，无脚本复制资产或设备逻辑。
- [ ] T085（T084）运行 Python 单元测试和固定小样评分对比；完成判定：迁移前后 score/quality/native 报告数值一致，未修改真值或阈值。

### 阶段 I：清理、架构检查与全面回归

- [ ] T086（T019、T030、T050、T077、T083）删除 core 中 ModelId、RUNTIME_COMPATIBILITY、重复取消/哈希及无用 re-export，并删除 models 的旧 `community_*` 过渡委托和旧 `recommend_device` 自含过渡实现；完成判定：全 workspace 无旧类型和旧函数引用，页面领域 golden 仍通过。
- [ ] T087（T031、T037、T060、T061、T077）删除 `ocrx-ovisocr2` 旧业务函数和依赖，并删除 runtime 的旧 Runtime/fake/请求响应兼容包装及其中暂存的 prompt；完成判定：adapter crate 只含协议 adapter，runtime 只含通用 backend，依赖和源码检查证明无 core/pdf/扫描 prompt/page。
- [ ] T088（T051、T052、T077）删除 output 中已迁移的扫描服务、识别抽象、旧 CompatibilityKey 和旧 v5 写入入口；完成判定：无重复实现，所有调用只走 scan 应用和 v6 持久化，`cargo check --workspace` 返回 0。
- [ ] T089（T077、T087、T088）实现 `validate-architecture-boundaries.py` 的 Cargo 白名单和源码禁用检查；完成判定：合法树返回 0，临时违规 fixture 能被检出，脚本只用标准库；禁用类型名显式包含 `ocrx_runtime::StepProfile` 在 `ocrx-scan` 中的引用（RBLK-3，StepProfile 只经公共 API DTO 流动）。
- [ ] T090（T089）把架构检查加入本机 bootstrap，不加入网络/模型/GPU 前置；完成判定：干净本机按脚本顺序可执行，外部条件缺失不会阻断 fake 测试。
- [ ] T091（T086、T087、T088）清理 Cargo 依赖、feature 转发和 lockfile；完成判定：无未使用直接依赖，vulkan/cuda 传递有效，`cargo metadata --locked` 成功。
- [ ] T092（T086、T087、T088）全量搜索并更新旧行为注释和文档字符串；完成判定：不再出现 runtime 持有 OCR prompt、adapter 写页产物、output 拥有扫描服务、请求含页码等过时描述。
- [ ] T093（T078、T085）运行 `cargo fmt --all -- --check` 和 `cargo build --workspace --locked`；完成判定：格式和完整构建均返回 0。
- [ ] T094（T093）运行 `cargo test --workspace --locked`；完成判定：全部非 ignore 测试通过，无真实模型下载，无新增失败被跳过。
- [ ] T095（T093）运行 `cargo clippy --workspace --all-targets --locked -- -D warnings`；完成判定：无 warning/error，未通过全局放宽 lint 掩盖问题。
- [ ] T096（T085、T089、T090）运行 Python 单元测试、架构检查和 release 脚本测试；完成判定：三个命令均返回 0，测试未访问外网。
- [ ] T097（T078、T094）运行 doctor/model/version 本机命令并比较 golden；完成判定：T003 所有适用输出和退出码一致，JSON 可解析且 stdout 行数正确。
- [ ] T098（T004、T068、T078、T094）执行 native-only/empty 的本机端到端对比；完成判定：模型准备和加载为 0，页产物、document、result 和续扫与基线一致。
- [ ] T099（T004、T062、T063、T064、T094）执行 fake OCR 多页端到端对比；完成判定：一次 prepare、窗口化 submit（attempt 1 批处理 + 批深 1 回退/阶梯变体重试——2048 网格、penalty 链、切栏合并与门控、纯截断重试——覆盖全部页），页产物（含 raw sidecar 与 think 计数）、路由、失败继续、取消、机制 A 提交和报告符合基线；多文档编排端到端（fake）另验 `scan-summary.json`、退出码聚合、逐文档独立 resume、编排 vs 逐个调用产物一致与共享 session 一次加载。
- [ ] T100（T047、T055、T064、T094）执行 checkpoint 专项验收；完成判定：v6 全身份匹配、两阶段恢复专项（完全复用零加载含默认 `auto`、prepare 后全执行字段终比较且仅限待解码文档，RBLK-2/RBLK-4/RBLK-8）、**auto 改选混用防护（RBLK-6）：预比较与 prepare 分别解析出不同 profile 时候选全部重处理、需要解码的后续文档静态结果不同于共享 session 时同样全部重处理，断言同文档不出现跨 profile 混用页面**、**RBLK-7 区分性测试：display 变化不触发重处理、执行字段变化触发；RBLK-8 专项：前文档已建共享 session 时完全复用的后文档零新解码复用且编排/单跑产物一致**、`auto` 解析结果变化与静态身份（资产元数据/runtime/adapter）变化时预比较失败重处理、v5 OCR 重做、pending 恢复、跨 prompt/runtime/model/批深/解码代次不复用、v5 `degenerated` 不复用、`think_blocks_stripped` round-trip、取消窗口零提交后整窗重扫和产物篡改拒绝均有实际测试证据；另行保留 GPU→CPU 回退 checkpoint 的测试记录，证明请求设备不同但实际 CPU 相同可复用、实际 backend 不同必须重处理，不得将该语义变化当回归修回 v5 的请求设备匹配。
- [ ] T101（T041、T064、T067、T094）执行设备与身份专项验收；完成判定：显式设备失败、auto 选择、GPU 初始化回退、实际 backend 报告和 checkpoint 身份均通过 fake/受控测试。
- [ ] T102（T084、T085、T096）执行质量模块专项验收；完成判定：两种评估入口可调用，报告身份完整，“同参重扫逐字节比对”原语执行通过，固定样例数值与历史一致。
- [ ] T103（T097、T098、T099、T100、T101、T102）运行 `bash scripts/bootstrap-check.sh`；完成判定：脚本完整返回 0，过程未触发模型下载或外部服务。
- [ ] T104（T103）检查最终 `git status` 和变更清单；完成判定：只有方案内文件和必要 lockfile 变化，无测试临时文件、日志、下载物或用户既有文件被误改。
- [ ] T105（T103、T104）逐项核对第 11 节完成定义并记录可选真实验证是否执行；完成判定：所有本机门禁有通过证据，未执行的真实模型/GPU 项仅注明环境原因且未被列为阻塞。

### 非门禁后续任务（不属于本次重构完成条件）

- [ ] T106（T105；仅在已有真实 Ovis 资产和 PDFium 时可选）运行真实 Ovis 故障注入和多页冒烟；完成判定：记录加载、生成、取消和续扫结果；环境不足时记录未执行原因，不反向阻塞 T105。
- [ ] T107（T105；仅在已有对应硬件时可选）分别观察 CPU/GPU、资源和 GPU→CPU 回退；完成判定：形成本机观察记录，不把单一硬件结果宣传为通用支持证明。
- [ ] T108（T105；后续独立变更，不在本次实施）另行评估并接入新 OCR/VLM；完成判定：先冻结新模型资产/runtime/adapter 兼容性和质量范围，再复用本方案扩展点；§5.8 缺口评估（规范中间层/网格能力/AWQ/per-adapter prompt 身份）作为该立项的设计输入重新裁决，不构成对 §5.1–§5.5 v2 冻结契约的预先修改；本任务不是 Qwen 兼容承诺，也不是当前重构验收项。
