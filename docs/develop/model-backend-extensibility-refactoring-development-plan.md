# OCR-CLI 模型后端可扩展架构重构详细开发方案

**文档状态：** 待实施（修订 v2，2026-11-03——按已合并的吞吐优化 P3/P4 与复读退化方案更新迁移输入；原 2026-10-31 范围协调注记的修订要求已并入正文）  
**迁移输入基线（2026-11-03，main `a02d459`）：** 吞吐优化方案（`docs/develop/scan-throughput-optimization-development-plan.md`）的 P3（PdfiumOwner 渲染/解码重叠）与 P4（多页合批：P4.1 推理线程 actor、P4.3 窗口解码、P4.4 窗口化扫描编排）已合并进 main；复读退化方案（`docs/develop/repetition-degeneration-detection-recovery-development-plan.md`）已先行合并。本计划按其头部协调注记完成两处冻结范围修订：§2.1 的"多页串行复用"保留项已改为"以已具备合批能力的 `ocrx-runtime` 为迁移输入（批深、窗口协议以 P4 落地形态为准）"；§2.2 的"不引入并发 OCR"已限定为"本重构不新增扫描层并发语义，但须迁移/保留 runtime 已落地的合批能力"。窗口协议冻结值见 `docs/develop/batched-inference-2026-11-02.md`（默认批深 N=4、机制 A 窗口原子提交、`;batch=N` 兼容键、可选窗口方法最小接口面）。本重构以该落地形态为迁移输入，不回退、不重设计窗口协议。  
**实施范围：** 仅重构现有 OvisOCR2 路径，不接入第二模型  
**目标读者：** 直接承担本仓库 Rust、CLI、模型资产、扫描流程和质量回归改造的开发与测试人员  
**方案性质：** 本文独立给出目标架构、业务规则、代码组织、契约、迁移方式、修改清单、验收方法和执行任务；实施时不需要借助其他设计文档才能理解本方案

---

## 1. 背景、问题与重构结论

### 1.1 当前系统实际怎样工作

当前工作区包含 `ocrx-cli`、`ocrx-core`、`ocrx-models`、`ocrx-runtime`、`ocrx-ovisocr2`、`ocrx-pdf`、`ocrx-output` 和 `ocrx-skill`。与本次重构有关的实际调用链是：

```text
ocrx-cli::run_scan
  ├─ 直接读取 PDF、分类页面、计算 native/empty/OCR 队列
  ├─ 直接选择 OvisOCR2 profile 和 CPU/GPU
  ├─ 直接生成 OvisOCR2 manifest、解析缓存路径、下载并校验 model/mmproj
  ├─ 直接创建 LlamaMtmdRuntime（内含 P4.1 推理线程 actor 与 P4.3 窗口解码）
  ├─ 通过 ocrx-output::ScanService 按窗口（P4.4，默认批深 4）渲染和识别
  │    └─ CLI 内的 OvisRecognizer
  │         └─ ocrx-ovisocr2 调用 Runtime（含 generate_window）、解析文本、裁剪 bbox、写图片与 raw sidecar
  ├─ 通过 CLI 内 pdfium_owner.rs 实现 P3 渲染/解码重叠（PdfiumOwner 线程 + 预取）
  ├─ 直接处理 checkpoint、续扫、路由 sidecar、报告和退出码
  └─ 直接合并 Markdown 并生成 result.json
```

当前各位置的关键职责和耦合如下：

| 当前位置 | 当前实际职责 | 必须解决的问题 |
|---|---|---|
| `src/ocrx-cli/src/main.rs` | 一个约 5400 行的主文件同时承担参数解析、模型管理、设备诊断、PDF 计划、页面处理、runtime 生命周期、续扫、遥测和报告（另有 `pdfium_owner.rs` 约 770 行承载 P3 渲染/解码重叠） | 换模型或 runtime 会改动扫描业务；模型管理命令与 scan 重复实现资产逻辑；无法独立复用扫描应用 |
| `src/ocrx-core/src/lib.rs` | PDF/页面领域类型、取消、哈希、`ModelId`、llama.cpp 兼容常量、窗口批深/批级错误常量（`WINDOW_DEPTH`/`BATCH_ERROR_PREFIX`）、错误码混放 | 通用技术原语、扫描领域和推理身份没有边界；`ModelId` 是只接受 `ovisocr2` 的枚举 |
| `src/ocrx-models/src/lib.rs` | Ovis 固定哈希/URL、两个固定资产、缓存、下载、安装、校验、设备/profile 推荐 | manifest 固定为 `model_sha256 + mmproj_sha256`，无法表达具名角色、可选组件和通用绑定；设备和模型管理混在一起 |
| `src/ocrx-runtime/src/lib.rs` | `Runtime` trait（含 P4.3 可选窗口方法 `generate_window`）、设备枚举、fake/`LoopFakeRuntime`、遥测 | `InferenceRequest` 含扫描页码却没有业务 prompt；接口不是通用模型任务契约 |
| `src/ocrx-runtime/src/llama_mtmd.rs` | llama.cpp/MTMD 加载、P4.1 推理线程 actor、图像编码、逐 token 与多槽位窗口生成 | OCR prompt 被硬编码在 backend 内，backend 因此知道扫描业务意图 |
| `src/ocrx-ovisocr2/src/lib.rs` | 调 runtime（含窗口提交转发）、解释 Ovis 文本、解析公式/表格/bbox、截断、复读退化解析、裁剪 PNG、替换图片链接、生成页 JSON | 模型协议、扫描业务解析和文件输出耦合；该 crate 直接依赖 runtime 和 PDF |
| `src/ocrx-output/src/lib.rs` | 输出、checkpoint、页面生命周期、扫描服务、P4.4 窗口编排与机制 A 原子提交、复读退化重试状态机、渲染/识别抽象、OCR 队列 | output crate 反向拥有扫描编排（含窗口编排与提交原子性）；兼容身份是字符串拼接，缺少资产、runtime、adapter、prompt 的结构化身份 |
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
- 保留 native-first、auto 模式 native 校验失败转 OCR、text 模式失败、empty 页、按需加载模型、单 job 单次加载、GPU 初始化失败回退 CPU、取消、失败继续、checkpoint 续扫和原子提交语义。
- 以已具备合批能力的 `ocrx-runtime` 为迁移输入，原样保留 P4 落地形态的多页窗口推理语义：固定批深 `WINDOW_DEPTH = 4`、按页序填充窗口、不做动态回填、窗口上下文按 Σ(各页图像 token + 提示 + 输出上限) 动态计算、批级失败整窗批深回退 1 重提、机制 A 窗口原子提交（取消的在飞窗口零提交、恢复整窗重扫）、相同（输入、批组合、批深）下输出逐字节可复现。多请求能力收敛为推理 API 的可选窗口方法（最小接口面），不重设计窗口协议。
- 保留复读退化方案已落地的行为：`repetition_loop` 终止原因、attempts 1..=3 状态机、attempt 1 随窗口批处理而退化页以扰动输入单独按批深 1 重试、`degenerated` checkpoint 字段与合并文档不完整标记、`page-NNNN.raw` 原始输出 sidecar。
- 保留 P3 渲染/解码重叠：PdfiumOwner 线程所有权、预取协议和扰动渲染转发随渲染路径迁入扫描应用，语义不变。
- 保持 `document.md`、页 Markdown/JSON、图片命名、`result.json` 的对外 schema、CLI 参数、stdout/stderr 分工和退出码兼容。
- 将 checkpoint 兼容身份升级为结构化、可审计身份；对旧 v5 checkpoint 采取安全且明确的迁移行为。
- 默认开发测试完全可使用 fake runtime、受控本地资产和仓库内 fixture 完成，不下载模型、不访问外网、不要求 GPU。

### 2.2 明确不做的事项

- 不增加 Qwen3.8 或任何第二模型，不验证 Qwen 的 GGUF、mmproj、chat template 或 llama.cpp 兼容性。
- 不升级当前 `llama-cpp-2 = 0.1.152`、`llama-cpp-sys-2 = 0.1.153` 和上游提交 `9e3b928`。
- 不实现动态模型注册、第三方插件加载、任意脚本执行或模型热插拔。
- 不改变 PDF 分类算法、native 文本校验算法、OCR 质量阈值和评分公式。
- 本重构不新增扫描层并发语义：不引入分布式任务、异步服务进程或新的网络服务；runtime 已落地的窗口合批（P4）按原语义迁移保留，不重设计、不改变默认批深，也不新增用户批深旋钮。
- 不重写既有 Python 评分器，不伪造真值，不重新生成历史预测来掩盖行为差异。
- 不把真实模型下载、真实 GPU、外部 CI、外部模型评测或 Qwen 冒烟设为本机开发和测试的前置门禁。

### 2.3 必须冻结的 OvisOCR2 行为

