# PDF 内容感知处理 v0.2 详细开发计划

状态：Draft  
日期：2026-08-24  
依据：`docs/requirement/pdf-content-aware-processing-v0.2.md`  
迭代范围：仅交付需求第 16 节 P0“安全的页级双路径”

## 1. 迭代结果

本迭代完成后，`ocrx scan <PDF>` 默认先逐页分析 PDF，再按页执行以下确定性路径：

- 可靠且能达到任务有效质量层级的文字页：直接提取原生文字，不加载 OCR 模型；默认层级为 L2，用户显式要求 L1 时按 L1 判断；
- 图片页、混合页、不确定页以及原生提取校验失败页：沿用现有全页渲染和 OvisOCR2 OCR；
- `--pdf-mode text`：只允许原生提取，不合格页明确失败；
- `--pdf-mode ocr`：完整保留当前全页 OCR 行为，用作兼容与质量对照。

两条路径写入同一版 `PageDocument`、page checkpoint、`document.md` 和 `result.json`。结果能够说明每页的分类、计划方式、实际方式、回退原因、耗时，以及任务是否加载过 OCR 模型。

## 2. 严格范围

### 2.1 本迭代必须完成

1. PDF 页面对象分析、确定性分类与版本化阈值配置。
2. P0 原生文字提取：Unicode、L2 block、阅读顺序、方向、归一化 bbox 和来源；同一输出 schema 支持 L1 按较低结构要求验收。
3. 原生提取后校验，以及 `auto` 回退 / `text` 失败。
4. `auto|text|ocr` 三种模式和 OCR 模型延迟加载。
5. `--quality-tier <l1|l2>`，默认 L2；路由和提取后校验使用任务有效层级。
6. 现有串行扫描、原子提交、checkpoint、恢复、进度和结果报告适配。
7. 固定回归集、质量评分、路由门禁和性能基准。

### 2.2 本迭代明确不做

- 不做单页内区域 OCR、跨来源 block 合并或去重；P0 的 `mixed` 页整页 OCR。
- 不承诺 L3 行内样式、链接关系、公式 LaTeX、HTML 表格或完整视觉对象关系。
- 不引入 LLM 文本修复、联网分类、第二 OCR 模型或并行扫描框架。
- 不重写现有 OCR adapter、模型管理、PDF worker 或 Markdown 输出体系。
- 不以通用插件系统、策略引擎或未来扩展为由新增抽象；只提取本迭代确有两个实现的最小端口。

## 3. 项目现状与改动落点

| 现状 | 本迭代落点 |
| --- | --- |
| `ocrx-pdf` 已封装 PDFium、页数查询、逐页渲染和 worker 渲染，但没有页面对象分析与文字提取 | 在该 crate 增加逐页 `analyze_page` 和 `extract_native_page`；继续复用同一个 PDFium binding 与坐标规范 |
| `PageDocument` schema v1 只有 blocks、termination、warnings；block 没有统一 bbox/来源 | 在 `ocrx-core` 升级 schema，加入分类、处理元数据和满足 L2 所需的文字 block；保留现有 OCR block 表达并完成 round-trip/golden |
| `ScanService` 在任务开始时无条件加载模型，并固定执行 render → recognize | 保留它作为最终 OCR 队列的执行器；在调用前先完成原生候选页的提取与校验，最终 OCR 队列非空时才构造并运行它，继续复用现有 OCR、提交和失败汇总逻辑 |
| CLI 在模型校验、安装和扫描前就要求 OCR 资产可用 | 在 `ocrx-cli` 把 PDF 分析放到模型处理之前；仅实际需要 OCR 时执行现有模型校验/安装/初始化 |
| checkpoint v4 的兼容键固定包含 model/profile/parameters | 升级 checkpoint；公共兼容项记录输入、PDF mode、分类器和提取器版本，OCR 项只约束实际 OCR 页 |
| `result.json` schema v3 固定输出 OvisOCR2 模型对象 | 升级结果 schema；纯原生任务允许 model 为 `null`，补充分类、方法、回退和分阶段耗时 |
| 当前 CLI 没有独立的 L1/L2 请求参数，模型 profile 也不能代表内容能力层 | 增加 `--quality-tier l1|l2`，默认 L2；将有效层级作为路由、校验、checkpoint 和报告输入 |

不新增 workspace crate。PDF 专用逻辑留在 `ocrx-pdf`，稳定领域枚举留在 `ocrx-core`，任务编排和持久化继续使用 `ocrx-output`，CLI 只负责参数、资源决策和报告组装。

