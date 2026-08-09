# OCR CLI v0.1 详细技术方案

状态：Draft  
对应需求：`docs/requirement/requirements-v0.1.md`  
适用版本：v0.1  
工作名称：`ocrx`

## 1. 方案目标与范围约束

本文给出 `ocrx` v0.1 的工程实现方案、模块边界、数据契约、关键流程、测试与交付计划。所有设计均以需求文档为唯一产品范围依据。

### 1.1 本期交付范围

- Rust CLI，v0.1 仅支持 macOS 14+ Apple Silicon；Windows 与其他平台延期，不纳入本版本发布承诺和冻结门禁。
- 仅支持模型 ID `ovisocr2`，在同一 Rust 进程内经 llama.cpp Rust binding 和 `libmtmd` 完成推理。
- PDF 逐页渲染、OCR、统一结构解析、Markdown 合并、插图 PNG 裁剪。
- OvisOCR2 四个 profile 的安装、校验、推荐、删除和状态查询。
- 可恢复的页面 checkpoint、终端/JSONL 进度、稳定 JSON 结果和退出码。
- `doctor`、配置、离线模式、安装脚本以及 Codex/Claude Code Skill。
- P0 模型、运行时、质量、资源和首发平台验证。

### 1.2 明确不实现

不实现 GUI、云 OCR、训练或微调、第二 OCR 模型、跨文档语义合并、人工编辑界面、原版式复刻、OCR 文本层回写 PDF、Linux 正式安装包、页面并发、语义化图片命名、图片 alt text 生成、Markdown pipe table、`ocrx clean`、遥测以及任何隐藏或实验性的其他模型入口。

内部的 adapter、model ID 和统一页面结构只是隔离 OvisOCR2 与共享流水线，不构成多模型用户功能。v0.1 的 manifest、注册表、命令输出、缓存写入和 Skill 均只能出现 `ovisocr2`。

### 1.3 已确认的技术基线

以下内容已有 macOS/P0 或 OvisOCR2 官方契约证据，可以作为 v0.1 开发基线，不再列为待确认项：

| 决策 | 已确认方案 | 证据/边界 |
|---|---|---|
| macOS 目标平台 | Apple Silicon arm64；当前开发基线按 macOS 14+ 编译 | M2/Metal 初始化、model/mmproj offload 和推理冒烟通过；最低 macOS 14 实机验证属于发布阶段的版本支持承诺，不阻断开发 |
| macOS Metal 构建 | 嵌入 Metal library | b10210 同 commit 构建和 CPU/Metal 主路径通过；正式 release 工件仍需复现 |
| OvisOCR2 adapter 基线 | `temperature=0`、关闭 thinking、官方 prompt 语义、`max_tokens=16384` | 官方模型契约和冒烟结果；质量阈值及清理参数仍属于质量验证 |
| OvisOCR2 图像约束 | 最小 `448×448`，最大总像素 `2880×2880`；超限按比例缩放并保持宽高比 | 官方模型契约；不同 DPI 下的质量选择仍不冻结 |
| bbox 坐标契约 | adapter 输出进入共享层前归一化到 `[0,1000)`；零面积和越界区域拒绝 | 官方输出格式和仓库基础不变量测试通过 |
| PDF 基础渲染契约 | PDFium 边界；默认 300 DPI；按页面尺寸输出 RGB8；不强制方形裁切 | `pdfium-render 0.9.3` + PDFium 7961 在 macOS arm64 的 fixture/真实 PDF 抽样通过；发布基线见 `docs/develop/pdfium-macos-release-baseline-2026-08-02.md` |
| PDFium binding 生命周期 | 进程内只初始化一次并由扫描任务复用 | `pdfium-render` 进程级 binding 限制已实测确认 |
| PDF 错误分类 | 不存在、非 PDF/损坏、加密/密码错误、动态库加载失败分别进入稳定错误类别 | 仓库 fixture、加密恢复和缺失动态库门禁通过 |
| 页面失败策略 | 默认继续处理后续页面；记录每页失败原因；任务最终为 `partial` 并返回退出码 `10` | 与需求建议一致；失败页进入 result/checkpoint，便于用户和 Agent 审计 |

以上是“接口和基础实现约束”的冻结，不等于已经冻结性能阈值、质量结果、Windows 发布能力或法律结论。

### 1.4 开发阶段与发布前待完成事项

以下项目尚未形成最终发布证据，但不再阻断项目启动。技术基线和用户可见原则已经确定；剩余工作分别在开发阶段实现、集成测试阶段验证，或发布前完成。只有明确标注“开发阻断”的项目，才需要在对应模块进入实现前完成。

