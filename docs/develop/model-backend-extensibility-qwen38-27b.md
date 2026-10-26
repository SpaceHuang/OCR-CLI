# 面向新模型切换的架构重构建议：以 Qwen3.8-27B 为例

**状态：** 架构调研建议，尚非 v0.1 实施承诺
**日期：** 2026-10-15

## 1. 目的与结论

本文基于当前 OCR-CLI 实现，梳理未来希望更容易切换 OCR/VLM 模型时需要调整的架构和机制。Qwen3.8-27B 用于说明重构后可能接入的新模型：它是视觉语言模型，支持图像/视频理解并生成文本，与当前 OCR 任务相关，但不是本次重构要接入的模型，也不能只替换权重文件。

结论：目标应先划出两个相对独立的子系统，再在子系统内部组织扩展点：

1. **推理平台（底层）**负责模型资产管理、设备/后端适配和通用推理任务执行，并提供独立的模型安装、校验、列表及设备诊断能力。它不理解 PDF、页码、页面分类、native-text 路由、checkpoint、Markdown 合并或扫描进度语义。
2. **业务应用（上层）**负责扫描流程：PDF 读取与分类、native/OCR 决策、页面队列、恢复、结果落盘与报告。它通过通用推理接口提交任务，并把通用推理结果解释为 OCR 业务产物。

两个子系统之间依赖窄小、稳定、与 PDF 扫描无关的推理契约。业务层生成任务 prompt，并作为任务输入提交；推理平台不决定业务意图。推理平台内部的模型协议 adapter 只负责把通用任务输入封装为模型所需协议并解码响应，不持有扫描 prompt。模型资产定义、运行时后端、协议 adapter 和 model + adapter + runtime 兼容绑定属于推理平台内部机制，不应让扫描业务进入推理模块。

Qwen3.8-27B 是视觉语言理解模型，不是 Qwen-Image 文生图扩散模型。当前 llama.cpp 绑定冻结在上游 commit 9e3b928；Qwen3.8 有较新的 GGUF/llama.cpp 使用记录，但尚不能据此认定这个冻结版本的加载、视觉编码、上下文、输出模板和后端行为都兼容。模型卡主权重为 Transformers/Safetensors 格式；llama.cpp 实际使用通常需要兼容版本的 GGUF 权重及视觉 mmproj。必须先在候选 runtime 上做真实模型冒烟和回归，再升级或解冻绑定。

## 2. 当前结构与耦合点

工作区 crate 边界见根目录 Cargo.toml。当前调用路径主要由 CLI 串接：扫描业务在 ocrx-cli::run_scan 中直接解析模型资产、选择设备、创建 LlamaMtmdRuntime，再调用 OvisOCR2 适配和输出层。ocrx-pdf、ocrx-output 边界已存在，但业务 orchestrator、模型目录/安装器和 runtime 具体类型仍由 CLI 直接耦合。

| 当前位置 | 已有职责 | 切换模型时的问题 |
|---|---|---|
| ocrx-core::ModelId | 模型标识与核心域类型 | 解析逻辑只接受 ovisocr2；清单、错误提示和兼容信息假设单一模型 |
| ocrx-models | OvisOCR2 profile、固定 revision/哈希/URL、缓存、下载、校验 | manifest、资产配对和 profile 是 Ovis 特定逻辑；没有 backend/adapter 组合解析 |
| ocrx-runtime | Runtime trait、llama.cpp/MTMD、设备后端和 telemetry | 请求含扫描页码；模型资产/设备策略和执行入口不够独立；业务层需直接依赖 llama.cpp runtime 具体类型 |
| ocrx-ovisocr2 | 固定 prompt、Ovis 输出解析、bbox 裁剪和 Markdown/JSON | 推理、响应协议、视觉区域策略与 Ovis 一体化；CLI 直接绑定该 crate |
| ocrx-cli::run_scan | PDF 路由、模型安装/验证、runtime 初始化、续扫与报告 | 扫描编排同时承担推理平台管理；依赖具体模型和 runtime 实现，替换推理 backend/model 会牵动业务流程 |
| ocrx-output | 检查点、结果提交、Markdown 合并、续扫 | compatibility key 是字符串参数，缺少可审计的 model/backend/adapter 结构化身份 |