## 4. 实施顺序

每个工作项遵循 Red → Green → Refactor：先写最小失败测试，再实现该项所需代码，最后只清理本项产生的重复。每个合并点均运行相关 crate 测试；阶段结束运行 workspace 门禁。

### 阶段 0：冻结基线与验证 PDFium 能力

目的：在写领域契约前确认当前 `pdfium-render = 0.9.3 / pdfium_7881` 能提供哪些真实对象信息，避免计划依赖不存在的 API。

任务：

1. 记录当前 `--pdf-mode ocr` 尚未新增时的等价 CLI 行为：选页、输出文件、checkpoint、`result.json`、退出码以及模型加载次数。
2. 用最小受控 PDF 验证当前 binding 可读取：字符 Unicode、字符/文字段 bbox、页面旋转、字体信息、文字渲染可见性，以及页面图片对象 bbox/尺寸。
3. 把验证结果写入 `docs/develop/pdf-native-extraction-spike.md`，明确每个分类信号的数据来源和无法直接取得的信号。
4. 若现有 binding 缺少 P0 必需能力，只允许在 `ocrx-pdf` 内评估一个最小补足方案；在方案确认前不改 CLI 和 schema。不得借此引入与 P0 无关的 PDF 解析能力。

验收：

- 一页原生文字、一页扫描图、一页扫描图叠加隐藏文字的 fixture 能输出可复核的原始对象统计；
- 明确 P0 的每个输入信号是“直接取得”“确定性推导”还是“本期不可用”；
- 现有 OCR 基线测试和输出样本已保存，后续 `ocr` 模式必须逐项对照。

### 阶段 1：冻结领域契约与配置

目的：先固定分类、处理和来源的机器契约，使分析、执行、checkpoint 和报告共用同一语义。

改动：

1. 在 `ocrx-core` 增加最小稳定类型：
   - `PdfMode { Auto, Text, Ocr }`
   - `QualityTier { L1, L2 }`
   - `PageType { Text, Image, Mixed, Empty, Uncertain }`
   - `RouteConfidence { High, Medium, Low }`
   - `ProcessingMethod { NativeText, Ocr, Empty }`
   - 需求第 12 节 reason code 枚举
   - `PageClassification` 与 `PageProcessing`
   - `BlockSource { PdfText, PdfImage, Ocr, Derived }`
2. 升级 `PageDocument` schema。新增分类和处理元数据；为原生 L2 文字 block 表达逻辑文字、block 类型、bbox、方向、旋转、顺序和来源。L1 使用同一 schema，但允许仅属于 L2 的字段留空或使用不臆造语义的粗粒度 block。现有 OCR adapter 的 block 不做无关重构，只补齐统一来源和必要默认值。
3. 在 `ocrx-pdf` 增加版本化的 `ClassifierConfig`。首版阈值先通过 fixture/回归集校准，再以常量或受版本控制的数据文件冻结；不新增用户可调参数。
4. 为全部枚举、schema v2 JSON、未知 reason code 兼容策略、bbox/顺序不变量建立 round-trip 与 golden 测试。

验收：

- 原生页、OCR 页、空页、回退页四个 golden 可稳定序列化和反序列化；
- `PageDocument::validate()` 拒绝无来源、无效 bbox、重复/缺失顺序和计划/实际方式矛盾；
- OCR adapter 的既有 fixture 迁移后内容语义不变；
- schema 升级策略明确：旧 page/checkpoint 不静默复用。

### 阶段 2：PDF 页面分析与分类

目的：不渲染高 DPI 页面、不接触 OCR 模型，完成所有选中页的分类计划。

改动：

1. 在 `ocrx-pdf` 定义内部 `PageSignals`，只包含需求第 6.1 节且阶段 0 已证实可取得的信号：页面几何、可见/不可见文字统计、Unicode 有效率、文字 bbox/覆盖、图片 bbox/像素/覆盖、重叠、重复和几何异常。
2. 实现 `analyze_page()`：一次只持有当前页对象，输出统计，不生成高 DPI RGB 位图。
3. 实现纯函数 `classify_page(signals, config)`，输出类型、置信度、推荐方式、reason codes、关键统计和 `pdf-route-v1`。
4. 实现文档汇总：逐页结果保持原页码；所选页使用多种主要路径时文档类型为 `mixed`。
5. 分类事实与执行准入分开计算：页面对象信号先形成分类，再结合 `required_quality_tier` 形成计划。空页直接计划为 `Empty`；`mixed`、`uncertain` 在 P0 一律计划为 OCR。