| 决策 | 当前候选 | 还缺什么 | 为什么不能现在冻结 | 冻结产物 |
|---|---|---|---|---|
| PDFium 发布工程落地 | `pdfium-render 0.9.3 + PDFium 7961`，动态库随 macOS 归档发布 | 正式归档打包、签名和发布脚本 | 发布前门禁，不阻断开发；资源策略采用独立 worker，正式 CLI 资源曲线在实现后验证 | `pdfium-macos-release-baseline-2026-08-02.md`、打包检查脚本、发布归档 |
| Rust binding/runtime/llama.cpp 对齐 | macOS target 已接入 `llama-cpp-2 0.1.152` + `llama-cpp-sys-2 0.1.153`；M2 Metal 真实生成、结构化输出对比、EOS、max-token、协作取消和进度事件均已验证 | Rust 与 b10210 的跨 commit 逐字节一致性、后续 wrapper/sys commit 对齐；v0.1 不做 CPU fallback | v0.1 Metal runtime 契约已冻结；启动只做轻量 Metal 探测，runtime 延迟到首次 OCR，初始化失败直接返回可诊断错误 | runtime 兼容矩阵、结构化 golden；详情见 `docs/develop/runtime-macos-candidate-2026-08-02.md` |
| GGUF/mmproj 来源与分发 | 当前 macOS 开发采用固定社区 Q4 + BF16 mmproj 候选 | 已有 revision、文件大小、SHA-256 和 Apache-2.0 标识；正式发布前再补充转换 provenance、来源责任链和 NOTICE | 不阻断开发；属于正式生产模型的可追溯性和再分发审计 | 开发 manifest/fixture；生产阶段补充 provenance 记录、NOTICE/许可证包 |
| profile 量化映射与推荐 | Q4/Q5/Q8/BF16 初始映射 | 四 profile 同基准质量、显存/内存、磁盘和推荐阈值实测 | 不阻断核心开发；在 profile 推荐器和发布默认档位冻结前，不能宣称质量/资源门槛已定 | profile 兼容矩阵、推荐规则和质量/资源报告 |
| Skill 默认安装层级 | 用户级，显式支持项目级 | Codex/Claude 实际目录、覆盖保护、卸载边界和项目目录兼容性 | 仅阻断 Skill 安装功能；不阻断 OCR 核心开发 | Skill 安装矩阵和安装器契约 |

项目现在进入正式开发。接口和基础实现立即开发；集成契约在实现后测试冻结；发布工程、模型分发审计、质量/profile、macOS 14 支持承诺在发布前完成。页面失败策略已经确认，但其错误分类、result/checkpoint 字段和退出码仍需在实现阶段通过契约测试冻结。

截至 2026-08-02 的 macOS 验证结果见 `docs/develop/p0-validation-macos-2026-08-02.md` 和 `docs/develop/macos-p0-automation-2026-08-02.md`。仓库当前已建立 Rust workspace、7 个 crate、bootstrap CLI、基础测试 runner，并将 `pdfium-render` 0.9.3 + PDFium 7961 的渲染边界接入 `ocrx-pdf`；仓库内确定性 PDF fixture、加密/密码恢复、缺失动态库、PNG 落盘、worker 候选和真实教材抽样均已有验证入口。CLI 仍处于 bootstrap 阶段，manifest、真实 runtime、完整输出/恢复契约、安装脚本和 Skill 安装逻辑尚未实现。简单合成 PDF 的 RSS 采样未观察到初始化后的按页持续增长，但真实 190 页教材全页采样中 RSS 约从 52 MB 增长到 612 MB，资源稳定性因此仍是阻断。PDFium crate 接入和 binding 生命周期可作为 macOS 开发基线，但正式 PDFium 发布工件、真实 PDF 资源释放策略和完整 CLI 资源生命周期仍未冻结。llama.cpp/Rust runtime 对齐、完整质量回归、模型转换 provenance 和其他 profile 也仍未冻结。

## 2. 总体架构

采用单 workspace、多 crate 的分层结构。依赖方向只从接口层指向应用层，再指向领域与基础设施；共享流水线不得识别 OvisOCR2 原始 `<img>` 标签。

```text
ocrx CLI
  ├─ command handlers / output discipline
  └─ application services
       ├─ ScanService ───────────────┐
       ├─ ModelService               │
       ├─ DoctorService              │
       └─ SkillService               │
                                     v
       domain contracts: Job, PageDocument, OcrModelAdapter,
                         ModelManifest, ProgressEvent, AppError
              ┌──────────────┬──────────────┬──────────────┐
              v              v              v              v
         PDF renderer   llama/MTMD     model store    filesystem
                             │
                      OvisOcr2Adapter
                             │
                      unified PageDocument
                             │
              Markdown renderer + image cropper
```

建议仓库结构：

```text
Cargo.toml
src/
  ocrx-cli/          # 参数解析、命令路由、stdout/stderr 与退出码
  ocrx-core/         # 领域类型、应用服务、统一流水线
  ocrx-ovisocr2/     # 唯一生产 adapter、prompt、原始输出解析
  ocrx-runtime/      # llama.cpp binding、MTMD、设备与资源探测
  ocrx-pdf/          # PDF 验证、页数、逐页 RGB 渲染
  ocrx-models/       # manifest、下载、缓存、许可、兼容性
  ocrx-output/       # checkpoint、Markdown、PNG、result.json
  ocrx-skill/        # 规范 Skill 与安装目录适配
assets/
  manifests/ovisocr2/<revision>/manifest.json
  skills/ocrx/SKILL.md
scripts/
  install.sh
  install.ps1
benchmarks/          # 回归集清单、真值 schema、评分工具与报告
```

crate 拆分可在不破坏上述边界的前提下合并，但禁止让 CLI、PDF 或输出 crate 直接解析模型专用标签。

## 3. 核心领域模型与契约

### 3.1 统一页面结构