硬编码例子包括 ModelId::AS_STR 和 parse、ocrx-models::community_manifest/community_asset_urls、RUNTIME_COMPATIBILITY.adapter_id、CLI 中的 ovisocr2 字面量、LlamaMtmdRuntime/OvisRecognizer 具体类型，以及 ocrx-ovisocr2 中的 prompt 和输出解释。

## 3. 目标架构：推理平台与扫描业务分离

```text
业务应用：PDF 扫描
  PDF 输入/分类 → native-text 与 OCR 路由 → 页面队列/取消/恢复
  → 调用 InferenceService → 把结果转换为页面 JSON/Markdown → checkpoint/报告
                         │
                         │ 通用 TaskRequest / TaskResponse
                         ▼
推理平台：模型与设备无关的任务执行能力
  ModelCatalog / AssetStore / Verifier
  → 模型安装、校验、列表 API/命令
  DeviceProbe / DeviceSelector / RuntimeFactory
  → 设备诊断 API/命令
  Model protocol Adapter + Runtime Backend
  → TaskResponse（文本、结构化数据、finish reason、usage）
```

依赖方向应由业务应用指向推理平台的公共 API；推理平台不能依赖扫描 crate，也不能引用 PageLifecycle、PdfPageDocument、PageClassification、扫描 checkpoint 或 Markdown 文档类型。推理平台内部仍可划分模型资产定义、运行时定义、adapter 定义和执行绑定，但它们都围绕“加载并执行模型任务”，不围绕 PDF 扫描。

推荐将公共接口放在独立的 `ocrx-inference-api`（或等价稳定 crate）中；推理平台实现可拆为 `ocrx-inference`、`ocrx-models` 和 backend crates；扫描应用服务由 CLI 上层 crate 组织，PDFium 与输出模块只被扫描业务使用。初期可以暂不拆 Cargo crate，但至少在模块/API/依赖图上形成此边界。

## 4. Qwen3.8 案例的目标请求链

切换 Qwen3.8 不应改变扫描业务的页面路由和 checkpoint 流程。业务层根据 OCR 场景生成 prompt，并把 prompt 与页面图像一起作为任务输入提交；推理平台解析模型资产、设备/runtime 和 Qwen3.8 adapter。Adapter 将业务提供的文本/图像输入映射成模型期望的消息格式，将模型文本响应交回业务层；OCR 结果的业务解释和页面产物生成留在扫描应用。Adapter 可包含模型协议必需的特殊 token/包装格式，但不能代替业务 prompt 或内嵌页面扫描策略。

```text
CLI scan
  → 扫描业务完成 PDF 分析与路由，决定此页需要 OCR 并构造图像任务
  → 业务生成 OCR prompt
  → InferenceService.submit(TaskRequest { prompt, page_image, generation })
推理平台：解析 Qwen3.8 + adapter + llama.cpp backend
  → 校验/安装资产，选择 CPU/GPU，加载模型
  → adapter 组织输入，backend 执行生成
  → 返回 TaskResponse（文本、结束原因、usage、错误）
扫描业务：把文本适配为页面产物并写 checkpoint/汇总文档
```

Qwen3.8 的资产 manifest 至少要区分文本主模型、视觉投影器/编码组件、量化/转换产物、来源 revision、转换工具版本和各文件校验和。tokenizer 若属于模型文件包或推理引擎必需资产，作为资产角色或 runtime 加载依赖记录；chat template 属于协议/adapter 配置，不放在资产 manifest。模型不必都恰好是一对 model + mmproj；用具名资产角色表达必需及可选组件。

## 5. 大模块划分

本节只定义业务系统的顶层模块，不预先把它们拆成过多小服务或 Cargo crate。推理平台作为独立子系统，通过公共推理 API 被扫描业务调用。

```text
ocrx 应用
├── CLI 与配置入口
├── 扫描应用
│   ├── 输入与页面计划
│   ├── 页面处理（原生文本 / OCR）
│   └── 扫描状态与结果输出
├── 质量评估与工具
│   ├── 样本集与标注管理
│   ├── 评估执行
│   └── 评分、回归与报告
└── 推理平台（独立子系统）
    ├── 模型管理
    ├── 设备与运行时
    └── 推理任务执行
```