- 模型 ID 为 `ovisocr2`，profile 为 `compact/balanced/quality/reference`，`auto` 的内存推荐规则和设备优先级 `metal > cuda > vulkan > cpu` 不变。
- 固定社区 revision 为 `8a22290a0c42dbcf84739d6d4c2763f877494ae0`；现有四个主模型和共享 mmproj 的 SHA-256、字节数、URL 逐项迁移，不能在重构时重新从网络“发现”元数据。
- 现有缓存布局继续使用：主模型位于 `<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf`，共享视觉资产位于 `<cache_root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf`。重构不能迫使用户重复下载已校验资产。
- 默认输出 token 上限 16384（参数范围 128–16384，复读退化方案 D5.2 冻结值），图像 token 默认 1024，context 默认 20480，DPI 默认 300，模型像素预算 `2880 × 2880`，贪心解码、thinking 关闭。
- 多页窗口推理参数冻结为 P4 落地值：默认批深 `WINDOW_DEPTH = 4`（`batched-inference-2026-11-02.md` §④ 终版冻结），批深参与 checkpoint 兼容身份，跨批深不互恢复；单流路径沿用 `context_size`（默认 20480），窗口上下文独立创建。
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
- `InferenceSession` 在一次扫描 job 中只创建一次，连续处理多个 `TaskRequest`；每次请求都不携带 PDF 或页面上下文。窗口合批是 session 的可选批量入口：`submit_window` 一次提交一批请求，默认实现逐个串行 `submit`（数值等价单流），真实 backend 覆盖为多槽位连续合批；多请求能力收敛为 runtime 内部实现细节 + 最小接口面（`batched-inference-2026-11-02.md` §⑤）。
- `ScanApplication` 持有的是 `dyn InferenceService` 或泛型 API trait，不知道 session 内是 llama.cpp 还是 fake。
- `ProtocolAdapter` 只负责文本/图像如何进入模型协议以及通用响应如何解码；`OcrPageParser` 只负责把通用文本变成扫描领域页面，二者不是同一个对象。

---

## 4. 重构后的 Cargo crate 与代码组织

### 4.1 目标目录树

```text
src/
├── ocrx-cli/
│   └── src/
│       ├── main.rs                 # 仅 main、命令分发和组合根调用
│       ├── args.rs                 # CliCommand、ScanOptions、参数校验
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
│       ├── task.rs                 # ImageInput/TaskRequest/TaskResponse/Usage；窗口批深常量
│       ├── service.rs              # InferenceService/InferenceSession（含可选 submit_window）
│       ├── management.rs           # ModelManager/DeviceManager 及 DTO
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
| `ocrx-runtime` | backend 执行（含窗口合批内部实现）和 fake | 模型安装、业务 prompt、业务文本解析 |
| `ocrx-ovisocr2` | Ovis 模型协议适配 | OCR prompt 内容、PageDocument、bbox 裁剪、Markdown 链接 |
| `ocrx-inference` | 静态绑定、设备管理、session 组合和遥测 | 扫描编排、模型评分 |
| `ocrx-scan` | 完整扫描应用、窗口编排、OCR prompt 和业务解析 | 具体资产路径、llama.cpp 类型、模型质量评分 |
| `ocrx-pdf` | PDF 事实获取和图像处理 | 质量真值、checkpoint、推理 |
| `ocrx-output` | 持久化、checkpoint 和通用输出 | 页面计划、识别、模型加载 |
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
    pub wrapper_version: String,     // 固定为 "0.1.152"
    pub sys_version: String,         // 固定为 "0.1.153"
    pub upstream_revision: String,   // 固定为 "9e3b928"
}

pub struct AdapterIdentity {
    pub adapter: AdapterId,          // 当前为 "ovisocr2"
    pub version: String,             // 当前为 "0.1.0"
}

pub struct InferenceIdentity {
    pub model: ModelIdentity,
    pub runtime: RuntimeIdentity,
    pub adapter: AdapterIdentity,
    pub backend: BackendId,           // 实际执行的 backend，而非仅请求值
}
```

`InferenceIdentity` 由推理平台在 prepare 成功后返回。扫描应用不得手工伪造 runtime、adapter 或资产哈希；它只追加自己的 prompt、渲染和 PDF 路由身份。

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

pub struct TaskRequest {
    pub prompt: String,
    pub images: Vec<ImageInput>,
    pub generation: GenerationParams,
}

pub enum FinishReason { Eos, StopToken, MaxTokens, Invalid, RepetitionLoop }

pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: u64,
}

pub struct TaskResponse {
    pub text: String,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
    pub warnings: Vec<String>,
}