统一结构是 adapter 与共享流水线的唯一交界面。建议使用带版本的可序列化类型：

```rust
struct PageDocument {
    schema_version: u32,
    page_number: u32,
    blocks: Vec<PageBlock>,
    raw_output: String,
    termination: Termination,
    warnings: Vec<PageWarning>,
}

enum PageBlock {
    Markdown { content: String },
    Formula { latex: String, display: bool },
    HtmlTable { html: String },
    VisualRegion { bbox: NormalizedBbox, source_order: u32 },
}

struct NormalizedBbox { left: u16, top: u16, right: u16, bottom: u16 }
enum Termination { Eos, StopToken, MaxTokens, Invalid }
```

实现时可用更精细的 AST 表达阅读顺序，但必须满足：所有 block 有稳定顺序；公式、HTML table 和视觉区域可区分；bbox 在进入共享层前已转换到 `[0,1000)`；原始输出可写入 checkpoint 供异常诊断。`raw_output` 不进入默认日志或最终 Markdown。

### 3.2 Adapter 接口

```rust
trait OcrModelAdapter: Send + Sync {
    fn identity(&self) -> AdapterIdentity;
    fn capabilities(&self) -> CapabilitySet;
    fn validate_runtime(&self, runtime: &RuntimeInfo, manifest: &ModelManifest)
        -> Result<(), AppError>;
    fn image_constraints(&self) -> ImageConstraints;
    fn inference_defaults(&self) -> InferenceDefaults;
    fn build_request(&self, image: &MtmdImage, options: &ScanOptions)
        -> Result<InferenceRequest, AppError>;
    fn parse_output(&self, raw: &str, finish: FinishReason)
        -> Result<PageDocument, AppError>;
}
```

v0.1 仅注册一个静态 `OvisOcr2Adapter`。CLI 解析 `--model` 后立即验证值是否为 `ovisocr2`，验证发生在 manifest 查询、网络访问和 runtime 初始化之前。不得提供动态 adapter 加载、远程模型注册或第二 adapter。

### 3.3 Manifest

manifest 随 CLI 发布并纳入版本控制，远端内容不得改变其语义。最低字段：

- manifest schema、model ID/name/family、准确 generation/revision。
- adapter ID/version、prompt schema、output schema。
- llama.cpp commit、binding ABI、GGUF/mmproj 格式兼容范围。
- language GGUF 与共享 mmproj 的 URL、SHA-256、字节数、revision。
- tokenizer/chat template、stop token、temperature `0.0`、上下文和输出限制。
- 最小/最大像素约束与能力集合。
- 每个 profile 的量化、内存门槛、磁盘需求、质量验证状态。
- 来源、许可证 ID/文本链接、是否要求用户明确接受、再分发信息。

manifest 加载后进行 JSON schema/业务校验；model ID 必须等于 `ovisocr2`，能力必须完整，profile 只能为四个既定值，model/mmproj 组合必须由同一 manifest 明确配对。任何关键字段变化产生新 revision，不覆盖旧缓存。

### 3.4 任务与 checkpoint 标识

任务兼容键建议为以下字段的规范 JSON 后计算 SHA-256：

```text
input PDF fingerprint
selected pages and render DPI
model ID/revision/profile/quantization
mmproj revision
backend
adapter ID/version
prompt schema/output schema
effective image constraints and inference parameters
```

PDF fingerprint 至少包含规范绝对路径、文件大小、修改时间和内容 SHA-256；恢复安全性以内容 SHA-256 为准。`job.json` 保存任务兼容键、选页、状态和创建时间。页面 JSON 保存尺寸、缩放、结构化结果、终止原因、警告、输出文件校验值和完成标记。

## 4. CLI 与输出通道设计

使用成熟 Rust 参数解析库实现需求第 5 节的命令和参数，参数枚举在解析阶段拒绝非法值。仅实现需求列出的命令；`--json` 只添加到需求明确要求的 model list/info 及各命令已规定的 report 接口，不擅自形成新命令族。

### 4.1 通道纪律

| 内容 | stdout | stderr |
|---|---|---|
| `scan --report json` 最终对象 | 单个 JSON | 日志、warning、进度 |
| `scan --report text` | 空 | 总结、日志、进度 |
| `--progress jsonl` | 禁止 | 每行一个版本化 JSON 事件 |
| 交互进度条/plain | 禁止 | 进度和日志 |

命令 handler 构建一个 `Reporter`，只有它可以写 stdout；日志订阅器固定写 stderr。最终 JSON 在内存中完成序列化后一次写出，防止半个 JSON。失败时若命令处于 JSON report 模式，stdout 输出单个版本化错误对象；stderr 仍可给诊断，但 Agent 以退出码和 JSON 为准。

### 4.2 参数与前置校验顺序

1. 解析参数；拒绝非 `ovisocr2` 模型和互斥的 `--resume/--overwrite`。
2. 规范化输入、输出绝对路径；校验 PDF 和页码表达式。
3. 校验输出目录策略，防止覆盖和路径逃逸。
4. 合并配置并校验 token/context/DPI/profile/device。
5. 探测资源并选 profile/backend。
6. 检查本地模型；仅在允许时进入许可确认与下载。
7. runtime/model/mmproj/adapter 兼容性 preflight。
8. 创建任务并执行扫描。