依赖方向：`CLI → 扫描应用 → 推理平台公共 API`；质量评估与工具可调用扫描应用的处理能力，也可在独立模型评估场景下调用推理平台公共 API。扫描应用同时使用 PDF 和输出基础设施；推理平台不依赖 PDF、扫描状态或文档输出模块。模型管理和设备诊断命令由 CLI 暴露入口，但实际能力归推理平台所有，可脱离 scan 命令独立使用。

### 5.1 CLI 与配置入口

负责命令解析、配置来源优先级、输出格式和退出码映射。`scan` 命令调用扫描应用；`model list/install/verify/remove` 与 `doctor` 命令调用推理平台管理 API。CLI 不实现 PDF 处理、模型下载、哈希校验或硬件探测。

### 5.2 扫描应用

负责一次扫描 job 的完整业务语义：读取和校验 PDF，分析页面并规划 native-text/empty/OCR 路线，逐页执行，决定页面失败/取消/续扫行为，并生成最终文档和报告。其内部先按三个大模块组织：

| 子模块 | 职责边界 |
|---|---|
| **输入与页面计划** | 解析扫描配置；校验 PDF、密码、页码和输出策略；读取页面信号；结合 PDF 模式与质量策略形成有序处理计划。 |
| **页面处理** | 对 native-text 页面提取并校验原生文本；为空白页产生空结果；对 OCR 页面渲染图像，由业务策略生成 prompt，调用推理 API，再把响应解析为扫描领域的页面内容和视觉区域。 |
| **扫描状态与结果输出** | 管理页面生命周期、checkpoint/续扫、原子结果提交、图像裁剪产物、Markdown/JSON 汇总、报告和进度呈现。 |

输入与页面计划模块决定“每页走哪条路线”；页面处理模块完成单页工作；扫描状态与结果输出模块负责跨页持久化及最终交付。bbox 解释、截断处理和图片链接验证属于 OCR 扫描语义，由页面处理/结果输出合作完成，不下沉到推理平台。

### 5.3 推理平台

推理平台只负责通用模型执行能力，内部按三个大模块组织：

| 子模块 | 职责边界 |
|---|---|
| **模型管理** | 模型/资产列表与信息、manifest、安装、校验、移除、缓存和许可证/来源信息。提供独立管理 API，供 CLI 的 model 命令及其他应用复用。 |
| **设备与运行时** | CPU/GPU 设备发现和诊断、编译 backend 能力、设备选择、runtime/backend 创建、模型加载与实例复用。提供独立 doctor/diagnostic API。 |
| **推理任务执行** | 接受业务传入的文本/图像等任务输入、生成参数、取消令牌和进度 sink；经模型协议 adapter 与 backend 执行，返回通用文本/结构化响应、finish reason、用量、警告和错误。 |

业务 prompt 是请求输入，由扫描应用生成。adapter 只适配模型协议，不生成 OCR prompt；平台也不处理扫描页状态、bbox 文件或 Markdown。

### 5.4 质量评估与工具

质量评估与工具和扫描应用并列，不是单次扫描流程的内部阶段。它负责维护可追溯的评估样本及其人工真值、执行固定配置的评估任务、比较预测与标注、生成质量门禁和回归报告。典型内容包括样本来源与版本、页面/文本/bbox/阅读顺序标注、预测产物、CER/WER 或结构化评分、路由混淆统计及评估报告。

评估模块持有业务真值和评分标准；推理平台只提供待评估的模型/设备/runtime 能力，不知道哪份标注是正确答案。评估执行需要完整 PDF 扫描质量时，应复用扫描应用，确保走与用户相同的 PDF 分析、路由和产物流程；若专门测量推理模型协议或 runtime，则可直接调用推理平台 API，但仍由评估模块提供 prompt、样本输入和评分规则。

### 5.5 两个子系统之间的业务调用流程

```text
CLI scan
  → 扫描应用校验 PDF 并创建页面计划
  → 页面处理选择 native-text、empty 或 OCR
  → OCR 页面由业务生成 prompt，渲染图像并调用推理 API
  → 推理平台确保模型/设备/runtime 可用并执行 TaskRequest
  → 页面处理把 TaskResponse 转为扫描领域结果
  → 扫描状态与结果输出提交页面 checkpoint 并汇总文档
```