测试顺序：

- 先用合成 `PageSignals` 做决策表单元测试，覆盖每个 reason code 和阈值边界；
- 再用真实 PDF fixture 做组件测试：短文本封面不能只因字符少变图片页、全页扫描图、可靠原生页、隐藏/错位/重复文字层、乱码/缺 ToUnicode、原生与扫描页交替、空白页、旋转页；
- 断言分析路径未调用 renderer、模型仓库或 runtime。

验收：

- 每个选中页都有且只有一个计划；
- 相同输入与分类器版本输出字节级稳定；
- “不得直接提取集”不存在直接交付决定；低置信度统一偏向 OCR；
- 分析 100 页时逐页释放对象，RSS 不随已完成页数线性累积。

### 阶段 3：P0 原生文字提取与校验

目的：将可靠文字页直接转换为统一页面文档，并按任务要求的 L1 或 L2 在交付前拦截不完整结果；默认和发布门禁仍为 L2。

改动：

1. 在 `ocrx-pdf` 实现字符事实提取：Unicode、bbox、方向、旋转、字体/字号证据、可见性和源对象标识；不可见文字不进入正文。
2. 以确定性规则完成：同位置精确重复消除、行聚合、断词处理、block 聚合、多栏阅读顺序和 L2 block 分类（标题、段落、列表、引用、预格式、页眉/页脚/页码、脚注）。
3. 只输出能够可靠确定的结构。公式、表格或重要视觉内容按有效层级判断：L1 能忠实保留全部可见字符、页面事实和自然阅读顺序时可使用粗粒度原生结果；L2 结构不能完整表达，或任一层级会遗漏/混淆必达内容时，返回 `native_structure_unresolved`，不得输出不满足该层级的成功页。
4. 实现独立 `validate_native_page(required_quality_tier, analysis, document)`：公共校验检查空/乱码、字符数量偏差、覆盖缺失、重复/重叠和基本阅读顺序；L2 额外检查 block 边界/角色、方向、bbox 和结构顺序。不得因只属于 L2 的字段缺失使 L1 失败。
5. 校验通过才将 `actual_method` 设为 `NativeText`；失败返回稳定 reason code，交给执行层决定回退或失败。
6. 复用现有 Markdown 生成和 page 原子提交；原生页不保存全页渲染 PNG，除非既有最终资产规则明确要求某个已提取视觉资产。

测试顺序：

- 字符归一化、连字/组合字符、软连字符、精确重复和旋转的单元测试；
- 单栏、多栏、列表、页眉页脚/脚注的 block 与阅读顺序 golden；
- 无效 Unicode、异常 bbox、缺块、公式/表格/重要视觉内容的校验失败测试；
- 同一页面“L1 通过、L2 不通过”的分层校验测试：L1 必须保留全部可见文字和页面事实，L2 因结构不足失败；
- 原生 page JSON → Markdown 的一致性测试。

验收：

- 固定“原生文字合格集”全部页面可生成有效 `native_text` 页面；
- 每个文字 block 均有 bbox、方向、顺序和 `pdf_text` 来源；
- 提取器不改写原文，不调用 renderer/OCR/runtime；
- 不合格结果不能被标为成功。

### 阶段 4：双路径扫描编排与最终 OCR 队列

目的：把分析和原生提取接入现有扫描流程，同时完整保留 OCR 路径。

技术决策：采用“原生候选页先提取校验，最终 OCR 队列后执行”，不采用执行循环内临时初始化模型。原因是当前模型路径不仅包含 `ModelLoader::load()`，还在 CLI 中包含 profile 推荐、资产路径解析、完整性校验、离线判断和可能的交互安装；把这些步骤塞进页面循环会扩大改造面并复杂化错误退出。两阶段方案可以在模型准备前确定包括动态回退在内的全部 OCR 页面，随后原样复用当前“一次加载、串行 OCR”的生命周期。代价是执行顺序分成两个阶段，但 page artifact、诊断和最终 Markdown 始终按原页码汇总，用户结果顺序不变。

改动：