这样可保证不支持模型、非法参数和离线缺模在昂贵操作前失败。

### 4.3 页码范围

解析 `1`、`1-5`、`1,3,8-12`，转换为去重、升序、闭区间页码集合。零、倒序范围、空段、超出 PDF 页数均返回 CLI 参数错误或输入错误，并给出具体片段和修正建议。

## 5. 配置与运行环境探测

配置按 `CLI > 环境变量 > 用户配置 > 内置默认` 分层合并，每个字段保留来源以便 debug 诊断。用户配置放平台标准配置目录；模型放平台标准缓存目录。环境变量使用统一 `OCRX_` 前缀，并仅覆盖需求列出的配置及 CLI 参数对应项。

资源探测输出：OS/arch、总内存、当前可用内存、缓存盘可用空间、编译进二进制的 backend、实际初始化可用性。`auto` 选择先排除未编译或初始化失败的 backend，再按平台优先：macOS Metal 后 CPU；Windows 可用已发布 GPU backend 后 CPU。显式 device 初始化失败返回 6；auto 失败可回退 CPU并记录 warning。

profile 推荐器只从已通过质量门槛且与 runtime 兼容的 OvisOCR2 profile 选择：可用内存低于 6 GB 拒绝；8 GB 档使用通过验收的 compact；24 GB+ 使用通过验收的 quality；reference 永不自动选择。16 GB 档在 balanced 通过时使用 balanced；balanced 未通过但 quality 通过时提升为 quality，并以 P0 实测结果同步修订 quality 的 16 GB 内存要求。实际资源阈值由 P0 实测冻结。显式 profile 低于最低加载门槛时拒绝，低于推荐门槛时 warning。

## 6. 模型管理

### 6.1 缓存布局

严格使用：

```text
<cache>/ocrx/models/ovisocr2/<revision>/manifest.json
<cache>/ocrx/models/ovisocr2/<revision>/mmproj.gguf
<cache>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf
```

下载临时文件放在目标文件同目录，以确保校验后原子 rename。临时名由程序生成，不接受模型输出或远端文件名作为路径。v0.1 不创建其他 model ID 目录。

### 6.2 下载事务

1. 从内置 manifest 解析固定 HTTPS URL、大小和 SHA-256。
2. 检查 endpoint/代理配置、offline、auto-download 和许可证状态。
3. 计算 model、mmproj、临时文件安全余量所需空间，空间不足提前退出 5。
4. 如许可证要求接受：交互式展示来源、大小和许可并记录同意；非交互式仅接受预先存在的对应 model/revision 许可记录。
5. 使用 `.part` 和元数据执行 HTTP Range 续传；服务端不支持或实体标识变化时从零重下。
6. 下载期间持续报告文件类型、字节、比例、速度和 ETA。
7. 流式计算并最终复核字节数与 SHA-256。
8. `fsync` 文件后原子 rename；随后原子写安装状态。
9. 失败删除不可续传或校验失败的临时文件；可续传文件必须仍带 `.part`，永不被 `verify` 视为已安装。

`--offline` 在下载器入口和 HTTP client 创建处双重阻断网络。endpoint 只能改变受信 manifest 中 URL 的主站映射，不得接受远端 manifest 替换哈希等信任字段。

### 6.3 子命令

- `model list/info`：仅从内置 OvisOCR2 manifest 和本地状态生成，JSON 包含需求规定字段。
- `model recommend`：只运行探测与推荐，不隐式安装。
- `model install [PROFILE]`：安装该 profile 配套 model 与准确 mmproj；缺省 profile 按 recommend。
- `model remove <PROFILE>`：只删指定 profile model；仅在不再被任何已安装 profile 使用时才可处理共享 mmproj，具体保留策略应避免破坏其他 profile。
- `model verify [PROFILE]`：重算哈希并检查 manifest、model、mmproj、runtime、adapter 配对。

## 7. PDF 扫描流水线

### 7.1 生命周期

```text
preflight → load model once → for each selected page serially:
render → resize → MTMD encode → generate → adapter parse/validate
→ repetition/anomaly handling → crop images → atomic checkpoint
→ release page image/tensors
then merge Markdown → write result.json → unload runtime
```

模型和 projector 每个任务只加载一次；页面图像、编码上下文和临时 tensor 每页释放。第一版不实现并发推理。

### 7.2 渲染与缩放