模型安装、校验、列表和设备诊断走独立路径：`CLI model/doctor → 推理平台管理 API`，不创建扫描 job，也不接收 PDF。

质量回归走独立路径：`CLI eval/tool → 质量评估与工具 → 扫描应用（端到端评估）或推理平台（模型/runtime 评估）`。真值、标注和评分逻辑始终由质量评估模块拥有。

### 5.6 跨模块稳定契约

- 推理 API 的 TaskRequest 包含业务提供的 prompt、图像/其他模态输入及生成参数；不包含 PDF 页面分类、输出目录或 checkpoint 路径。
- 推理 API 返回通用 TaskResponse 和通用执行 telemetry；扫描应用把其映射为页面状态和面向用户的扫描进度。
- 推理平台的模型身份、资产 hash、runtime/backend 和 adapter 协议身份，与扫描应用的输入 PDF hash、渲染参数、prompt 版本共同组成扫描 checkpoint 兼容身份。
- 推理平台不依赖 `ocrx-pdf`、`ocrx-output` 或扫描应用；扫描应用不依赖 llama.cpp binding、具体 runtime 类型或模型缓存实现。
- 安装/校验/设备诊断属于推理平台管理能力；扫描流程仅通过 API 查询或请求服务就绪，不能复制实现。
- 样本真值、标注、质量指标和回归门禁属于质量评估与工具；推理平台仅返回推理结果，不拥有业务正确性判断。

顶层模块先形成逻辑边界，不要求马上按表格逐项新建 crate。实施时先把 CLI 中的扫描 orchestrator 和推理平台调用拆开，稳定跨边界契约，再依据依赖和发布需要决定 crate 拆分。

## 6. 分阶段路线

### 阶段 0：记录现状与重构基线

- 盘点当前样本集、真值标注、评分脚本和回归报告，确认质量评估模块需承载的现有能力及格式。
- 记录当前 OvisOCR2 的扫描行为、输出/checkpoint 格式、模型管理命令、设备诊断和质量回归结果，作为重构前基线。
- 定义重构完成的行为保持与模块边界验收条件；本阶段不要求下载或运行第二模型。

### 阶段 1：建立顶层模块边界，不增加第二模型

- 从 `run_scan` 抽出扫描应用入口，按输入与页面计划、页面处理、扫描状态与结果输出三个大模块组织现有逻辑。
- 将模型列表/安装/校验/移除和设备诊断从 scan 流程移到推理平台管理 API；CLI 命令只作为入口与展示层。
- 建立推理平台公共执行 API，隔离 CLI/扫描应用对 `LlamaMtmdRuntime` 的直接依赖。
- 维持 OvisOCR2 唯一模型行为与输出格式；明确 Ovis prompt 属于扫描应用，消息封装/响应协议属于 adapter。
- 从扫描应用中划清质量评估与工具的模块边界，盘点并迁入/包装既有样本、标注、评分、回归与报告能力；不改变评估语义。
- 将扫描 checkpoint compatibility identity 改为结构化数据，由扫描应用维护，并纳入推理平台返回的模型/runtime/adapter 身份及扫描业务的 prompt 版本。
- 明确评估执行有两种调用模式：端到端评估复用扫描应用；runtime/model 专项评估调用推理 API。样本真值和评分始终留在评估模块。

### 阶段 2：稳定推理平台内部扩展点

- 抽象模型资产定义、资产组 manifest、设备/backend 能力、runtime factory、model protocol adapter 和兼容绑定。
- 让现有 OvisOCR2 路径通过新边界运行，确保管理面与扫描业务互不依赖。
- 让质量评估与工具通过稳定的扫描应用或推理平台 API 执行，不复制扫描流程或资产/设备管理实现。
- 通过 fake runtime 和受控资产测试验证契约，不在此阶段默认引入第二模型。

### 阶段 3：重构验收与冻结