pub trait InferenceSession {
    fn identity(&self) -> &InferenceIdentity;
    fn telemetry(&self) -> RuntimeTelemetry;
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

- `TaskRequest` 没有 `page`、`output_root`、`PdfMode`、`PageClassification`、checkpoint 或 Markdown 字段。
- 当前扫描每页只传一张图，窗口是多个单图请求的批量提交；图像尺寸和 RGB8 长度在 API 边界校验。
- 取消不是正常 `FinishReason`。backend 遇到取消统一返回 `InferenceError::Cancelled`，扫描应用据此停止取新页；被取消的在飞窗口零提交（机制 A）。
- `finish_reason = max_tokens` 只表示生成预算耗尽；扫描应用把它映射为页 `Termination::MaxTokens` 和 `truncated` warning。`repetition_loop` 由扫描应用进入复读退化处理（attempts 状态机、扰动重试、`degenerated` 标记），不视为普通质量失败。
- `FinishReason` 枚举与现有 `ocrx-core::Termination` 全集一致（含 `Invalid`、`RepetitionLoop`），不得在迁移中收窄。
- 窗口确定性契约沿用 P4 冻结语义：相同（输入、批组合、批深）下逐字节可复现，不同批组合允许分叉；扫描应用的窗口填充策略（固定批深、按页序、不动态回填）保证断点前后批组合一致，批深进入 checkpoint 兼容身份。
- telemetry 仍是 session 累计值（窗口内全序列聚合累加），扫描报告每页结束后读取当前累计值，不能重复累加。

### 5.4 adapter 与 backend 的内部扩展契约

```rust
pub enum ImagePlacement { BeforePrompt }

pub struct EncodedTask {
    pub prompt: String,
    pub images: Vec<ImageInput>,
    pub image_placement: ImagePlacement,
    pub generation: GenerationParams,
}

pub struct BackendOutput {
    pub text: String,
    pub native_finish_reason: NativeFinishReason,
    pub usage: TokenUsage,
}

pub trait ProtocolAdapter {
    fn identity(&self) -> &AdapterIdentity;
    fn encode(&self, request: TaskRequest) -> Result<EncodedTask, InferenceError>;
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

Ovis adapter 只声明“图像位于 prompt 前，使用 runtime 默认多模态图像 marker，保持生成参数，解码通用结束原因”。llama.cpp backend 在图像位置调用 `mtmd_default_marker()`，拼接 marker 与 adapter 已给出的 prompt。这样既保留当前精确输入顺序，又不让 adapter 依赖 llama.cpp 类型。P4 已落地的推理线程 actor、多序列槽位、逐序列环检测与窗口上下文容量计算保留为 backend 内部实现，不暴露给 adapter 或扫描应用。

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

pub trait InferenceService {
    fn prepare(
        &mut self,
        request: PrepareRequest,
        interaction: &mut dyn InstallInteraction,
    ) -> Result<PreparedInference, InferenceError>;
}
```

`prepare` 的顺序固定为：解析 catalog → 选择 profile → 校验绑定 → 选择设备 → 检查资产 → 按需安装并复检 → 创建 runtime → 必要时 GPU 回退 CPU → 创建 adapter/session → 返回实际身份。任何一步失败都不能留下“看似可用”的 session。

`InstallInteraction` 是推理 API 的通用交互端口，只表达“需要下载多少字节、来源和许可证是什么、是否允许继续”。CLI 实现终端询问；`--non-interactive` 和离线策略由请求明确控制。扫描应用和模型 install 命令复用同一准备/安装实现，不再各自拼 URL 和 hash。

### 5.6 模型与设备管理 API

`ModelManager` 至少提供：

- `list()`：列出 catalog 中模型，不触碰 PDF。
- `info(model)`：列出所有 profile、安装状态和来源 revision。
- `recommend(model, resources)`：返回 profile 推荐，不隐式安装。
- `install_plan(model, profile)`：返回资产、字节数、许可证和当前状态，供 CLI 确认。
- `install(model, profile, policy, interaction)`：安装、校验、写 receipt；重复安装已验证资产时无网络且成功。
- `verify(model, profile)`：检查所有必需角色、大小、SHA-256 和静态 runtime/adapter 绑定。
- `remove(model, profile)`：只删除目标 profile 的私有资产；共享资产只有在同 revision 不再被其他 profile 使用时才删除。

`DeviceManager` 至少提供：

- `snapshot()`：返回内存、硬件可用性和编译能力。
- `select(requested)`：显式设备不可用时报错，不静默改选；auto 按冻结优先级。
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

扫描错误至少区分 PDF 输入、PDF 路由、native 提取、渲染、模型准备、推理、响应解析、产物完整性、输出和取消。CLI 统一把结构化错误映射为现有退出码：用法 2、输入 3、模型 4、下载 5、runtime 6、解析 7、完整性 8、输出 9、部分完成 10、取消 130。现有个别命令分支的历史退出码如有差异，以既有 CLI golden 为准保留，不能在重构中顺便“修正”为另一套行为。

错误消息可以补充结构化 cause，但 stdout/stderr 通道、JSON 外层 schema 和敏感信息脱敏规则不能改变。密码、prompt 内容、token、授权信息和代理凭据不写入报告。

---

## 6. 重构后的业务逻辑

### 6.1 应用启动与组合

CLI 启动后只创建一个生产组合上下文：

1. 创建内置 Ovis manifest 集合和 `ModelCatalog`。
2. 创建 HTTPS transport、资产存储和 `ModelManagerImpl`。
3. 创建系统 probe、`DeviceManagerImpl` 和 llama.cpp runtime factory。
4. 注册 Ovis adapter 以及 `ovisocr2 + llama.cpp + 固定版本` 的兼容 binding；其中 `llama.cpp` 是本次新定义的内部 RuntimeId，不是对现有 result.json 展示字符串的照抄。
5. 创建 `InferenceServiceImpl`。
6. 用 `dyn InferenceService`、PDF 基础设施和输出存储创建 `ScanApplication`。
7. `scan` 调用扫描应用；`model` 调用模型管理；`doctor` 调用设备和模型管理；`skill` 继续走原模块。

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
3. 已提交且身份和产物哈希都匹配的页进入复用集合，不重复渲染或识别。
4. 未完成且属于 pending OCR 的页回到 OCR 队列；其他不匹配或损坏状态进入重处理。
5. native/empty 和 OCR 使用不同 route 身份，避免同一页被两种恢复逻辑重复收集。

#### 第四步：执行 native 与 empty

1. native candidate 先提取并校验。
2. auto 模式校验失败时写 pending OCR checkpoint 并加入 fallback；text 模式同类失败直接终止。
3. native 成功页生成原有 `PdfPageDocument` 和 Markdown，原子提交，不生成伪造的整页 PNG。
4. empty 页生成空结果并原子提交。
5. 此阶段不读取模型权重、不创建 runtime。

#### 第五步：确定最终 OCR 队列并按需准备推理

1. 合并初始 OCR、native fallback 和恢复的 pending OCR，再次排序去重并排除已复用页。
2. 最终队列为空时，跳过模型 profile 实际解析、资产校验、安装和 runtime 创建，直接汇总。
3. 队列非空时调用一次 `InferenceService::prepare`。
4. 准备失败时，为未处理 OCR 页保留可恢复的 pending 状态和 `ocr_required_but_unavailable` 诊断，已提交 native/empty 页不丢失，按现有模型错误路径输出部分结果。
5. 准备成功后保存 `InferenceIdentity` 和 session；同一 session 处理所有 OCR 页。

#### 第六步：窗口化 OCR

1. 若已取消，停止取新页。
2. 按冻结策略组装窗口：固定批深 `WINDOW_DEPTH = 4`、按页序填充、不做动态回填（窗口不满按实际页数运行）；恢复时已复用页在窗口前逐页排水，不改变在飞窗口的批组合。
3. 对窗口内每页渲染 RGB8，保持 DPI、PdfiumOwner 线程与预取（P3）、worker 分段、像素缩放和 render audit 不变；扫描应用取得 `ocr-markdown-v1` prompt，构造只含 prompt、图像和生成参数的 `TaskRequest`；页码留在扫描局部变量。
4. 一次 `submit_window` 提交整窗（attempt 1 全窗批处理）；进度回调携带槽位序号，扫描 callback 映射页码后再交给 Reporter，通用进度事件本身不含页码。
5. adapter 编码各任务，backend 按槽位解码生成；每个解码秒至少发一次进度，阻塞图像编码开始时发一次事件。窗口解析规则：单页失败只占该槽位条目；批级失败（`WindowFailed`）整窗以批深 1（等价单流）重提；退化页按 attempts 状态机以扰动输入单独按批深 1 重试。
6. 每页返回 `TaskResponse` 后，扫描应用将 `max_tokens` 映射为截断、`repetition_loop` 映射为退化处理，将文本解析为 `PageDocument`，保留公式、表格、视觉区域和重复尾部处理。
7. 扫描应用从原 RGB8 裁剪 bbox，按 `page-NNNN-figure-MMM.png` 落盘，替换 bbox 占位符并验证所有图片链接存在且不逃逸；原始输出按 `page-NNNN.raw` 写 sidecar。
8. 机制 A 窗口原子提交：窗口内全部页到达终态（成功或失败）后，按页序执行既有逐页落盘——整页 PNG、页 Markdown、页 JSON、render audit 和 v6 checkpoint，沿用“产物先 durable，checkpoint 最后 complete”的顺序；取消的在飞窗口零提交，恢复时整窗重扫。
9. 单页渲染/识别/解析失败记录诊断并继续同批其他页；取消不记为质量失败并停止取新页；截断页/退化页提交但 job 为 partial。

#### 第七步：汇总与报告

1. 只收集身份兼容、checkpoint complete、Markdown 及其引用图片哈希匹配的页。
2. native、empty、恢复页和 OCR 页统一按页码排序合并，重复页或 0 页号拒绝。
3. 全部所需页成功后生成 `document.md`；按 `--keep-pages` 决定是否删除整页 PNG，figure 裁剪保留。
4. 无可合并页时删除陈旧 `document.md`，不输出伪造成功文档。
5. 写每页 routing sidecar 和 `result.json`，模型字段从实际 `InferenceIdentity` 生成，backend 使用实际执行值，GPU→CPU 回退后必须报告 CPU。
6. 保持文本/JSON stdout、进度 stderr、routing summary、warnings、diagnostics、图片计数和退出码行为。

### 6.4 模型协议与 OCR 业务解释的分界实例

对于 OCR 页，推理平台只看到：

```text
prompt = 扫描应用给出的完整 OCR 指令
images = 一张 RGB8 图像
max_output_tokens = 16384（或用户有效值）
temperature_milli = 0
thinking = false
```

它不知道这是 PDF 第几页，也不知道输出要有 checkpoint。它只返回文本和结束原因。随后扫描应用独立完成：

```text
TaskResponse.text
  → 去除/解释 bbox 占位符
  → Markdown / Formula / HtmlTable / VisualRegion
  → PageDocument.page_number = 扫描局部页码
  → 图片裁剪与链接替换
  → PdfPageDocument / checkpoint / document.md
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
  batch_depth = 4,
  pixel_width_budget = 2880, pixel_height_budget = 2880,
  page_parser_version = "ovis-page-v1"
}
```

`prompt_schema_version` 是独立的数值字段，当前固定为 1；展示层直接读取该字段，不从 `prompt_version` 字符串解析数字。dpi 只属于 OCR 渲染身份：native/empty 不渲染页面，因此不把 dpi 放入其兼容身份。

匹配规则：

- native/empty 精确比较输入、route、`pdf_mode`、`quality_tier`、`classifier_version`、`extractor_version` 和 `platform`；dpi、批深、profile、device、token、context 等 OCR-only 配置变化不影响复用，与 v5 native 的有效匹配范围（前五个公共参数，含 `platform`）一致。
- OCR 比较全部字段，包括 dpi、批深、每个资产哈希、runtime 版本和提交、adapter 版本、实际 backend、prompt 版本和 schema 版本、生成参数、渲染参数和页面解析版本；批深不同即不兼容（跨批深不互恢复，P4.2 §② 冻结）。
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
warnings
render_audit
```

`degenerated`（bool）、`attempts`（1..=3）和 `raw_sidecar`（`page-NNNN.raw` 指针）承接现有 v5 字段的语义：退化页即使其余身份兼容也不复用；raw sidecar 指针与产物一同纳入完整性收集。

写入仍使用临时文件、flush/sync、原子 rename；complete 只能在窗口内全部页到达终态后、且该页 Markdown、JSON、图片、raw sidecar 和 render audit 成功后写入（机制 A：取消的在飞窗口零提交，恢复整窗重扫）。

v5 读取和迁移规则必须保守（v5 现状：checkpoint 已含 `degenerated`/`attempts`/`raw` 字段，兼容串为 `pdf-mode;quality-tier;classifier;extractor;platform;dpi;tokens;max_image_tokens;context_size;device;batch;route=native` 加可选 `;ocr-profile=`/`;ocr-device=`/`;pending-ocr=true` 后缀）：

- v5 文件可以解析，不能仅因版本旧而崩溃。
- 旧 `complete: bool` 和 compatibility 参数中的 `;pending-ocr=true` 后缀按固定规则映射：`complete=true` 且无 pending 后缀对应 v6 `status=complete`；`complete=false` 且有 pending 后缀对应 v6 `status=pending_ocr`；`complete=false` 且无 pending 后缀表示未完成但不是可恢复的 pending OCR，进入 reprocess；`complete=true` 却带 pending 后缀属于矛盾状态，按无效数据处理并进入 reprocess，不能映射为 complete。
- v5 native/empty 页只有在输入、route 和可映射的 PDF/native 字段完全匹配，且产物哈希有效时才允许复用；dpi、批深变化不影响 native/empty 复用（与 v5 native 宽松匹配的有效范围一致），其他无法映射的必需字段按不兼容处理。
- v5 `degenerated=true` 的页在任何身份组合下都不复用，进入重处理。
- v5 兼容串中的 `batch=N` 参数映射为 v6 `batch_depth`；值不同即不兼容，跨批深不互恢复。
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

`result.json` 保持 `schema_version = 3`、既有字段和字段顺序（`pages` 计数含 `total/completed/failed/truncated/degenerated`，其中 `degenerated` 为复读退化方案加入的既有字段）。`ocr_model_loaded=false` 时 `model` 仍为 `null`；为 true 时，模型对象按下表逐字段派生，不允许实施者另造展示值：

| result.json 模型字段 | 唯一派生规则 | 当前 Ovis 的确定输出 |
|---|---|---|
| `id` | `InferenceIdentity.model.model` | `ovisocr2` |
| `name` | 已解析 profile 的 `ModelManifest.display_name` | `OvisOCR2` |
| `family` | 已解析 profile 的 `ModelManifest.family` | `Ovis` |
| `profile` | `InferenceIdentity.model.profile`，使用 auto 解析后的具体值 | `compact/balanced/quality/reference` 之一 |
| `revision` | `InferenceIdentity.model.source_revision` | `8a22290a0c42dbcf84739d6d4c2763f877494ae0` |
| `mmproj_revision` | 当前 manifest 的 revision 级共享资产所属 `source_revision` | 与 `revision` 完全相同 |
| `adapter` | `InferenceIdentity.adapter.adapter` | `ovisocr2` |
| `adapter_version` | `InferenceIdentity.adapter.version` | `0.1.0` |
| `prompt_schema_version` | `ScanCompatibility.ocr.prompt_schema_version`，直接序列化数值 | 数值 `1`，不是字符串 |
| `runtime` | 字符串拼接 `{InferenceIdentity.runtime.runtime}@{InferenceIdentity.runtime.wrapper_version}`；内部 RuntimeId 固定为 `llama.cpp`，wrapper_version 固定为 `0.1.152` | 精确为 `llama.cpp@0.1.152` |
| `quantization` | 已解析 profile 的 `ModelManifest.quantization`，不得按 display name 猜测 | compact=`Q4_K_M`，balanced=`Q5_K_M`，quality=`Q8_0`，reference=`BF16` |

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
- 删除 `ModelId` 枚举、`AS_STR`、`RUNTIME_COMPATIBILITY` 和 `Device`；模型与 runtime 身份由 inference API 拥有。`WINDOW_DEPTH` 与 `BATCH_ERROR_PREFIX` 随窗口契约迁到 `ocrx-inference-api`（批深常量与批级窗口错误分类），core 不再为 runtime 协议提供常量。
- `Profile` 若仅是扫描配置展示用途则移到 scan/CLI；模型 profile 标识统一使用 inference API 值类型。
- 原有页面 JSON 序列化 golden 不变。

### 8.4 `ocrx-inference-api`：建立稳定公共契约

- 新建第 5 节所有 DTO、trait、错误和校验。
- 请求构造器校验图像宽高、RGB8 长度、token 上限、空 prompt 策略和图像 ID 唯一性。
- 当前 OCR prompt 非空；通用 API 是否允许空 prompt 由 `TaskRequest::validate` 明确，不允许 backend 各自判断出不一致结果。
- fake 测试证明公共类型中不存在页码和 PDF 字段，并覆盖序列化身份的稳定顺序。

### 8.5 `ocrx-models`：通用化资产管理

- 把 `ModelManifest` 从两个哈希字段改为具名资产列表，保留 Ovis 数据到 `builtin/ovisocr2.rs`。
- 把 `community_manifest/community_asset_urls/community_asset_sizes` 替换为 catalog 查询；旧函数不保留为绕过 manager 的第二入口。
- 把缓存、安装、校验、锁、回滚、legacy 迁移和删除按模块拆开。
- 用 `Transport` 抽象下载；生产实现仍执行 HTTPS-only curl，测试使用本地 fake transport，不访问网络。
- 将 `recommend_device` 和硬件资源探测移出；profile 推荐仍在此基于模型元数据和内存输入执行。
- 将 runtime/adapter 兼容检查从硬编码全局常量改为 manifest binding 与 registry 实际身份的严格比较。
- 所有现有缓存安全、幂等安装、磁盘不足、许可证、共享 mmproj 和损坏 hash 测试迁移并扩展到多角色资产。

### 8.6 `ocrx-runtime`：变成纯 backend

- `InferenceRequest/InferenceResponse` 由 API 的通用任务类型替代，删除 `page`；窗口提交以槽位序号标识页，不再依赖业务页码。
- `Runtime` 改为内部 `RuntimeBackend`，输入是 adapter 已编码的 `EncodedTask`；P4.1 推理线程 actor 与 P4.3 多槽位窗口解码（多序列 batch、逐序列 EOS/环检测、窗口上下文容量计算、批深回退 1 语义）保留为 backend 内部实现，暴露为可选 `generate_window` 覆盖。
- 从 `llama_mtmd.rs` 删除 `PROMPT`，只根据 `ImagePlacement` 插入 MTMD 默认 marker 并拼接任务 prompt。
- 保留 GPU 层数、CPU `main_gpu=-1`、context（单流 20480 默认 + 窗口动态容量）、图像 token、EOS、贪心、逐 token telemetry（窗口内全序列聚合累加）、阻塞编码进度和取消检查。
- 将字符串错误改为结构化错误，但保留 CLI 可识别的设备初始化类别；批级窗口失败映射为 `WindowFailed`，替代现 `BATCH_ERROR_PREFIX` 字符串协议。
- fake 改成可编程响应队列，可检查接收到的 prompt/图像/参数并模拟 EOS、max_tokens、`repetition_loop`（现 `LoopFakeRuntime` 语义）、backend error、批级窗口错误和 cancel；不再返回 `page {request.page}`。
- runtime factory 返回实际 backend 和 runtime identity；不接触模型 catalog 或安装。

### 8.7 `ocrx-ovisocr2`：只保留协议 adapter

- 新建 `OvisProtocolAdapter`，ID/version 保持 `ovisocr2/0.1.0`。
- 移出 `write_page_artifacts*`、`parse_page*`、`adapt_response`、bbox、Markdown 和图片逻辑。
- 删除对 `ocrx-core`、`ocrx-runtime`、`ocrx-pdf` 和 `image` 的依赖，只依赖 inference API。
- 添加协议测试：图像在 prompt 前、prompt 字节不变、生成参数不变、native finish 正确映射。
- 原 real-model 测试迁移到 scan 或 inference 的可选集成测试，不再让协议 crate 知道 PDF 输出。

### 8.8 `ocrx-inference`：实现平台组合与管理

- 建立静态 registry，注册当前唯一模型、adapter、runtime factory 和 binding。
- binding 同时校验模型所需资产角色、adapter ID/version、runtime wrapper/sys/upstream 版本和支持的图像输入。
- `prepare` 实现完整的资产就绪、设备选择、runtime 加载、GPU→CPU 回退和 session 创建。
- 将 CLI 的 `RealSystemProbe`、设备可用性交集、doctor 支撑和 GPU 采样迁入。
- 保留“显式设备不可用直接失败”和“已选 GPU 在初始化时失败则回退 CPU并 warning”两种不同阶段的不同语义。
- session 持有 backend、adapter、identity 和累计 telemetry；每次 submit 只做 encode → generate → decode。

### 8.9 `ocrx-output`：去掉扫描编排，升级持久化身份

- 移出 `ScanService`、`ModelLoader`、`PageRecognizer`、`PageArtifactRecognizer`、OCR 队列和页面计划逻辑到 scan；P4.4 窗口编排（窗口组装/恢复页排水/槽位解析/批级失败批深回退 1/机制 A 窗口原子提交）与复读退化重试状态机随 `ScanService` 与识别抽象（含 `recognize_window_cancelled`）一并迁到 scan。
- 保留 `PageLifecycle` 或移到 scan/state；output 只关心持久化状态，不决定下一页走哪条路线，也不决定窗口怎么组装。
- 用 `ScanCompatibility` 替换 `CompatibilityKey` 和字符串 `parameters`。
- 新写 v6，按第 7 节读取 v5；保留 complete/output hash/truncated/degenerated/attempts/raw sidecar/warnings/audit 和 pending 状态。
- 保留 `commit_page`、`commit_native_page`、原子写、输出路径保护、产物哈希、Markdown 合并、Reporter 和现有 golden。
- output 可以依赖 inference API 来保存 `InferenceIdentity`，但不得依赖具体 models/runtime/adapter。

### 8.10 `ocrx-scan`：形成真正的扫描应用

- 从 CLI 迁移 `run_scan` 的业务部分，对外提供不调用 `std::process::exit` 的 `ScanApplication::run`。
- 从 CLI 迁移分类队列、native-first、fallback、pending 恢复、最终 OCR 计划、renderer（含 P3 `pdfium_owner.rs` 的 PdfiumOwner 线程所有权、预取协议和扰动渲染转发）、recognizer、路由 sidecar、结果汇总和遥测。
- 在 `window.rs` 实现窗口编排：固定批深 `WINDOW_DEPTH`（常量来自 inference API）、按页序填充、不做动态回填、恢复页排水、批级失败（`WindowFailed`）整窗批深回退 1、机制 A 窗口原子提交；窗口策略是扫描编排语义，不进入推理 API 请求类型。
- 从 runtime 迁移完整 prompt 到 `ocr/prompt.rs`。
- 从 `ocrx-ovisocr2` 迁移页解析、bbox 裁剪、Markdown 替换和链接验证到 `ocr/`。
- 定义 `ScanRequest/ScanOutcome/ScanEvent`，由 CLI 将事件映射为现有 Reporter；扫描库不直接打印终端，也不自行安装进程级 signal handler。
- 用 API session 替代 `PreloadedLoader<LlamaMtmdRuntime>` 和 `OvisRecognizer<LlamaMtmdRuntime>`。
- 所有页错误返回结构化 outcome，由 CLI 决定退出码，保证测试可以直接调用完整业务而无需启动子进程。

### 8.11 `ocrx-pdf`：保留 PDF 基础设施，移出评估

- 保留 PDFium renderer、worker、页面信号、分类、native 提取和校验、empty/native 产物、像素预算、bbox 和错误分类。
- 将 `QualityManifestEntry`、质量 manifest 加载/校验、NED、block/bbox/阅读顺序指标、路由混淆、native release gate、performance gate 和 release report 移到 `ocrx-eval`。
- 不修改分类和 native 校验函数输入输出；迁移测试必须逐项同名或建立清晰的一一对应。
- 移除 PDF crate 对质量真值和发布报告的所有权。

### 8.12 `ocrx-eval`：建立独立质量模块

- 接收从 PDF 移出的 Rust 质量契约、指标和门禁，保持数值与报告 golden。
- `execute.rs` 提供两种明确入口：完整 PDF 质量评估调用 `ScanApplication`；模型/runtime 专项评估调用 `InferenceService` 并由评估方提供 prompt、图像和真值。
- 评估身份包含 set/truth 版本、输入 hash、完整 inference identity、prompt/parser 版本和渲染参数。
- 既有 `assets/regression/` 的 manifest、truth、predictions、reports 路径和 JSON schema 不因重构改变。
- 既有 `scripts/score-regression.py`、`run-quality-gate.py`、native 评分和验证脚本继续作为可执行工具；将脚本调用对象收敛到公开 CLI 或版本化文件契约，不让脚本复制 Rust 扫描/资产/设备流程。
- 用同一批小型输入比较迁移前后评分结果，不能通过改阈值或改真值获得通过。

### 8.13 `ocrx-cli`：收缩为入口和展示

- `main.rs` 只保留参数到 command 的分发和组合上下文调用。
- 删除 `run_scan` 及其辅助业务函数，删除 `OvisRecognizer`、`PreloadedLoader`、`PdfScanRenderer`、`WorkerScanRenderer`、`pdfium_owner`（迁至 scan 渲染路径）、`load_runtime_with_fallback`、模型安装实现和资产路径拼接。
- 删除对 `ocrx-runtime`、`ocrx-models`、`ocrx-ovisocr2`、`ocrx-pdf` 的直接依赖；CLI 通过 `ocrx-inference` 组合，通过 `ocrx-scan` 执行。
- 保留参数、help、默认模型 `ovisocr2`、配置优先级、输出格式、doctor 展示、退出码和 skill 命令。
- 将 `serialize_scan_result` 改为基于 scan outcome 的展示序列化，字段和顺序 golden 不变。
- CLI 不直接打印扫描库内部错误；统一由 present 层输出，避免重复报告。

### 8.14 测试、脚本和说明的同步修改

- 现有单元测试随所有者迁移，不允许通过删除测试解决类型变化。
- 新增 inference API 合约测试、静态绑定测试、多角色资产测试、扫描应用 fake 端到端测试和架构依赖检查。
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
| 窗口合批 | 固定批深填充/不动态回填、恢复页排水、批级失败批深回退 1、机制 A 窗口原子提交、取消窗口零提交、同输入同批深自一致逐字节、跨批深不互恢复 | fake 与受控测试证明，行为与 P4.4 门禁语义一致；无真实模型 |
| binding | 正确三元组通过，runtime/sys/adapter/角色任一不符失败 | 错误类别为 IncompatibleBinding |
| native/empty | 不准备模型、不创建 session、产物无整页伪造 PNG | fake 计数为 0，产物 golden 不变 |
| OCR | prompt 从 scan 到 fake、文本解析、公式/表格/bbox、PNG 和链接 | 页 JSON/Markdown/PNG golden 不变 |
| 路由 | auto/text/ocr、native fallback、empty、队列排序去重 | 现有路由测试全部保留并通过 |
| checkpoint | v6 完整身份、任一身份变化、产物 hash、pending、degenerated 不复用、v5 迁移 | OCR 不跨模型/prompt/runtime/批深复用；v5 OCR 安全重做 |
| 取消/失败 | 首/二次取消、页间停止、失败继续、已提交页保留 | 退出码和诊断保持，取消不算质量失败 |
| 报告 | doctor、result、错误、进度、路由 sidecar、路径 escaping/redaction | 现有 golden 和 JSON 解析断言通过 |
| 评估 | manifest、NED、block、bbox、阅读顺序、路由、release/performance gate | 同一输入迁移前后数值和 JSON 一致 |

### 9.3 行为对比验收

在同一台机器、同一 fixture、同一有效配置下，对重构前后结果做结构化比较：

- 页分类、初始/最终 OCR 队列、fallback 原因一致。
- 页 Markdown、页 JSON、document.md、figure PNG 和 render audit 一致；时间戳等非确定字段不参与逐字节比较。
- `result.json` 排除 `elapsed_ms` 后一致；真实模型文本若存在环境差异，用固定 fake 输出验证业务产物，再用已有真实资产做非门禁观察。
- 首次扫描、完整复用续扫、pending OCR 续扫、模型/prompt 变化后的重处理符合预期。
- native-only 扫描中模型安装和 runtime 加载调用均为 0。
- 多页 OCR 中 prepare/load 为 1；窗口提交次数与窗口组装策略一致，attempt 1 批处理请求数加上批深 1 回退与扰动重试请求数等于实际需处理页数。
- 同一输入以相同批深连跑两遍，页产物逐字节相同（P4 自一致性语义在重构后保持）。
- 所有 CLI 命令的 stdout/stderr 和退出码符合现有 golden。

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
- native/empty/OCR、fallback、按需加载、单 session 多页窗口合批（P4 冻结语义不回退：默认批深 4、机制 A 原子提交、自一致确定性）、复读退化处理、GPU 回退、取消、失败继续、续扫和最终合并均通过测试。
- v6 checkpoint 保存完整身份，v5 迁移符合安全规则，不跨模型/prompt/runtime 复用 OCR。
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

- [ ] T001（无依赖）记录当前工作区状态和已有未提交修改，建立只修改本重构所需文件的清单；完成判定：不会覆盖或回滚用户已有修改，基线记录可供后续对照。
- [ ] T002（T001）运行 `cargo build --workspace` 和 `cargo test --workspace`，保存通过/失败清单；完成判定：每个既有失败被区分为重构前问题或环境问题，不把既有失败误记为重构回归。
- [ ] T003（T001）运行现有 CLI doctor/model/参数错误及扫描失败分支 golden，逐分支记录 stdout、stderr 和退出码特例；完成判定：清单显式包含 routing sidecar 提交失败为 `code:5, kind:"output"` 的历史行为，后续展示测试有明确期望值且不得顺手统一为 9。
- [ ] T004（T001）用本地 fixture 记录 native-only、empty、auto fallback、OCR fake 的页面计划和产物 golden；完成判定：每类至少有一个可重复输入和预期输出，缺 PDFium 的项目明确标为环境未执行而非失败。
- [ ] T005（T001）逐项核对 Ovis 四 profile 的 revision、URL、模型文件名、共享 mmproj、SHA-256、字节数及展示元数据；完成判定：形成内置 manifest 字段清单，display_name=OvisOCR2、family=Ovis 和四档 quantization 映射明确，所有值与当前代码一致且无需网络。
- [ ] T006（T001）冻结完整 `ocr-markdown-v1` prompt、MTMD 图像在 prompt 前的顺序和生成默认值；完成判定：有字节级预期字符串和参数清单，包括开头换行。
- [ ] T007（T001）盘点 v5 checkpoint、result schema 3、doctor schema 2、routing sidecar 和质量报告字段；完成判定：每种格式都有字段级兼容/迁移决定，明确 runtime=`{RuntimeId}@{wrapper_version}`=`llama.cpp@0.1.152`、prompt_schema_version=数值 1、native/empty 忽略 dpi/批深、v5 状态映射（含 `degenerated`/`attempts`/`raw` 字段与 `platform`/`batch` 等兼容串参数、`;pending-ocr=true` 后缀），无“实施时再定”的字段。
- [ ] T008（T001）盘点 `ocrx-pdf` 中质量清单、指标、门禁与 `scripts/`/`assets/regression/` 的对应关系；完成判定：每个待迁移 Rust 函数都有目标模块和现有测试/脚本验证方式。

### 阶段 B：工作区骨架、技术原语和公共 API

- [ ] T009（T002、T003、T004、T005、T006、T007、T008）在根 `Cargo.toml` 注册五个新 crate 并创建最小 package；完成判定：workspace metadata 能解析，新 crate 尚无业务反向依赖。
- [ ] T010（T009）实现 `ocrx-primitives` 的取消与进度类型并移动相关测试；完成判定：第一次/第二次取消和进度字段测试通过，类型不含 OCR/PDF 概念。
- [ ] T011（T009）将 SHA-256 流式和内存实现移动到 `ocrx-primitives`；完成判定：NIST 向量、分块和文件哈希测试通过，core/models/output 可改用同一实现。
- [ ] T012（T009）在 `ocrx-inference-api/id.rs` 实现模型、profile、adapter、runtime、backend、资产角色值类型及校验；完成判定：非法空值/重复角色有测试，当前 catalog 不需要核心枚举。
- [ ] T013（T012）实现资产 manifest v2、作用域、来源和绑定 DTO；完成判定：多必需/可选角色、重复角色、非法 hash、非法路径均有单元测试。
- [ ] T014（T012）实现 `InferenceIdentity` 及确定性序列化；完成判定：资产顺序固定，模型、runtime、adapter、实际 backend 和逐资产 hash 均可序列化比较。
- [ ] T015（T010、T012）实现 `TaskRequest/TaskResponse/ImageInput/GenerationParams/Usage/FinishReason`；完成判定：公共请求无页码/PDF/output 字段，RGB8 和 token 非法输入测试通过。
- [ ] T016（T015）实现 `ProtocolAdapter/RuntimeBackend/EncodedTask` 扩展契约；完成判定：EncodedTask 能表达图像在 prompt 前，且不包含 Ovis PageDocument、bbox 或 Markdown 类型。
- [ ] T017（T010、T014、T015）实现 `InferenceService/InferenceSession/PrepareRequest/InstallInteraction`；完成判定：trait 对象可用于 fake，session 支持同一实例多次 submit 和累计 telemetry。
- [ ] T018（T013、T014）实现 `ModelManager/DeviceManager` DTO 与结构化错误；完成判定：所有管理和推理失败都有明确类别，CLI 可映射到既有退出码而无需解析任意字符串。

### 阶段 C：模型目录、资产与安装管理

- [ ] T019（T013）把 Ovis 四 profile 固定数据迁移到 `ocrx-models/builtin/ovisocr2.rs`，旧 `community_*` 函数可暂时委托新实现；完成判定：T005 清单逐项相等，旧常量不再成为第二份事实源，旧函数签名在 T086 前保持不变且旧 CLI 仍编译。
- [ ] T020（T019）实现静态 `ModelCatalog` 的 list/info/resolve；完成判定：只列出 `ovisocr2`，profile 去重排序，未知模型/profile 返回结构化错误。
- [ ] T021（T013、T019）实现具名资产生成的缓存路径和现有 Ovis 布局映射；完成判定：新旧 Ovis 路径逐字一致，绝对路径和 `..` 逃逸测试通过。
- [ ] T022（T021）迁移默认缓存根和 legacy 缓存探测/迁移；完成判定：Windows/macOS/temp fallback、旧布局、篡改文件不迁移等现有测试全部通过。
- [ ] T023（T021）实现本地安装状态检测与全资产大小/hash 校验；完成判定：缺任一必需角色、错误大小或错误 hash 均失败，无 receipt 的合法旧安装可通过。
- [ ] T024（T018、T021）实现 `Transport` 与 HTTPS-only curl 生产实现；完成判定：offline 在创建进程前失败，HTTP/file/降级重定向被拒绝，测试不访问网络。
- [ ] T025（T023、T024）实现 fake transport 和受控本地小文件安装 fixture；完成判定：可模拟字节、网络失败和错误 hash，所有测试输入都在临时目录生成。
- [ ] T026（T025）实现 revision 范围安装锁、`.part`、备份和资产组提交；完成判定：主模型与共享资产在同一事务中，锁覆盖全部目标，异常路径清理临时文件。
- [ ] T027（T026）实现失败回滚、幂等安装、磁盘与许可证策略；完成判定：任一步失败不暴露半套有效状态，已验证重复安装无网络且成功。
- [ ] T028（T027）实现最后提交的 `install-receipt.json`；完成判定：receipt 含完整来源和资产身份，损坏 receipt 不会绕过实际文件 hash 校验。
- [ ] T029（T023）实现 profile 删除与共享资产引用检查；完成判定：删一个 Ovis profile 保留仍共享 mmproj，最后一个 profile 删除后回收 mmproj 和空目录。
- [ ] T030（T018、T020、T023、T026、T028、T029）实现 `ModelManagerImpl` 全部方法并将设备探测职责移到推理设备服务；完成判定：list/info/recommend/install/verify/remove 均可无 PDF 独立测试，新生产路径不调用旧 `community_*` 或旧 `recommend_device`；旧 `community_*` 仅作为未迁移 CLI 的过渡委托保留到 T086，旧 `recommend_device` 自含保留现有实现到 T086，`ocrx-models` 不得为委托它而依赖 `ocrx-inference`。

### 阶段 D：runtime、Ovis adapter 与推理组合

- [ ] T031（T015、T016）新增接收 `EncodedTask` 的 llama.cpp `RuntimeBackend`（迁移 P4.1 推理线程 actor 与 P4.3 多槽位窗口解码，暴露可选 `generate_window`），同时保留旧 `Runtime`/`generate_window` 的 legacy 包装供未迁移 CLI 使用；完成判定：新 backend 路径无 OCR prompt、页码、PageDocument 或输出目录，legacy 包装能把旧请求与旧窗口调用委托到新路径且旧 CLI 仍编译（批级错误协议保持 `BATCH_ERROR_PREFIX` 行为），固定 binding 版本未升级。
- [ ] T032（T031）实现 MTMD 默认 marker 与 prompt 的协议拼接；完成判定：图像在 prompt 前，prompt 字节原样进入 tokenize，RGB8/context/image-token 行为保持；冻结 prompt 只允许暂存于待 T087 删除的 legacy 包装，新 backend 不含该常量。
- [ ] T033（T010、T031）迁移逐 token 生成、取消、进度和累计 telemetry；完成判定：EOS/max_tokens/backend error/cancel 映射结构化，编码开始与每秒进度测试通过，legacy 与新 backend 的取消行为一致。
- [ ] T034（T031）实现 runtime factory 和实际 backend/runtime identity；完成判定：CPU/GPU 请求与实际 identity 分开，RuntimeId=`llama.cpp`、wrapper=`0.1.152`、sys=`0.1.153`、upstream=`9e3b928` 均准确，旧 runtime 构造器在 CLI 切换前仍可用。
- [ ] T035（T015、T016）实现可编程 `FakeRuntimeBackend` 并暂时保留旧 fake 包装；完成判定：新 fake 可捕获请求并排队返回 EOS、max_tokens、`repetition_loop`（现 `LoopFakeRuntime` 语义）、批级窗口错误、错误和取消，不再用页码构造文本，旧 fake 的既有测试在迁移完成前仍通过。
- [ ] T036（T016）实现本身只依赖 inference API 的 `OvisProtocolAdapter` 新模块；完成判定：图像顺序、prompt、generation 和 finish 测试通过，新模块无 core/runtime/pdf/image 依赖；旧业务模块及其依赖可暂时保留到 T087，但新 adapter 不得调用旧模块。
- [ ] T037（T036）把 Ovis 页解析、bbox、Markdown 和图片代码复制到预定 scan 模块，暂不删除 adapter crate 的旧实现和旧导出；完成判定：新旧 parser/artifact 单元测试均通过，旧导出签名不变且 CLI 仍编译，`cargo check --workspace` 返回 0，旧代码明确限定由 T087 删除。
- [ ] T038（T014、T019、T034、T036）实现静态兼容 binding registry；完成判定：正确三元组通过，runtime/sys/adapter/资产角色任一不符均返回 IncompatibleBinding。
- [ ] T039（T018、T034）迁移系统 probe 和编译能力交集到 `ocrx-inference`；完成判定：Metal/CUDA/Vulkan 编译与硬件取交集，auto 优先级和显式设备错误测试通过。
- [ ] T040（T030、T038、T039）实现 `InferenceServiceImpl::prepare` 的资产就绪和绑定路径；完成判定：只准备一次，缺资产走统一安装，离线/无效 hash/绑定失败不会创建 session。
- [ ] T041（T040）实现 runtime 加载及 GPU 初始化失败回退 CPU；完成判定：显式不可用设备仍直接失败，加载阶段 GPU 失败回退并 warning，identity 和报告 backend 为实际 CPU。
- [ ] T042（T035、T036、T040）实现 session 的 encode → generate → decode 和多次 submit；完成判定：fake 连续请求共享一次加载，响应和累计 telemetry 不重复累加。
- [ ] T043（T039、T041）迁移 doctor 支撑和可选 GPU 采样；完成判定：无 OCR 不启动采样，GPU→CPU 后停止并无意义 VRAM 数据，doctor 所需 DTO 完整。

### 阶段 E：结构化 checkpoint 与输出持久化

- [ ] T044（T014、T015）在 output 新增 `ScanCompatibility` 和 route/native/OCR/批深字段，暂时保留旧 `CompatibilityKey`；完成判定：新身份不用字符串前缀或参数位置判断语义，OCR 可保存完整 InferenceIdentity、prompt/parser/render 版本和 `batch_depth`，旧 CLI 仍可使用旧类型编译。
- [ ] T045（T044）实现 native/empty/OCR 匹配函数；完成判定：native/empty 忽略 dpi、批深及全部 OCR-only 改动，OCR 对 dpi、批深和任一资产/runtime/adapter/backend/prompt/render 改动失效，route 不交叉匹配。
- [ ] T046（T045）实现 v6 checkpoint 确定性读写和状态字段，暂时保留旧 v5 写入入口；完成判定：complete/pending_ocr、output hash、truncated、degenerated、attempts、raw sidecar、warnings、audit 全部 round-trip，complete 写入发生在窗口全部页终态之后（机制 A），旧 CLI 的 v5 读写测试仍通过。
- [ ] T047（T046）实现 v5 读取和安全迁移；完成判定：`complete=true` 且无 pending 后缀映射为 `status=complete`，`complete=false` 且有 `;pending-ocr=true` 映射为 `status=pending_ocr`，其他 false 状态为 reprocess，`complete=true` 与 pending 后缀的矛盾组合按无效/reprocess；兼容 native/empty 可复用，v5 OCR complete 必重处理，v5 `degenerated=true` 不复用，v5 `batch=N` 映射 `batch_depth` 且值不同即不兼容，v4 不崩溃且不直接复用。
- [ ] T048（T046）迁移原子页提交、native 提交、产物 hash 和 render audit；完成判定：complete checkpoint 最后写，引用图片缺失或篡改不可复用，无整页 PNG 的 native 行为不变。
- [ ] T049（T046）迁移 pending 队列恢复并按 route 去重；完成判定：排序稳定、输入不兼容不恢复、同一页不会被 native 和 OCR 重复收集。
- [ ] T050（T009）将 output 的取消/进度/哈希切换到 primitives；完成判定：无重复实现，现有 output 测试通过，对外 legacy re-export 可暂留并由 T086 统一删除。
- [ ] T051（T044、T048）把 ScanService（含 P4.4 窗口编排与机制 A 原子提交）、loader、recognizer（含 `recognize_window_cancelled`）和 OCR 队列复制到 scan 新模块，暂不删除 output 旧实现和旧导出；完成判定：新 scan 不调用 output 旧扫描类型，旧导出签名不变且 CLI 仍编译，`cargo check --workspace` 返回 0，旧代码明确限定由 T088 删除。

### 阶段 F：扫描应用迁移

- [ ] T052（T017、T051）建立 `ScanApplication/ScanRequest/ScanOutcome/ScanEvent`；完成判定：库入口不打印、不退出进程、不安装 signal handler，CLI 可订阅事件并映射报告。
- [ ] T053（T052）迁移输入、页范围、数值参数和输出预检；完成判定：PDF 先于模型报错、路径保护、resume/overwrite 互斥和历史退出类别测试通过。
- [ ] T054（T052）迁移页面分析、auto/text/ocr 初始计划和路由诊断；完成判定：现有分类输入对应相同队列和 planned/actual/fallback 元数据。
- [ ] T055（T045、T047、T052）实现扫描恢复计划；完成判定：v6/v5、完整复用、pending、重处理和 selected pages 限制测试通过；OCR 回退专项证明 v6 按实际 backend 而非请求设备匹配——请求 GPU 但回退 CPU 所写的 checkpoint 在实际仍为 CPU 时即使请求设备不同也复用，实际 GPU→CPU 变化时重处理。
- [ ] T056（T052、T055）迁移 native-first、text 失败、auto fallback 和 empty 提交；完成判定：native/empty 路径不调用 prepare，产物 golden 与 T004 一致。
- [ ] T057（T056）实现最终 OCR 队列合并和按需 prepare；完成判定：队列为 0 时模型/设备 prepare 为 0，非 0 时只 prepare 一次。
- [ ] T058（T052）迁移 PDFium 与 worker renderer、PdfiumOwner 线程所有权、预取协议和扰动渲染转发（P3）、像素缩放和 render audit；完成判定：DPI、分段、尺寸校验、2880² 预算、scale ratio、预取与故障注入测试保持，PDFium 入口安全不变式不回退。
- [ ] T059（T006、T052）在 `ocr/prompt.rs` 写入完整 `ocr-markdown-v1` prompt 和构造器；完成判定：T006 字节 golden 通过，扫描生成的 TaskRequest prompt 与冻结文本一致。
- [ ] T060（T037、T059）迁移 Ovis 文本到 PageDocument 的解析；完成判定：公式、表格、bbox、CRLF、重复尾部、空输出、EOS/stop/max_tokens golden 全通过。
- [ ] T061（T037、T060）迁移 bbox 裁剪、整页 PNG、图片替换和链接校验；完成判定：文件命名、数量、路径逃逸拒绝和缺失链接错误与现状一致。
- [ ] T062（T017、T058、T059、T060、T061）实现单页 OCR 流程；完成判定：fake 收到无页码任务，scan 用局部页码生成产物，截断和解析警告正确映射。
- [ ] T063（T062）实现多页窗口化编排（固定批深填充/恢复页排水、attempt 1 批处理、批级失败批深回退 1、机制 A 窗口原子提交、退化页扰动重试）、失败继续、取消停止和进度槽位→页码映射；完成判定：prepare/load 一次，窗口提交与页数关系准确，取消的在飞窗口零提交且恢复整窗重扫，后续已提交页保留，同批深自一致。
- [ ] T064（T045、T062、T063）把实际 InferenceIdentity 写入 OCR v6 checkpoint；完成判定：GPU 回退、模型资产、prompt/parser/render/generation 任一变化均按规则处理。
- [ ] T065（T048、T049、T056、T063）实现跨路线产物收集和 document.md 汇总；完成判定：只收集已验证页，排序/去重、空结果 stale 清理和 keep-pages 行为与基线一致。
- [ ] T066（T052、T065）迁移 routing sidecar、ScanOutcome 和 result 所需字段；完成判定：模型 identity、实际 backend、页计数、warnings、diagnostics、images 和 routing 数据完整。
- [ ] T067（T052、T063）迁移扫描遥测与可选 GPU 报告映射；完成判定：累计值不双算，报告写失败只 warning，实际 CPU 回退不生成误导 GPU 数据。
- [ ] T068（T052、T057）实现模型准备失败后的 pending 状态和部分结果；完成判定：native/empty 已提交结果保留，OCR 页可续扫，错误和结果输出与原分支一致。

### 阶段 G：CLI 收缩与生产组合

- [ ] T069（T030、T038、T039、T042、T052）实现 `compose.rs` 生产组合根；完成判定：只注册 Ovis，scan 接收 API trait，model/doctor 使用同一 manager，无第二套注册数据。
- [ ] T070（T069）拆分参数、配置和交互模块；完成判定：现有参数取值、默认值、配置优先级、offline/non-interactive 和下载确认测试通过。
- [ ] T071（T069）将 `model list/info/recommend` 切换到 ModelManager；完成判定：无 PDF/scan 对象，输出与 T003 golden 一致。
- [ ] T072（T069、T070）将 `model install/verify/remove` 切换到 ModelManager；完成判定：CLI 无 URL/hash/cache 拼接，交互和错误码 golden 通过。
- [ ] T073（T043、T069）将 doctor 切换到 DeviceManager/ModelManager DTO；完成判定：文本、schema 2 JSON、缓存 escaping 和 skill_installed 合并与基线一致，旧 scan 经兼容层仍编译且 `cargo check --workspace` 返回 0。
- [ ] T074（T052、T069）将 scan 命令切换到 `ScanApplication::run`；完成判定：CLI 不再包含 run_scan orchestrator，fake 与本机 fixture 结果符合基线。
- [ ] T075（T066、T074）实现扫描事件到 Reporter、result schema 3 和退出码映射；完成判定：stdout/stderr、字段顺序、错误对象、partial/cancel 代码与 T003/T004 一致，逐字段 golden 明确验证 runtime=`llama.cpp@0.1.152`、adapter_version=`0.1.0`、prompt_schema_version=数值 1、四档 quantization 及 `pages.degenerated` 计数。
- [ ] T076（T074）保留 Ctrl+C 两次信号策略并连接扫描取消；完成判定：handler 在慢阶段前安装，token 类型全链一致，第二次仍强制 130。
- [ ] T077（T069、T074）删除 CLI 对 runtime/models/ovis/pdf/image 的直接依赖和旧辅助类型；完成判定：Cargo manifest 与源码均无旧 import，CLI 仅保留组合/展示/交互/skill 职责，`cargo check --workspace` 证明后续兼容层已没有 CLI 消费方。
- [ ] T078（T071、T072、T073、T074、T075、T076、T077）运行全部 CLI 子进程测试；完成判定：doctor、model、scan 参数、native resume、输出保护、JSON 和退出码测试全部通过。

### 阶段 H：质量评估独立化与脚本适配

- [ ] T079（T008、T009）将质量 manifest 类型、加载和输入 hash 校验复制到 `ocrx-eval/manifest.rs`，暂留 PDF 过渡 re-export；完成判定：原合法/非法 TSV 用例结果一致，新实现归 eval 所有，旧导出在 T083 前保持编译兼容。
- [ ] T080（T079）将 NED、block、bbox、阅读顺序和路由指标复制到 `ocrx-eval/metrics.rs`，暂留 PDF 过渡 re-export；完成判定：同一组数值 fixture 的每个浮点/计数结果与迁移前一致。
- [ ] T081（T079、T080）将 native/performance/release 门禁和报告复制到 eval，暂留 PDF 过渡 re-export；完成判定：阈值、失败列表和 JSON golden 完全不变。
- [ ] T082（T014、T052、T079）实现评估执行身份和两种公开执行入口；完成判定：端到端入口调用 ScanApplication，专项入口调用 InferenceService，均记录 set/truth/inference/prompt/render 版本。
- [ ] T083（T079、T080、T081）调整 Rust 调用方和测试到 eval 新路径并删除 PDF 过渡 re-export；完成判定：无 `ocrx_pdf::Quality*` 等旧引用，未通过删除质量测试解决编译，`cargo check --workspace` 返回 0。
- [ ] T084（T082）核对并适配 Python 工具的公开 CLI/文件契约；完成判定：现有 regression manifest/truth/prediction/report schema 和路径不变，无脚本复制资产或设备逻辑。
- [ ] T085（T084）运行 Python 单元测试和固定小样评分对比；完成判定：迁移前后 score/quality/native 报告数值一致，未修改真值或阈值。

### 阶段 I：清理、架构检查与全面回归

- [ ] T086（T019、T030、T050、T077、T083）删除 core 中 ModelId、RUNTIME_COMPATIBILITY、重复取消/哈希及无用 re-export，并删除 models 的旧 `community_*` 过渡委托和旧 `recommend_device` 自含过渡实现；完成判定：全 workspace 无旧类型和旧函数引用，页面领域 golden 仍通过。
- [ ] T087（T031、T037、T060、T061、T077）删除 `ocrx-ovisocr2` 旧业务函数和依赖，并删除 runtime 的旧 Runtime/fake/请求响应兼容包装及其中暂存的 prompt；完成判定：adapter crate 只含协议 adapter，runtime 只含通用 backend，依赖和源码检查证明无 core/pdf/扫描 prompt/page。
- [ ] T088（T051、T052、T077）删除 output 中已迁移的扫描服务、识别抽象、旧 CompatibilityKey 和旧 v5 写入入口；完成判定：无重复实现，所有调用只走 scan 应用和 v6 持久化，`cargo check --workspace` 返回 0。
- [ ] T089（T077、T087、T088）实现 `validate-architecture-boundaries.py` 的 Cargo 白名单和源码禁用检查；完成判定：合法树返回 0，临时违规 fixture 能被检出，脚本只用标准库。
- [ ] T090（T089）把架构检查加入本机 bootstrap，不加入网络/模型/GPU 前置；完成判定：干净本机按脚本顺序可执行，外部条件缺失不会阻断 fake 测试。
- [ ] T091（T086、T087、T088）清理 Cargo 依赖、feature 转发和 lockfile；完成判定：无未使用直接依赖，vulkan/cuda 传递有效，`cargo metadata --locked` 成功。
- [ ] T092（T086、T087、T088）全量搜索并更新旧行为注释和文档字符串；完成判定：不再出现 runtime 持有 OCR prompt、adapter 写页产物、output 拥有扫描服务、请求含页码等过时描述。
- [ ] T093（T078、T085）运行 `cargo fmt --all -- --check` 和 `cargo build --workspace --locked`；完成判定：格式和完整构建均返回 0。
- [ ] T094（T093）运行 `cargo test --workspace --locked`；完成判定：全部非 ignore 测试通过，无真实模型下载，无新增失败被跳过。
- [ ] T095（T093）运行 `cargo clippy --workspace --all-targets --locked -- -D warnings`；完成判定：无 warning/error，未通过全局放宽 lint 掩盖问题。
- [ ] T096（T085、T089、T090）运行 Python 单元测试、架构检查和 release 脚本测试；完成判定：三个命令均返回 0，测试未访问外网。
- [ ] T097（T078、T094）运行 doctor/model/version 本机命令并比较 golden；完成判定：T003 所有适用输出和退出码一致，JSON 可解析且 stdout 行数正确。
- [ ] T098（T004、T068、T078、T094）执行 native-only/empty 的本机端到端对比；完成判定：模型准备和加载为 0，页产物、document、result 和续扫与基线一致。
- [ ] T099（T004、T062、T063、T064、T094）执行 fake OCR 多页端到端对比；完成判定：一次 prepare、窗口化 submit（attempt 1 批处理 + 批深 1 回退/扰动重试覆盖全部页），页产物、路由、失败继续、取消、机制 A 提交和报告符合基线。
- [ ] T100（T047、T055、T064、T094）执行 checkpoint 专项验收；完成判定：v6 全身份匹配、v5 OCR 重做、pending 恢复、跨 prompt/runtime/model/批深不复用、v5 `degenerated` 不复用、取消窗口零提交后整窗重扫和产物篡改拒绝均有实际测试证据；另行保留 GPU→CPU 回退 checkpoint 的测试记录，证明请求设备不同但实际 CPU 相同可复用、实际 backend 不同必须重处理，不得将该语义变化当回归修回 v5 的请求设备匹配。
- [ ] T101（T041、T064、T067、T094）执行设备与身份专项验收；完成判定：显式设备失败、auto 选择、GPU 初始化回退、实际 backend 报告和 checkpoint 身份均通过 fake/受控测试。
- [ ] T102（T084、T085、T096）执行质量模块专项验收；完成判定：两种评估入口可调用，报告身份完整，固定样例数值与历史一致。
- [ ] T103（T097、T098、T099、T100、T101、T102）运行 `bash scripts/bootstrap-check.sh`；完成判定：脚本完整返回 0，过程未触发模型下载或外部服务。
- [ ] T104（T103）检查最终 `git status` 和变更清单；完成判定：只有方案内文件和必要 lockfile 变化，无测试临时文件、日志、下载物或用户既有文件被误改。
- [ ] T105（T103、T104）逐项核对第 11 节完成定义并记录可选真实验证是否执行；完成判定：所有本机门禁有通过证据，未执行的真实模型/GPU 项仅注明环境原因且未被列为阻塞。

### 非门禁后续任务（不属于本次重构完成条件）

- [ ] T106（T105；仅在已有真实 Ovis 资产和 PDFium 时可选）运行真实 Ovis 故障注入和多页冒烟；完成判定：记录加载、生成、取消和续扫结果；环境不足时记录未执行原因，不反向阻塞 T105。
- [ ] T107（T105；仅在已有对应硬件时可选）分别观察 CPU/GPU、资源和 GPU→CPU 回退；完成判定：形成本机观察记录，不把单一硬件结果宣传为通用支持证明。
- [ ] T108（T105；后续独立变更，不在本次实施）另行评估并接入新 OCR/VLM；完成判定：先冻结新模型资产/runtime/adapter 兼容性和质量范围，再复用本方案扩展点；本任务不是 Qwen 兼容承诺，也不是当前重构验收项。