- `Pdfium` binding 在进程内只初始化一次；扫描任务复用同一个 renderer/binding，页面循环不得重复加载动态库。
- 真实 PDF 资源验证必须比较单文档逐页、分段重开和必要的显式释放策略；“每页重开 PDF”目前只是缓解候选，不得在未完成性能/内存回归前冻结。
- 当前真实 190 页教材对照观测：单文档逐页约 612 MB、每页重开约 288 MB、每 20 页重开约 331 MB 峰值 RSS；这些是当前机器和 fixture 的候选数据，不是产品阈值。页段大小必须由多 PDF 的资源/耗时回归冻结。
- 其余真实教材使用每 20 页重开时峰值约为 292–575 MB，文档内容差异明显；在资源增长来源和上限策略解决前，不得冻结统一页段或资源阈值。
- 对八年级下册，5/10/20/40 页分段的观测峰值约为 548/560/575/619 MB；分段策略未消除内容相关增长，后续必须定位 bitmap、页面内容和字体/图片缓存，而非仅调小分段。
- 已显式释放 PDFium bitmap 和 `PdfPage` 后，高 RSS 文档每 20 页重开仍约 576 MB；在资源策略冻结前，必须继续定位 PDFium 内部缓存或真实内容处理路径。
- 已启用 PDFium 图像缓存限制和渲染前清理开关，峰值仍约 576 MB；如继续使用该 PDFium 版本，应评估进程隔离/renderer 重启或替代实现。
- v0.1 正式采用独立 `pdfium-worker` 进程隔离真实 PDF 渲染资源。主进程按连续、半开页段 `[start,end)` 启动 worker；worker 只负责加载 PDFium、渲染 RGB8/300 DPI 页面并将结果写入该页段专属临时目录，完成后正常退出。主进程只有在 worker 退出码为 0、输出 PNG 数量等于 `end-start`、文件名/页码完整且无 `.part` 残留时，才校验并提交该页段 checkpoint。
- 页段默认 20 页；最后一段可小于 20 页。页段不得为空、倒序或越界。每个 PNG 先写同目录 `.part` 文件，成功后原子 rename；worker 异常退出、超时、被取消、输出缺失、数量不符、hash/PNG 校验失败时，主进程不得提交该页段，清理该页段临时目录并按页面失败策略记录失败。
- worker 不持有任务级 checkpoint，不合并 Markdown，不加载模型，不解析模型输出。主进程负责 worker 生命周期、超时/取消、输出校验、页面提交和最终审计。worker 每个页段独立进程退出，以回收 PDFium 内容、字体和图片缓存。
- 当前 macOS 验证已覆盖 190 页真实教材的 20 页分段、独立进程、PNG 交付、精确页数和无 `.part` 残留，以及非法页段拒绝。worker 协议和基础实现已冻结；页段大小的性能最优性和完整 CLI checkpoint/IPC 接入属于后续实现与性能验证，不改变隔离方案。
- 打开 PDF 时区分不存在、非 PDF、加密、损坏和渲染器不支持，映射退出码 3/7。
- 先取总页数，再按选页逐页渲染 RGB；默认 300 DPI。
- 记录 PDF 原始页面尺寸、目标 DPI 渲染尺寸、最终送模尺寸、横纵缩放比。
- 若像素总量超过 adapter 的 2880×2880 总预算，按 `sqrt(budget/(w*h))` 计算统一比例并保持宽高比；同时保证 manifest 最小约束。不得裁页面。
- 裁图始终使用缩放前的未压缩渲染位图；bbox 先映射到该位图坐标。
- 发生缩放写 page warning 和最终 warning；完成裁图和 checkpoint 后立即释放位图。

需求所述像素预算是总像素上限，不能误实现为强制 2880×2880 方图。`--dpi` 只改变初始渲染，不绕过 adapter 预算。

### 7.3 推理

adapter 固定 temperature 0、关闭 reasoning、构造 OvisOCR2 prompt/chat template，传入 effective context、最大图像 token 和最大输出 token。用户只能降低默认 16384 输出 token；若实现允许更高值将超出需求，故 v0.1 校验上限为 manifest 默认/安全上限。

生成循环向取消令牌轮询，至少每秒发出可见 heartbeat/page_progress。出现 EOS/stop 正常结束；达到 max token 标记 `truncated`；runtime 错误标记页面失败。任务处理中绝不切换 model/profile/backend。

### 7.4 解析、异常和重复处理

OvisOCR2 adapter 负责识别其 bbox 图片标签并转为 `VisualRegion`，共享层只处理统一 bbox。解析时检测：

- 空输出、无效/未闭合代码块、公式、HTML table。
- 图片标签格式、数字、坐标范围和面积。
- max-token 或缺失正常终止。
- 尾部周期性重复。

重复检测只裁除达到保守阈值、位于输出尾部且连续重复的片段；无法高置信判定时不删正文，保留原始输出并 warning。任何大量清理都必须 warning。页面失败或截断计入最终 `partial`；默认继续其他页面，并在页面 checkpoint 与最终 result 中记录失败类型、页码、原因和建议操作。

### 7.5 bbox 与裁图

对每个 `NormalizedBbox`：

1. 再次验证 `0 <= left < right < 1000` 和纵坐标。
2. 按未压缩渲染图宽高用一致的 floor/ceil 规则映射像素边界并 clamp。
3. 拒绝零面积和低于固定、经测试的异常小区域。
4. 对同页候选按源顺序处理；完全重复或 IoU 达到冻结阈值的高度重叠区域只保留第一个。
5. 只在 `<output>/images` 下用程序生成的稳定文件名写 PNG。
6. 原子保存后生成 `![Figure N](images/page-NNNN-figure-NNN.png)`。

模型提供的 `src` 仅用于 adapter 解析坐标，绝不用于路径拼接。无效 bbox 删除对应图片占位、保留正文并 warning。

## 8. 原子输出、恢复与中断

所有关键 JSON、Markdown 和 PNG 先写同目录临时文件，flush/必要时 fsync 后 rename。输出目录存在且包含结果时，默认失败；`--overwrite` 明确重建本任务结果，`--resume` 只复用兼容 checkpoint。实现删除前必须验证目标是用户明确指定且不是输入、缓存根、工作区根或文件系统宽泛目录。