1. 在 `ocrx-cli` 增加 `--pdf-mode <auto|text|ocr>`（默认 `auto`）和 `--quality-tier <l1|l2>`（默认 `l2`）；更新 help、typed options 和参数错误测试。`l3` 或其他值必须在 PDF/模型调用前拒绝。
2. 调整 `run_scan` 和应用服务的执行顺序，明确采用“两阶段执行”，不依据初始分类计划提前决定是否加载模型：
   1. 输入/PDFium/页码/输出策略检查；
   2. 确定任务有效质量层级，分析全部选中页并据此形成计划（`ocr` 模式可直接形成全 OCR 计划，但仍需页数/选页校验）；
   3. 复用兼容 checkpoint；
   4. 第一阶段按原页码处理所有尚未完成的原生候选页：提取、按有效层级校验；成功页立即使用现有原子提交能力写 page artifact/checkpoint，失败页在 `auto` 下原子写入 `complete=false`、`planned_method=ocr`、`fallback_from=native_text` 和回退原因的页面 checkpoint 后加入最终 OCR 队列，在 `text` 下记录失败；
   5. 初始计划为 OCR 的尚未完成页与第一阶段新产生的回退页合并为最终 OCR 队列，按页码去重、升序；队列只保存页码、分类和回退原因，不持有 PDF 页面对象、文字对象或位图；
   6. 仅当最终 OCR 队列非空时，执行现有模型推荐、资产校验/安装，构造 renderer/loader/recognizer，并调用现有 `ScanService`；模型在 OCR 阶段开始时加载一次，处理整个最终队列；
   7. 合并两个阶段的诊断与计数，按原 PDF 页码汇总 Markdown 和结果。
3. 不把原生分支塞进当前要求 `ModelLoader` 的 `ScanService`，也不为本期引入通用懒加载容器。对 `ocrx-output` 做最小改造：
   - 提取原生与 OCR 两阶段共用的 page 原子提交入口；
   - 将当前 `ScanService` 中“处理传入页”和“生成最终 `document.md`/清理全页图片”拆开，OCR 阶段只处理最终 OCR 队列，不得用 OCR 子集提前覆盖最终文档；
   - 两阶段结束后，由统一 finalize 入口使用本次全部选中页检查终态、按原页码合并 Markdown，并执行既有图片清理规则；
   - 外层只合并两个阶段的 `JobSummary`/diagnostics，不建立第二套文件协议或页面状态定义。
4. 模式规则：
   - `auto`：提取校验失败即把同一页转入现有全页 OCR，并记录 `fallback_from`/原因；
   - `text`：任何非文字计划或校验失败页标记失败，不初始化模型；
   - `ocr`：所有非空页走现有 OCR，输出内容与阶段 0 基线一致。
   - L1：只要求文字忠实、页面事实完整和自然阅读顺序；标题角色、列表嵌套、精细 block 边界或方向元数据等 L2 缺失不触发 OCR。
   - L2：执行完整 P0 block 结构准入和校验。
5. 若最终 OCR 队列非空但 OCR 资产不可用：保留第一阶段已经原子提交的原生页，将最终队列中的页面逐页记录为 `ocr_required_but_unavailable`，任务返回 partial/失败；不得因模型准备失败丢失回退原因。`--resume` 后复用兼容原生页，并从兼容的未完成 checkpoint 直接恢复已确定的 OCR 计划；无需重复原生提取和校验。
6. 取消检查同时存在于两个阶段。第一阶段取消后不得开始模型准备；OCR 阶段取消沿用现有 `ScanService` 语义。最终合并只在全部选中页达到允许的终态后执行。

关键集成测试：

- 纯文字 + `auto --offline` + 空模型缓存成功，模型 loader 调用 0 次；
- 纯文字 + `text` 成功，模型相关字段为空；
- 扫描件 + `text` 明确失败且 OCR 调用 0 次；
- 页级混合文档只渲染/识别扫描、mixed、uncertain 或回退页，模型只加载一次；
- 原生提取后校验失败在 `auto` 下同页回退，在 `text` 下失败；
- 初始分类计划没有 OCR 页、但至少一页原生提取后校验失败时，`auto` 必须在第一阶段结束后形成非空 OCR 队列，完成模型准备并只加载一次模型；
- 上述动态回退场景在模型不可用时，已成功的原生页保留，回退页标记 `ocr_required_but_unavailable`，结果为准确的 partial/失败，不能标为原生成功；
- 文字忠实但只有 L2 结构不足的页面，在 `auto + l1` 下原生成功，在 `auto + l2` 下回退，在 `text + l2` 下失败；
- 初始 OCR 页与提取后回退页合并时按页码去重；最终 Markdown 仍按原页码排序，不依赖两个阶段的执行顺序；
- `ocr` 模式的页内容、图片资产、checkpoint 行为和退出码与基线一致；
- 第一阶段取消不会触发模型检查、下载或初始化；OCR 阶段取消和页面失败继续沿用现有语义。