- 以现有 OvisOCR2 端到端跑通扫描应用、推理平台管理/执行 API 和质量评估工具。
- 对比重构前后 OvisOCR2 的页面路由、Markdown/JSON、图像产物、checkpoint/续扫和错误语义，确认重构没有悄悄改变业务行为。
- 确认质量评估可以固定模型资产、runtime、adapter、prompt 和评估集版本独立复跑，并产出可追溯报告。
- 验证依赖方向：推理平台不依赖扫描/评估；扫描应用不依赖具体 llama.cpp 类型；评估模块通过公开业务 API 复用流程。
- 满足上述条件后冻结重构契约，将后续模型接入作为独立变更规划。

### 阶段 4：后续独立接入 Qwen3.8（不属于本次重构）

- 重构验收完成后，再核实 Qwen3.8 权重/量化/视觉资产、目标 llama.cpp commit、平台后端和资源要求。
- 将 Qwen3.8 的 adapter/runtime/资产支持作为单独功能变更，复用已冻结的模型管理、推理任务 API、扫描应用和质量评估模块。
- 使用相同版本化样本和标注与 OvisOCR2 对照，评估文字、结构/阅读顺序、bbox、截断、路由和资源指标，再决定支持范围。
- 外部模型定义或第三方 adapter 插件仍需单独论证；不作为 Qwen3.8 接入或本次重构的默认前提。

## 7. 验收建议

- 推理平台不依赖扫描/PDF/output crates；扫描应用只依赖公共推理 API，不直接依赖 llama.cpp binding。
- 质量评估与工具拥有样本、标注及评分规则，可独立复跑并报告评估版本；端到端评估复用扫描应用，不复制扫描实现。
- 模型 list/install/verify/remove 和设备 doctor 可脱离 PDF 扫描独立运行，并通过同一推理平台 API 实现。
- 不改 PDF 分类、native/OCR 路由、checkpoint 业务即可通过配置切换到新的兼容 OCR/VLM 组合。
- Fake runtime 契约测试覆盖下载校验、adapter 转换、取消、截断、错误和 checkpoint 兼容；默认测试不下载真实模型。
- 真实冒烟覆盖模型加载、RGB 图像推理、停止条件、多页复用、CPU/GPU 和资源采样。
- 明确支持的平台、backend、GGUF 量化和最低内存建议；单一硬件报告不构成通用支持证明。
- 质量报告记录模型/mmproj hash、runtime commit、adapter/prompt 版本、渲染参数和逐页结果。
- 资产来源、许可证和转换 provenance 可追溯；缓存隔离，checkpoint 不跨模型或 prompt 版本复用。

## 8. 范围说明

这是一份未来架构建议，不修改当前代码，也不改变 v0.1 单模型发布约定。本次规划先完成不改变 OvisOCR2 行为的架构重构；Qwen3.8 是后续独立接入案例，不是重构验收前提，也不涉及文生图、视频生成或任意第三方插件执行。当前技术设计和 TDD 计划明确 v0.1 不接入第二模型、动态模型注册；若后续决定正式支持第二模型，需另行更新产品范围和验收计划。

## 9. 参考资料

- [Qwen3.8-27B 官方 Hugging Face 模型卡](https://huggingface.co/Qwen/Qwen3.8-27B)：模型形态、Safetensors 权重、视觉能力及 Transformers/vLLM/SGLang/llama.cpp 使用入口。
- [llama.cpp Qwen3.8 使用讨论](https://github.com/ggml-org/llama.cpp/discussions/27988)：社区使用 Qwen3.8 GGUF、mmproj 和 llama-server 的报告，不是完整兼容性承诺。
- [llama.cpp Qwen3.8 性能/上下文问题](https://github.com/ggml-org/llama.cpp/issues/27623)：提醒按版本、量化、硬件和上下文实测。
- 当前代码：src/ocrx-cli/src/main.rs、src/ocrx-core/src/lib.rs、src/ocrx-models/src/lib.rs、src/ocrx-runtime/src/lib.rs、src/ocrx-runtime/src/llama_mtmd.rs、src/ocrx-ovisocr2/src/lib.rs、src/ocrx-output/src/lib.rs。
- 当前范围约束：docs/develop/technical-design-v0.1.md、docs/plan/v0.1-tdd-development-plan.md。