页面完成顺序：写 PNG → page Markdown → page JSON（含前两者校验值与 `complete=true`）。因此 page JSON 是提交标记。恢复时校验 job 兼容键、页面 JSON schema、文件存在和哈希；任一失败只重跑该页。已完成页面不得因后续页失败而丢失。

Ctrl+C 设置取消令牌：停止继续取 token/页面，等待当前底层调用安全返回，删除当前页未提交临时文件，保留已提交 checkpoint，原子更新 job 状态，返回 130 并提示 `--resume`。第二次中断可立即退出，但仍不得把临时文件当作完成页。

最终 Markdown 按原 PDF 页码顺序合并，每页插入 `<!-- page: N -->`，UTF-8/LF，图片路径使用 `/`。只有所有选页终态确定后写 `document.md` 和 `result.json`。页面失败或截断不阻止后续页面；所有选页终态确定后，若存在失败或截断页，status 为 `partial`、退出 `10`，并在 result 中报告 `total/completed/failed/truncated` 及逐页失败明细。

## 9. 进度、日志与错误

内部统一 `ProgressEvent { schema_version, event, timestamp, job_id, page?, total?, stage?, elapsed_ms?, ... }`，渲染为 bar/plain/jsonl/off。阶段严格使用需求列出的名称；下载事件包含文件类型和传输统计。bar 只在 TTY 默认启用，非 TTY 默认 plain。

生成阶段用 1 秒定时 heartbeat 更新 elapsed/spinner；ETA 使用已完成页耗时的简单移动平均，样本不足时显示未知，不伪造精度。

`AppError` 包含稳定错误种类、退出码、用户消息、可能原因、建议操作、可选底层 source。底层 C/C++ 错误只进入 debug 日志，面向用户的错误必须有三段语义。退出码严格映射需求第 10 节；多个页面错误统一返回 10，初始化级错误仍返回其专用码。

默认日志不打印 prompt、OCR 正文、密钥、代理凭据或带查询参数的下载 URL。debug/trace 也要经过敏感字段脱敏。

## 10. `result.json` 与 JSONL schema

按需求第 8.4 节实现 `schema_version: 1`，字段不得缺失；路径经绝对化和词法/存在路径规范化处理。`status` 至少稳定区分 `completed` 与部分完成状态；错误对象格式在实现前形成 JSON Schema 并用 golden tests 冻结。

页面 checkpoint 额外记录：

- 页码、状态、耗时、图像数、警告。
- 原始页面尺寸、DPI 渲染尺寸、送模尺寸、缩放比例。
- 任务兼容字段或其哈希。
- raw output、统一页面结构、终止原因。
- page Markdown/PNG 的路径和 SHA-256。

JSONL 事件 schema 独立版本化；事件仅写 stderr。兼容性策略为 v0.1 内字段不改名、不改变类型，只允许后续增加可选字段。

## 11. Doctor 与 Skill

`doctor --report json` 复用实际资源探测、manifest 和模型校验组件，输出 CLI、平台、内存、backend、Metal/CUDA/Vulkan、OvisOCR2 安装/校验状态、推荐 profile、缓存路径/空间和 Codex/Claude Skill 状态。探测不得下载模型。

仓库只维护一份规范 Skill，再由安装器替换安装目录差异。Skill 内容只引用稳定 CLI/result 契约，包含需求第 12 节全部行为，明确：先 doctor、绝对路径、非交互调用、不覆盖、仅 OvisOCR2、按退出码恢复、选择性读取长 Markdown、本地处理和关键用途人工核验。

`skill install/uninstall [codex|claude|all]` 只管理由本版本安装且可识别的 Skill 文件。安装原子写入并保留来源/version 标记；卸载不得删除目录内用户的其他文件。默认安装层级和各宿主目录见第 1.4 节，冻结前不视为最终路径契约。

## 12. 构建、打包与平台策略

- Rust toolchain、Cargo.lock、llama.cpp commit、binding wrapper 与 sys crate 版本及 sys 内嵌 llama.cpp commit 全部精确锁定。只固定 `llama-cpp-2` 顶层版本不足以满足兼容性要求，因为兼容版本解析可能选择不同的 `llama-cpp-sys-2`。
- macOS arm64 工件编译 Metal；最低运行系统和版本支持范围在发布阶段通过 macOS 14 实机验收后承诺。
- Windows x86_64、CUDA/Vulkan 及其他跨平台工件不属于 v0.1；后续版本重新建立独立验证和发布支持矩阵。
- CI 对目标平台执行单元、集成、CLI contract 和安装烟测；GPU/真实模型测试在对应硬件 runner 执行。
- Release 提供二进制归档和 SHA-256 清单。`install.sh`/`install.ps1` 只下载匹配平台工件、验哈希、安装用户目录、提示 PATH，可选安装 Skill，不下载全部模型。
- CLI、runtime、manifest、权重和 Skill 各自版本化；升级不删除缓存模型，兼容性 preflight 决定能否继续使用。

## 13. 安全与隐私设计