验收：

- 纯原生任务在代码路径和 spy 断言上均无模型文件读取、下载、loader/runtime 初始化或高 DPI 全页渲染；
- 混合任务不为原生页渲染高 DPI 位图；
- 初始 OCR 计划和动态回退共同决定最终 OCR 队列，模型是否准备只以该最终队列是否为空为准；
- 模型不可用时不把需 OCR 页伪装成原生成功，且已提交原生页可恢复。

### 阶段 5：checkpoint、结果与进度契约

目的：使两条路径可恢复、可审计，同时保持 stdout/stderr 纪律。

改动：

1. checkpoint 升级为新版本，记录：`pdf_mode`、`required_quality_tier`、分类器版本、提取器版本、页面分类、计划/实际方式、回退原因；OCR 页另外记录现有 model/profile/runtime 参数。允许原子持久化 `complete=false` 的“已完成分析/校验、等待 OCR”状态，但它不能被计为完成页或参与最终 Markdown 合并。
2. 将 checkpoint 兼容判断拆成共同项和按实际方式生效的项：
   - 输入 hash、schema、模式、分类器/提取器版本影响原生页；
   - 有效质量层级变化必须重新判断页面是否满足要求，不复用层级不兼容的成功结论；
   - model/profile/OCR 参数只使 OCR 页失效；
   - 模式或分类版本变化不得错误复用旧页面。
   - 兼容且 `complete=false, planned_method=ocr` 的页面恢复到最终 OCR 队列；只有 `complete=true` 且 artifact 校验通过的页面才可直接复用为完成页。
3. `result.json` 升级并加入需求第 10.2 节字段。记录 `required_quality_tier`；纯原生任务 `model: null`、`ocr_model_loaded: false`；逐页诊断包含分类、计划、实际、回退、warning 和各阶段耗时。
4. 进度增加稳定阶段：`analyzing`、`extracting`、`rendering`、`recognizing`、`merging`。JSON/JSONL 只输出稳定枚举；正文不写日志。
5. 文本报告增加文档类型、页类型计数、处理方式计数、OCR 页数和回退页数；默认 `document.md` 不加入来源噪声。

测试：

- 新 checkpoint round-trip、旧版本明确拒绝、原生页不因本地模型变化失效、OCR 页因 profile 变化失效；
- “原生完成 + OCR 未完成”恢复、回退页恢复、模式/分类器版本变化重跑；
- 在第一阶段完成后、OCR 阶段开始前中断并恢复：兼容原生页不重复提取，动态回退页依据未完成 checkpoint 直接重新进入最终 OCR 队列且保留回退原因；
- L1/L2 切换时 checkpoint 不被错误复用；
- 纯原生、纯 OCR、页级混合、partial 四组 `result.json` golden；
- stdout 恰好一个 JSON 对象，进度仅在 stderr，日志不含整页正文。

验收：

- 仅查看 `result.json` 可判断任务分类、每页路径、模型是否加载、回退/失败页及原因；
- resume 不重复已校验且兼容的原生页，也不错误复用不兼容页；
- 最终 Markdown 页码、顺序、图片引用和 page JSON 一致。

### 阶段 6：固定门禁与发布验收

目的：用需求冻结的完整集合证明 P0 的路由正确性、L2 质量和效率，不以局部成功样本代替验收。

任务：

1. 冻结两个页级集合及 manifest：
   - 原生文字合格集不少于 30 页；
   - 不得直接提取集不少于 20 页。
2. manifest 固定源 PDF hash、页码、人工类型/允许路径、逻辑文字、L2 blocks、阅读顺序、方向、bbox 真值和基准版本。测试拒绝缺项、集合重叠和零分母。
3. 在现有质量工具或测试支持代码中增加本需求规定的确定性评分：文本 NED、阅读顺序、block precision/recall/F1、bbox 有效率/召回率、重复/空白/失败数、路由混淆矩阵。
4. 对完整合格集运行 `auto` 和 `ocr --model-profile quality`；不得只评分最终成功的原生子集。
5. 同机冷启动至少 5 次测量完整合格集的 `auto` 与强制 OCR，记录各次绝对耗时、中位数、峰值 RSS、输出大小和模型读取/初始化证据。
6. 对纯扫描和页级混合 PDF 运行回归，确认现有 OCR 质量和输出能力不退化。
7. 增加分层行为集：页面文字真值完整但 L2 结构真值故意不可可靠恢复；验证 `auto + l1` 原生交付、`auto + l2` 回退，以及 L1 下乱码/遗漏/重复/错序仍不能直接交付。