- 默认网络 client 只在模型下载/显式更新路径创建；扫描本地模型不联网。
- 所有下载为 HTTPS，URL 和 SHA-256 来自受信内置 manifest；revision 不跟随远端主页漂移。
- 输入、页面、OCR 正文和统计不上传、不遥测。
- 输出目录使用 capability/根目录约束：裁图和内部文件只能由程序生成的相对路径 join，join 后校验仍位于 output root。
- HTML/Markdown 是不可信数据，只作为文本写入，不执行脚本、URL、命令，不主动抓取外部资源。
- 许可同意按 model ID + revision + license revision 独立记录；非交互模式不能代替用户同意。
- 配置及日志对 token、代理认证和 URL secret 做脱敏；result 不包含密钥。

## 14. 测试与验证方案

### 14.1 测试分层

| 层级 | 重点 |
|---|---|
| 单元测试 | 页码解析、配置优先级、profile 推荐、bbox 映射/去重、重复检测、路径约束、错误码 |
| 契约/golden | manifest、PageDocument、checkpoint、result.json、错误 JSON、JSONL、Markdown |
| 组件测试 | Range 续传、哈希失败、磁盘不足、许可、原子写、PDF 单页渲染、adapter 输出解析 |
| 集成测试 | fake runtime 跑完整多页流水线、失败继续、截断、Ctrl+C、resume、stdout 污染检查 |
| 真实模型 E2E | 锁定 OvisOCR2 在 macOS/Windows 的加载、MTMD、OCR、裁图、Skill 调用 |
| 稳定/资源 | 100 页 RSS 曲线、长生成 heartbeat、OOM 防护、下载中断、损坏 PDF |
| 发布验收 | 第 17 节全部功能、质量、稳定性门槛和安装包烟测 |

fake runtime 只用于快速验证共享流水线，不能替代真实模型发布验收。

### 14.2 P0 回归基线

在完整 CLI 开发前建立至少 30 页人工真值集，逐类满足需求覆盖与每类至少 2 页。版本化保存输入清单/允许分发方式、真值、评分工具、模型与 runtime 锁定信息、原始结果和报告。

评分严格只做 NFC、LF、行尾和多空行归一化；实现文本 NED、KaTeX 兼容公式完整率、TEDS、pairwise 阅读顺序、bbox 有效率/召回率、重复率、截断率和空白/失败率，并报告适用样本数。所有 profile 用相同输入、渲染、adapter、prompt、参数、后处理和随机种子。官方 Safetensors BF16 是实现参考，人工真值是最终依据。

四个 profile 均必须执行完整评分并保留逐项结果，但发布门禁按需求 17.2.4 判定：`reference`、`quality`、`compact` 必须达到各自阈值，且 reference 相对官方 BF16 的下降不得超过规定差值；`balanced` 达标时作为 16 GB 默认档，未达标时不单独阻断发布，而是由已通过的 `quality` 接替 16 GB 默认档，并同步修订 quality 的内存要求。不得为使任一 profile 通过而降低阈值。每次关键输入变更都升级 benchmark version 并全量重跑，禁止跨版本混算。

### 14.3 关键故障注入

- HTTP 断流、无 Range、ETag 变化、错误长度/哈希、代理失败、offline。
- 磁盘预检不足、写到一半失败、rename 失败。
- model/mmproj revision 不配、runtime ABI 不兼容、GPU 初始化失败并 auto 回退。
- 加密/损坏 PDF、某页渲染失败、空输出、循环输出、token 截断。
- bbox 越界/非数/零面积/极小/重叠、恶意 `src` 路径。
- checkpoint 被截断、内容哈希错误、输入或模型兼容键改变。
- stdout 捕获断言：JSON 模式恰好一个 JSON，所有进度/日志只在 stderr。

### 14.4 macOS 自动冻结门禁

除真实模型质量和跨平台验收外，macOS 本机可以自动冻结以下工程契约。它们必须在 CLI workspace 建立后进入可重复测试，而不是依赖一次性手工冒烟：

| 门禁组 | 必须自动证明的事实 |
|---|---|
| CLI/输出 | 参数前置失败、退出码、stdout/stderr 隔离、JSON/JSONL/schema/golden 稳定 |
| 文件安全 | 输出覆盖、路径逃逸、软链接、临时文件、原子写和 rename 失败安全 |
| 恢复/取消 | 兼容键校验、checkpoint 损坏拒绝、SIGINT 返回 130、已完成页保留 |
| PDF | 非 PDF/损坏/加密/密码错误分类，RGB8/尺寸/hash 确定性，动态库加载失败 |
| 资源 | 100 页 RSS 非线性增长、bitmap 释放、长生成 heartbeat、OOM 防护 |
| 模型事务 | Range/ETag/hash/大小错误、磁盘预检、`.part` 状态、offline 不联网 |
| 信任边界 | manifest 不可被远端替换关键语义，model/mmproj/revision/profile 配对严格 |
| 诊断与隐私 | doctor 不下载模型；日志/result 脱敏，不泄漏正文、凭据和 secret URL |

当前仓库已具备 Rust workspace、基础测试 runner、正式 `ocrx-pdf` crate 和仓库内 PDF fixture；但 CLI 仍是 bootstrap，manifest、真实 runtime、完整输出/恢复契约尚未实现。上述门禁中，PDF 基础渲染和简单 100 页 RSS 采样已有实现级证据，不能扩展解释为完整 CLI 资源稳定性或发布能力已经冻结。

## 15. 实施阶段与门禁

### 阶段 P0：可行性与质量冻结

1. 固定候选 llama.cpp/binding，跑通 `llama-mtmd-cli` 的 OvisOCR2 model + mmproj。
2. 建立回归集、真值和评分器，锁定官方 BF16 基线。
3. 比较 GGUF BF16/Q8/Q5/Q4、200/250/300 DPI、统一/不同送模像素预算。
4. 验证 bbox、重复/截断、Rust binding 与 CLI 一致性。
5. 在 Mac M2 Metal 完成稳定验证；Windows、CUDA/Vulkan 留待后续版本。
6. 冻结 PDF renderer、runtime commit/binding、模型来源/revision/hash/license、profile 映射和资源阈值。
7. 输出 ADR、兼容矩阵、逐项质量报告和 go/no-go 结论。
8. 在 macOS 执行 14.4 的自动冻结门禁，并将测试报告、fixture 版本和运行环境写入验证工件。

门禁：需求第 18 节任一失败则停止，不进入完整 CLI 开发。

### 阶段 P1：骨架与稳定契约

- 建立 workspace、错误/退出码、配置、CLI 命令骨架。
- 冻结 manifest、统一 PageDocument、result/checkpoint/JSONL schema。
- 实现 fake runtime 的串行流水线和 stdout/stderr 契约测试。

### 阶段 P2：模型与运行时

- 实现资源/backend 探测、推荐器、模型缓存/下载/许可/校验。
- 集成锁定 binding/libmtmd 和 OvisOCR2 adapter。
- 完成 model 子命令与兼容性 preflight。

### 阶段 P3：PDF 与输出

- 实现逐页 RGB 渲染、像素预算、推理循环、异常检测。
- 实现 bbox 裁图、Markdown、checkpoint、resume、Ctrl+C 和 result。
- 完成长页与 100 页内存验证。

### 阶段 P4：交互、Skill 与发布

- 完成 bar/plain/jsonl、doctor、日志脱敏。
- 完成规范 Skill、安装/卸载和 Agent E2E。
- 完成 macOS/Windows 安装脚本、Release 校验和平台烟测。

### 阶段 P5：发布验收

- 在冻结基准上全量重跑四 profile 和官方 BF16 对照。
- 执行功能、稳定、安全、恢复、离线、安装验收。
- 发布包含兼容矩阵、profile 原始评分、样本数、失败页、资源数据、默认选择及已知限制的报告。

## 16. 需求追踪矩阵

| 需求章节 | 技术落点 | 主要验证 |
|---|---|---|
| 3 平台 | runtime/backend、平台构建 | 双平台烟测与真实硬件 E2E |
| 5 CLI | `ocrx-cli`、Reporter | CLI golden、stdout/stderr 捕获 |
| 6 模型 | manifest、adapter、model store | 兼容/哈希/许可/profile 测试 |
| 7 PDF | ScanService、`ocrx-pdf` renderer、runtime | 多页串行、尺寸记录、异常输出、RSS 采样 |
| 8 输出 | output/checkpoint/cropper | schema、链接存在、bbox 安全 |
| 9 进度 | ProgressEvent/renderers | TTY/non-TTY、JSONL、heartbeat |
| 10 错误 | AppError/exit mapping | 故障注入与消息 golden |
| 11 配置 | layered config | 优先级与敏感信息测试 |
| 12 Skill | canonical Skill/installer | Codex、Claude 端到端调用 |
| 13 安装 | release scripts | OS/arch、哈希、无管理员安装 |
| 14 安全 | network/path/license boundary | offline、路径穿越、日志脱敏 |
| 15 性能 | 串行生命周期/资源释放 | 100 页 RSS 与基准设备报告 |
| 16 可观测 | doctor/logging | JSON schema、无正文日志 |
| 17 验收 | benchmark/release suite | 全量发布门槛 |
| 18 P0 | P0 实验与 ADR | go/no-go 报告 |

## 17. 完成定义

v0.1 只有同时满足以下条件才可发布：

- P0 所有硬门禁通过，技术选型、revision、哈希、许可和兼容矩阵已冻结。
- 需求列出的命令、参数、输出、错误码、恢复、进度、doctor 和 Skill 均有自动化或硬件验收证据。
- OvisOCR2 的 `reference`、`quality`、`compact` 达到各自质量阈值；`balanced` 已完成同基准评分，若通过则作为 16 GB 默认档，若未通过则由通过验收且内存要求已相应修订的 `quality` 接替，不因 balanced 单独未通过而阻断发布。
- macOS Apple Silicon 与 Windows x64 CPU 可稳定运行；声明发布的 GPU 工件均已验证，未验证能力不进入发布承诺。
- 100 页处理无随页数线性增长的内存占用，离线/下载/哈希/磁盘/PDF/bbox/checkpoint 故障均安全失败。
- JSON stdout 无污染，最终 Markdown 图片均存在，任何失败或截断正确返回 10。
- 发布工件、安装脚本、Skill、manifest、基准报告和文档的版本彼此可追踪。

任何第二模型、隐藏实验入口或需求非目标功能均不得作为 v0.1 实现或验收内容。