发布门禁直接采用需求第 14.3 节，不在计划中改写阈值：

- 合格集 100% 最终 `native_text`，0 回退/OCR/失败，且全部绝对质量与 OCR 相对质量指标通过；
- 不得直接提取集最终 `native_text` 错误放行数为 0；
- 合格集 `auto` 总耗时中位数不高于强制 quality OCR 的 50%，且 5 次均未加载/读取模型；
- workspace 测试、格式化、Clippy、CLI 输出契约和恢复测试全部通过。

上述正式发布门禁固定使用默认 `--quality-tier l2`。L1 分层行为测试是附加功能门禁，不替代或降低完整合格集的任何 L2 指标。

任何一项失败均保持需求状态为未发布；不通过降低阈值、删除困难页或启用 P1/P2 降低门槛。

## 5. 建议的提交拆分

每个提交都应可编译、相关测试通过，避免一个提交同时改完所有 crate。

1. `test(pdf): freeze native extraction capability fixtures`
2. `feat(core): add pdf routing and block provenance contracts`
3. `feat(pdf): analyze and classify pdf pages`
4. `feat(pdf): extract and validate native l2 pages`
5. `feat(cli): add pdf mode and content-aware scan planning`
6. `refactor(output): run native validation before final ocr queue`
7. `feat(output): persist pdf routing checkpoints and reports`
8. `test(pdf): add routing quality and performance release gates`

提交 2–7 均须包含对应 Red/Green 测试，不把测试集中推迟到最后一个提交。

## 6. 文件级工作清单

| 路径 | 计划改动 |
| --- | --- |
| `src/ocrx-core/src/lib.rs` | PDF mode、分类/处理/来源枚举，L2 文字 block，`PageDocument` schema 与校验 |
| `src/ocrx-pdf/src/lib.rs` | 页面信号采集、分类、原生提取、结构恢复、提取后校验；复用现有 PDFium 生命周期 |
| `src/ocrx-pdf/tests/` | 真实 PDF fixture 的分析、分类、提取、旋转/隐藏层/乱码/多栏测试 |
| `src/ocrx-output/src/lib.rs` | 两阶段任务汇总、两路径统一提交、最终 OCR 队列执行、checkpoint 与诊断汇总 |
| `src/ocrx-cli/src/main.rs` | `--pdf-mode`、`--quality-tier`、前置顺序、资源按需处理、结果/进度组装 |
| `src/ocrx-ovisocr2/src/lib.rs` | 只补充统一 block 来源/schema 适配，不改变识别逻辑 |
| `docs/test/` 或现有 benchmark 目录 | 两个冻结集合的 manifest、评分说明、发布报告；真实样本按现有授权规则管理 |
| `docs/develop/pdf-native-extraction-spike.md` | 阶段 0 PDFium 能力验证与实现选择记录 |

实现中若发现某项不需要改动，应删除该项，而不是为了符合清单制造变更。

## 7. 持续验证命令

每个阶段先运行受影响测试，阶段合并前运行：

```bash
cargo fmt --all -- --check
cargo test -p ocrx-core
cargo test -p ocrx-pdf
cargo test -p ocrx-output
cargo test -p ocrx-cli
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

真实模型、固定质量集和性能基准单独运行并保存报告，不让默认单元测试依赖网络或本地模型。跳过真实门禁只能报告为“未执行”，不能视为通过。

## 8. 完成定义

只有同时满足以下条件，本迭代才完成：

1. 第 4 节阶段 0–6 全部验收完成；
2. 需求第 14.2 节全部功能验收通过；
3. 需求第 14.3 节完整质量门禁全部通过；
4. 纯文字 PDF 在无模型、离线环境成功，且有零模型调用证据；
5. `--quality-tier l1` 的分层行为通过，且没有降低默认 L2 发布门禁；
6. `ocr` 模式与阶段 0 既有 OCR 基线无行为退化；
7. checkpoint 恢复、partial、取消和 stdout/stderr 契约通过；
8. 发布报告包含固定集合版本、命令、环境、逐项结果、性能原始值和未通过项；
9. P1/P2 功能未被作为 P0 默认行为引入。
