# OCR CLI 产品需求规格 v0.1

状态：Draft  
日期：2026-08-01  
工作名称：`ocrx`

## 1. 文档目的

本文定义 `ocrx` 第一版的产品范围、用户接口、模型管理、PDF 解析、输出格式、进度反馈、Agent Skill 和验收标准。

第一版采用以下核心技术路线：

- 使用 Rust 开发跨平台命令行程序。
- 通过 llama.cpp 的 Rust binding，在同一个 Rust 进程中加载 OvisOCR2 GGUF 权重。
- v0.1 只正式支持 OvisOCR2，不注册、下载、运行或承诺支持任何其他 OCR 模型。内部架构应保留后续接入其他模型的边界，但该扩展能力不属于 v0.1 用户功能和发布门槛。
- 使用 llama.cpp `libmtmd` 处理页面图片和配套的 multimodal projector（`mmproj`）。
- 根据设备能力和可用内存自动选择并下载合适的量化模型。
- 将 PDF 逐页渲染、解析并合并为 Markdown，同时裁剪页面中的插图为 PNG。
- 提供稳定的机器可读输出和 Skill，供 Codex、Claude Code 等 Agent 调用。

## 2. 产品目标

### 2.1 核心目标

用户安装 CLI 后，应能通过一条命令把本地 PDF 转换为：

1. 保持自然阅读顺序的 Markdown。
2. 使用 LaTeX 表示的行内公式和块级公式。
3. 使用 HTML 表格表示的表格内容。
4. 从原页面裁剪并保存的 PNG 插图。
5. 包含任务状态、页数、耗时、模型和警告的 JSON 结果。

示例：

```bash
ocrx scan ./document.pdf --output-dir ./document-ocr
```

### 2.2 非目标

第一版不包含：

- GUI。
- 云端 OCR 服务。
- 模型训练或微调。
- OvisOCR2 之外的任何 OCR 模型，包括 HunyuanOCR；多模型选择和第二模型接入属于后续版本。
- 多个 PDF 的跨文档语义合并。
- 对 OCR 结果进行人工编辑的界面。
- 精确复原原 PDF 字体、分页和视觉样式。
- 将 OCR 文本层重新写回 PDF。

## 3. 目标平台

### 3.1 首发平台

- macOS 14 及以上：Apple Silicon arm64，Metal 加速。
- Windows 10/11：x86_64，CPU 必须可用。

### 3.2 Windows 可选加速

- NVIDIA GPU：CUDA 构建。
- AMD、Intel、NVIDIA 通用 GPU：Vulkan 构建。
- GPU 后端不可用或初始化失败时，允许回退 CPU，并明确显示警告。

### 3.3 暂不承诺

- Intel macOS 的 GPU 加速。
- Windows on ARM。
- Linux 正式安装包；架构应允许后续增加。

## 4. 主要用户场景

### 4.1 人工调用

用户将论文、合同、扫描书籍或报告转换为 Markdown：

```bash
ocrx scan paper.pdf -o ./paper
```

### 4.2 Agent 调用

Codex 或 Claude Code 根据 Skill 调用 CLI：

```bash
ocrx scan paper.pdf \
  --output-dir ./paper-ocr \
  --report json \
  --progress jsonl \
  --non-interactive
```

Agent 读取最终 JSON 中的 `markdown_path`，而不是从终端日志中猜测输出位置。

### 4.3 离线调用

模型已下载后，用户可断网运行：

```bash
ocrx scan paper.pdf --offline
```

离线状态缺少所需模型时，程序必须立即失败并给出可操作错误，不能静默联网。

## 5. CLI 命令设计

### 5.1 命令集合

```text
ocrx scan <PDF>
ocrx model list
ocrx model info [ovisocr2]
ocrx model recommend
ocrx model install [PROFILE]
ocrx model remove <PROFILE>
ocrx model verify [PROFILE]
ocrx doctor
ocrx skill install [codex|claude|all]
ocrx skill uninstall [codex|claude|all]
ocrx version
```

### 5.2 `scan` 基础参数

```text
ocrx scan <INPUT>
  -o, --output-dir <DIR>
  --model <MODEL>  # v0.1 仅接受 ovisocr2；为后续兼容保留
  --model-profile <auto|compact|balanced|quality|reference>
  --device <auto|cpu|metal|cuda|vulkan>
  --pages <RANGE>
  --dpi <DPI>
  --max-image-tokens <N>
  --max-output-tokens <N>
  --context-size <N>
  --progress <auto|bar|plain|jsonl|off>
  --report <text|json>
  --resume
  --overwrite
  --offline
  --non-interactive
  --keep-pages
```

页码范围至少支持：

```text
1
1-5
1,3,8-12
```

### 5.3 默认行为

- `--model` 默认值和 v0.1 唯一合法值均为 `ovisocr2`。传入其他值必须在下载或初始化前返回“不受支持的模型”错误，不得尝试联网查找或动态加载。
- `--model-profile auto`。
- 交互式终端默认 `--progress bar`。
- 非 TTY 环境默认 `--progress plain`。
- `--report text` 时，人类可读总结写到 stderr，Markdown 内容不直接写到 stdout。
- `--report json` 时，最终结果以单个 JSON 对象写到 stdout；日志和进度不得污染 stdout。
- 默认输出目录为输入文件同级的 `<stem>-ocr/`。
- 输出目录已存在且包含结果时，除非使用 `--resume` 或 `--overwrite`，否则失败。

## 6. 模型与量化分档

### 6.0 v0.1 支持边界

v0.1 的唯一正式支持模型是 OvisOCR2。模型 manifest、安装器、`model` 子命令、自动推荐、Skill、质量基线和发布工件均只允许面向 OvisOCR2。不得将 HunyuanOCR 或其他模型作为实验开关、隐藏参数、远程 manifest 条目或“尽力支持”功能随 v0.1 发布。

本节出现的 adapter、统一页面结构、model ID 和兼容矩阵用于防止 OvisOCR2 实现与共享 PDF 流水线永久耦合，仅属于内部可扩展性要求。v0.1 不要求实现第二个生产 adapter，也不要求用户能够安装或切换到第二个模型。

### 6.1 模型组成

v0.1 的 OvisOCR2 模型包必须同时声明并校验：

- 唯一模型 ID `ovisocr2`。
- language GGUF。
- 与该转换版本匹配的 `mmproj` GGUF。
- tokenizer/chat template 元数据。
- 模型来源、revision、SHA-256、许可证和文件大小。
- 最低兼容 llama.cpp commit/binding 版本。
- 模型 adapter ID、adapter 版本和输出 schema 版本。
- 图像像素范围、上下文限制、默认采样参数和 stop token。
- `markdown`、`latex`、`html_tables`、`reading_order`、`visual_region_bbox` 能力声明。

语言模型和 `mmproj` 不匹配时禁止启动推理。

模型 manifest 必须锁定准确模型代际和 revision。模型主页指向新代际时不得静默替换已注册代际；修改 GGUF、`mmproj`、chat template、adapter 或输出 schema 中任一项都必须产生新的可追踪 revision。

### 6.2 第一版默认模型分档

| Profile | 建议量化 | 典型模型体积 | 最低可用内存 | 推荐内存 | 目的 |
|---|---|---:|---:|---:|---|
| `compact` | Q4_K_M | 约 529 MB + mmproj | 8 GB | 8 GB | 最小下载、最低内存 |
| `balanced` | Q5_K_M | 约 578 MB + mmproj | 8 GB | 16 GB | 默认质量与速度平衡 |
| `quality` | Q8_0 | 约 812 MB + mmproj | 12 GB | 16 GB | OCR 质量优先 |
| `reference` | BF16 | 约 1.52 GB + mmproj | 16 GB | 24 GB | 回归与高质量对照 |

首版可使用约 207 MB 的 BF16 `mmproj`。最终分档阈值必须由 P0 实测修订，不能只根据模型文件大小决定。

`compact`、`balanced`、`quality`、`reference` 是产品级质量档，不等同于固定量化格式。OvisOCR2 manifest 必须分别声明各 profile 对应的量化文件、资源门槛和质量结果。内部数据结构不得假设未来所有模型使用相同的量化映射，但 v0.1 不提供其他模型的映射。未完成实测的 profile 不得发布或被 `auto` 选择。

### 6.3 自动选择规则

`auto` 只能在 OvisOCR2 内部选择 profile，不得选择、下载或回退到其他模型。选择 profile 时必须考虑：

1. 操作系统和 CPU 架构。
2. 物理内存与当前可用内存。
3. 可用推理后端。
4. GPU/统一内存能力。
5. 用户配置的最大模型磁盘预算。
6. 模型是否已经下载并校验通过。

初始规则：

| 环境 | 自动推荐 |
|---|---|
| 可用内存低于 6 GB | 拒绝启动，提示释放内存 |
| 8 GB 设备 | `compact` |
| 16 GB 设备 | `balanced` |
| 24 GB 及以上 | `quality` |

`reference` 不自动选择，只能由用户明确指定。

若推荐模型尚未下载：

- 交互模式：显示模型、来源、大小、磁盘空间和许可后确认下载。
- `--non-interactive`：默认允许下载公开且 manifest 已信任的模型；可通过配置禁止自动下载。
- `--offline`：禁止下载并返回专用错误码。

用户显式指定 profile 时，若内存不足，程序必须给出警告；明显无法加载时必须拒绝，而不是让系统进入严重 swap 或 OOM。

### 6.4 模型存储

模型默认存放在平台标准用户缓存目录，不放入当前项目：

```text
<cache>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf
<cache>/ocrx/models/ovisocr2/<revision>/mmproj.gguf
<cache>/ocrx/models/ovisocr2/<revision>/manifest.json
```

缓存布局必须保留 `<model-id>` 层级，以便未来版本隔离模型；v0.1 只允许创建 `ovisocr2` 目录。不同 revision、量化和 `mmproj` 不得复用会产生歧义的缓存项。

下载要求：

- 支持断点续传。
- 先写临时文件，校验成功后原子重命名。
- 必须验证 SHA-256。
- 磁盘不足时下载前失败。
- 支持代理和可配置 Hugging Face endpoint。
- 失败后不得留下会被误认为完整模型的文件。
- manifest 必须声明许可证；需要用户接受的许可证在首次下载前记录明确同意。非交互模式未提供预先同意时必须失败，不能代替用户接受条款。

### 6.5 可扩展模型适配架构

模型差异必须封装在版本化的 `OcrModelAdapter` 中。adapter 至少负责：

- 报告模型能力和运行时兼容性。
- 构造模型专用 prompt/chat template。
- 给出图像尺寸和像素预算约束。
- 给出 temperature、上下文、输出 token、stop token 等推理默认值。
- 将模型原始输出解析并校验为统一页面文档结构。

共享 PDF 流水线不得直接解析某个模型专用标签。统一页面结构至少能表达有序的文本、LaTeX 公式、HTML 表格和带 bbox 的视觉区域。Markdown 渲染、PNG 裁剪、文件命名和结果 JSON 必须只消费该统一结构，以保证更换模型不改变输出契约。

OvisOCR2 adapter 必须同时实现 `markdown`、`latex`、`html_tables`、`reading_order` 和 `visual_region_bbox`，并通过第 17 节验收。缺少任一必需能力时不得发布。第一版不为其他模型实现 adapter，也不为弥补其他模型的 bbox 能力而引入额外版面检测模型。

同一个扫描任务只能使用一个 model ID、revision、profile、adapter 和 prompt schema；禁止在 PDF 处理中途自动换模。v0.1 的 model ID 必须始终为 `ovisocr2`。

### 6.6 运行时兼容与模型管理输出

每个 CLI 发布版本必须包含并可查询以下兼容矩阵：

```text
CLI version
  → llama.cpp commit / Rust binding ABI
  → 支持的 model ID 与 revision
  → adapter / output schema
  → GGUF / mmproj 格式
```

`model list --json` 和 `model info --json` 在 v0.1 只返回 OvisOCR2，并至少包含模型代际、安装状态、能力、可用 profile、许可证状态、所需空间、运行时是否兼容及不可用原因。加载前必须执行兼容性 preflight；仅能打开 GGUF、但 encoder、projector、chat template 或 adapter 不兼容，也视为初始化失败。

## 7. PDF 逐页处理

### 7.1 流水线

```text
验证 PDF
  → 获取总页数
  → 选择并加载模型
  → 逐页渲染 RGB 页面
  → 通过 libmtmd 编码页面
  → 模型 adapter 构造输入并完成推理
  → adapter 将原始输出解析为统一页面结构
  → 清理截断重复
  → 解析视觉区域 bbox
  → 裁剪并保存 PNG
  → 写入页面 checkpoint
  → 合并最终 Markdown
```

### 7.2 页面渲染

- PDF 必须逐页渲染，禁止默认一次性将全部页面加载到内存。
- 默认以 300 DPI 渲染 PDF 页面，以提高小字号、公式、标点和扫描件的识别质量。
- 页面输入需满足 OvisOCR2 manifest/adapter 的最小和最大像素约束。首版配置使用最小 `448 × 448`、最大 `2880 × 2880` 的总像素预算；该预算表示像素总量上限，不要求页面为正方形。未来版本接入其他模型时必须使用各自经过验证的约束，不得继承 OvisOCR2 数值。
- 页面按 300 DPI 渲染后，若总像素超过模型预算，应保持宽高比缩小到预算以内，不得裁掉页面内容。
- `result.json` 和页面 checkpoint 必须记录原始页面尺寸、300 DPI 渲染尺寸、最终送模尺寸和缩放比例，便于诊断质量问题。
- 用户通过 `--dpi` 指定其他值时仍受模型像素预算约束；如需绕过预算，必须使用单独的实验性参数，不能让 `--dpi` 隐式绕过安全限制。
- 超大页面发生等比缩放时，应在报告中记录 warning。
- 默认同时只推理一页；未来可增加受控并发，但第一版不要求。
- 加密、损坏或不支持的 PDF 必须给出明确错误。

### 7.3 推理提示词

所有完整扫描模型的 adapter 必须通过各自的模型专用提示词和输出解析实现以下产品语义：

- 按自然阅读顺序提取全部可读内容。
- 不翻译、不总结、不改写。
- 输出单个 Markdown 文档片段。
- 公式使用 LaTeX。
- 表格使用 HTML `<table>`。
- 统一页面结构必须包含插图或图表的 bbox。OvisOCR2 adapter 的原始输出格式为：

```html
<img src="images/bbox_{left}_{top}_{right}_{bottom}.png" />
```

- OvisOCR2 adapter 的 bbox 坐标归一化到 `[0, 1000)`；其他 adapter 可以接受模型原生坐标系，但进入统一页面结构前必须转换为该规范坐标。
- 关闭 thinking/reasoning 输出。
- temperature 固定为 `0.0`。

第一版默认最大输出 token 为 16384；允许通过 CLI 降低。若达到上限，页面状态必须标记为 `truncated`，不能作为完全成功处理。

### 7.4 重复与异常输出

必须处理或检测：

- 输出末尾周期性重复。
- 未闭合的代码块、公式或 HTML 表格。
- 无效图片标签。
- bbox 非数字、越界或面积为零。
- 模型输出为空。
- EOS 前达到最大 token。

自动清理不得静默删除大量正常文本。发生不确定清理时应保留原始页面输出到 checkpoint，并在最终报告中产生 warning。

### 7.5 中断与恢复

每页成功后必须原子写入 checkpoint：

```text
.ocrx/pages/page-0001.json
.ocrx/pages/page-0001.md
```

`--resume` 应跳过满足以下条件的页面：

- 输入 PDF 指纹一致。
- model ID、模型 revision、量化、`mmproj` revision 和后端一致。
- adapter、prompt 和输出 schema 版本一致。
- 页面 checkpoint 完整且校验通过。

用户按 Ctrl+C 时：

- 停止当前生成。
- 保留已经完成的页面。
- 删除未完成页面的临时文件。
- 返回中断退出码。
- 提示可以使用 `--resume`。

## 8. 输出规格

### 8.1 目录结构

```text
document-ocr/
  document.md
  result.json
  images/
    page-0001-figure-001.png
    page-0003-figure-001.png
  .ocrx/
    job.json
    pages/
      page-0001.md
      page-0001.json
```

`.ocrx` 是内部恢复数据。后续可提供 `ocrx clean`，第一版不要求。

### 8.2 Markdown

最终 `document.md`：

- 使用 UTF-8 和 LF。
- 按 PDF 页码顺序合并。
- 默认在页面间插入稳定的 HTML 注释：

```markdown
<!-- page: 1 -->
```

- 块级公式使用：

```markdown
$$
E = mc^2
$$
```

- 行内公式使用 `$...$`。
- 表格保留为 HTML `<table>`；第一版不强制转换为 Markdown pipe table。
- 图片引用必须使用相对路径和正斜杠：

```markdown
![Figure 1](images/page-0001-figure-001.png)
```

- 不允许保留模型生成的 `.jpg` bbox 占位路径。

### 8.3 插图裁剪

模型返回：

```html
<img src="images/bbox_120_230_780_690.png" />
```

程序必须：

1. 将 `[0,1000)` 坐标映射回当前渲染页面像素。
2. 将 bbox 限制在页面范围内。
3. 拒绝零面积或异常小的区域。
4. 从未压缩的页面渲染图裁剪。
5. 以 PNG 保存。
6. 用稳定文件名替换 Markdown 标签。
7. 对同页重复或高度重叠的 bbox 去重。

稳定命名：

```text
page-{page:04}-figure-{index:03}.png
```

第一版不要求对插图进行语义命名或自动生成 alt text。默认 alt text 为 `Figure <index>`。

若 bbox 无效，保留正文，移除无效图片标签，并在 `result.json` 中记录 warning。

### 8.4 `result.json`

最低 schema：

```json
{
  "schema_version": 1,
  "status": "completed",
  "input": "/absolute/path/document.pdf",
  "output_dir": "/absolute/path/document-ocr",
  "markdown_path": "/absolute/path/document-ocr/document.md",
  "model": {
    "id": "ovisocr2",
    "name": "OvisOCR2",
    "family": "ovis",
    "profile": "balanced",
    "quantization": "Q5_K_M",
    "revision": "pinned-revision",
    "mmproj_revision": "pinned-revision",
    "adapter": "ovisocr2-v1",
    "adapter_version": 1,
    "prompt_schema_version": 1,
    "runtime_version": "pinned-llama.cpp-commit",
    "backend": "metal"
  },
  "pages": {
    "total": 10,
    "completed": 10,
    "failed": 0,
    "truncated": 0
  },
  "images": 4,
  "elapsed_ms": 125000,
  "warnings": []
}
```

路径必须是规范化绝对路径，方便 Agent 后续读取。

## 9. 进度与终端体验

### 9.1 交互式进度条

交互式终端至少显示：

```text
[██████████░░░░░░░░░░] 5/10  page 5  generating  01:12 elapsed  ETA 01:05
```

状态阶段：

```text
checking
downloading-model
verifying-model
loading-model
rendering
encoding-image
generating
extracting-images
saving
completed
```

长时间生成时，必须持续显示 elapsed time 或 spinner，不能让用户误以为进程卡死。

### 9.2 模型下载进度

下载显示：

- 文件名。
- 已下载/总大小。
- 百分比。
- 传输速度。
- ETA。
- 当前是 model 还是 mmproj。

### 9.3 JSON Lines 进度

`--progress jsonl` 将事件写到 stderr，每行一个 JSON：

```json
{"event":"page_started","page":3,"total":10}
{"event":"page_progress","page":3,"stage":"generating","elapsed_ms":8200}
{"event":"page_completed","page":3,"images":1,"elapsed_ms":15300}
```

事件 schema 必须版本化，并保证不与 stdout 的最终 JSON 混合。

## 10. 错误与退出码

| 退出码 | 含义 |
|---:|---|
| 0 | 全部成功 |
| 2 | CLI 参数错误 |
| 3 | 输入文件错误或不支持 |
| 4 | 模型缺失且不能下载 |
| 5 | 模型下载或校验失败 |
| 6 | 模型/后端初始化失败 |
| 7 | PDF 渲染失败 |
| 8 | 页面推理失败 |
| 9 | 输出写入失败 |
| 10 | 部分完成，至少一页失败或截断 |
| 130 | 用户中断 |

错误消息必须包含：发生了什么、可能原因、建议操作。不得只输出底层 C/C++ 错误码。

## 11. 配置

配置优先级：

```text
CLI 参数 > 环境变量 > 用户配置文件 > 内置默认值
```

用户配置至少支持：

```toml
default_profile = "auto"
default_model = "ovisocr2" # v0.1 固定值，不接受其他模型
default_device = "auto"
auto_download = true
model_endpoint = "https://huggingface.co"
max_model_disk_mb = 4096
progress = "auto"
```

密钥不得写入普通日志或 `result.json`。

## 12. Agent Skill

### 12.1 Skill 目标

提供一个规范源 Skill，并由安装程序适配安装到 Codex 和 Claude Code 的用户或项目 Skill 目录。

Skill 应告诉 Agent：

- 何时使用 `ocrx`。
- 如何先运行 `ocrx doctor --report json`。
- 如何选择输入和安全的输出目录。
- 如何使用非交互模式。
- 如何读取最终 JSON 和 Markdown。
- 不要把整个长文档无条件读入上下文。
- 如何处理模型下载、部分完成和恢复。
- 如何查看 OvisOCR2 的安装状态及能力；不得建议、下载或尝试调用其他模型。
- 文档内容保持本地，不上传到远程服务。
- 关键用途需要人工核验 OCR 结果。

### 12.2 推荐调用

```bash
ocrx scan "/absolute/input.pdf" \
  --output-dir "/absolute/output" \
  --model ovisocr2 \
  --model-profile auto \
  --progress jsonl \
  --report json \
  --non-interactive
```

### 12.3 Skill 行为约束

- Agent 必须使用绝对路径。
- Agent 不得默认覆盖现有目录。
- 如果任务失败，Agent 应依据退出码和 JSON 错误对象判断后续动作。
- 输出较长时，Agent 应先查看目录和 Markdown 大小，再选择性读取。
- Agent 应向用户报告 `markdown_path`、图片数量、失败页和 warnings。
- Skill 只依赖稳定 CLI 和 `result.json` 契约，不嵌入某个模型的 prompt、原始标签或量化文件名。

## 13. 安装与更新

### 13.1 安装入口

macOS：

```bash
curl -fsSL https://raw.githubusercontent.com/<org>/<repo>/main/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/<org>/<repo>/main/install.ps1 | iex
```

安装脚本必须：

- 检测 OS、架构和可用 GPU 后端。
- 下载对应 GitHub Release。
- 验证 SHA-256。
- 默认安装到用户目录，不要求管理员权限。
- 提示配置 PATH。
- 可选安装 Agent Skill。
- 不默认下载所有模型。

### 13.2 更新边界

CLI、llama.cpp runtime、模型 manifest、模型权重和 Skill 分别版本化。升级 CLI 不得自动删除已下载模型。

## 14. 安全与隐私

- 默认不上传输入、页面、OCR 文本或统计信息。
- 除模型下载和显式更新检查外，不进行网络访问。
- `--offline` 必须从网络层阻止下载。
- PDF、模型文件名和 OCR 内容不得进入遥测。
- 模型下载必须使用 HTTPS 和固定校验值。
- 模型 manifest 来源必须受信任并固定 revision；下载器不得根据模型输出或未经验证的远端元数据拼接可执行路径。
- v0.1 必须独立管理 OvisOCR2 的许可证、用户接受记录和再分发资格；存储结构应允许未来模型使用独立记录，不得把 OvisOCR2 的许可结论作为其他模型的默认值。
- 插图输出必须限制在目标目录内，模型生成的路径不得直接作为文件系统路径使用。
- HTML 表格和模型生成标签属于不可信文本；CLI 不执行其中的脚本、URL 或命令。

## 15. 性能与资源要求

### 15.1 基准设备

第一版至少在以下设备验证：

- Mac mini M2 8 GB。
- Mac mini M2 16 GB。
- Windows x64 16 GB CPU-only。
- Windows x64 NVIDIA GPU（如可用）。

### 15.2 资源约束

- PDF 页面默认串行处理。
- 完成一页后及时释放页面和临时 tensor。
- 8 GB 设备使用 `compact` 时不得因默认参数导致系统 OOM。
- 默认 300 DPI 渲染不得改变逐页处理原则；送模前必须应用模型像素预算，页面原始位图应在完成裁图后及时释放。
- 处理 100 页 PDF 时，内存不应随页数线性增长。
- 模型只加载一次，不得每页重新加载。
- 单个扫描任务内不得更换模型；任务间换模时必须释放前一模型、projector 和相关后端资源。
- 无输出 token 的生成阶段仍需至少每秒刷新一次可见进度状态。

### 15.3 性能指标

P0 后补充并冻结：

- 模型加载时间。
- 页面渲染时间。
- 图像编码时间。
- 首 token 时间。
- 输出 token/s。
- 普通页和复杂页总耗时。
- 峰值内存。
- 不同量化的质量差异。

第一版不以固定 pages/min 作为发布阻断指标，但不得出现无限生成或无超时卡死。

## 16. 可观测性

`ocrx doctor --report json` 至少输出：

- CLI 版本。
- 平台和架构。
- 总内存与可用内存。
- 检测到的 llama.cpp backend。
- Metal/CUDA/Vulkan 可用性。
- 已安装模型及校验状态。
- 推荐 profile。
- 模型缓存目录和可用磁盘空间。
- Skill 安装状态。

提供 `--log-level error|warn|info|debug|trace`。默认日志不得输出 OCR 正文。

## 17. 第一版验收标准

### 17.1 功能验收

- 能在 macOS Apple Silicon 和 Windows x64 启动。
- 能自动检测配置并推荐模型分档。
- `--model ovisocr2` 能正常工作，任何其他模型 ID 都在联网或加载前被明确拒绝。
- `model list/info --json` 只列出 OvisOCR2，并能返回安装、能力、profile、许可和兼容状态。
- 能下载 model 和 mmproj，显示进度并校验哈希。
- 能在 Rust 进程内通过 llama.cpp binding 完成图片多模态推理。
- 能逐页处理 PDF，不一次性加载全部页面。
- 能生成单个合并 Markdown。
- 公式以可渲染 LaTeX 保存。
- 能根据模型 bbox 裁剪至少一个测试文档中的插图，并保存为 PNG。
- Markdown 中图片链接全部可解析且文件存在。
- 能显示逐页进度和 ETA/elapsed time。
- Ctrl+C 后可以 `--resume`。
- JSON stdout 不被日志和进度污染。
- Codex 与 Claude Code Skill 能完成一次端到端调用。

### 17.2 质量验收

#### 17.2.1 回归集与基线

进入完整 CLI 开发前，必须冻结一个带人工校对真值的回归集。回归集至少包含 30 页、每类场景至少 2 页，并覆盖：

- 简体中文。
- 繁体中文。
- 英文。
- 中英混排。
- 双栏论文。
- 行内与块级公式。
- 简单和复杂表格。
- 页面插图和图表。
- 扫描噪声。
- 竖排或旋转文字。
- 超长页面输出。

每个页面的真值至少包括：

- 按自然阅读顺序排列的正文文本。
- 公式 LaTeX；没有公式的页面标记为不适用。
- 表格 HTML 结构；没有表格的页面标记为不适用。
- 文本块阅读顺序。
- 插图和图表的 bbox；没有视觉区域的页面标记为不适用。

同一模型的所有 profile 必须使用相同 PDF、页码、300 DPI 渲染规则、该模型的送模像素预算、adapter、prompt、采样参数和后处理版本运行。随机种子固定；temperature 为 `0.0`。该模型对应 revision 的官方 Safetensors BF16 输出作为参考实现结果，人工校对真值作为最终质量依据。GGUF `reference` 还必须与同一模型同一 revision 的官方 BF16 结果比较，以识别 llama.cpp 或 GGUF 转换带来的偏差。不同模型可以使用不同的模型专用 prompt 和像素预算，但必须输出同一统一页面结构并使用同一真值及评分实现；不同基准版本的分数不得混算。

回归集、真值、评分脚本、model ID、模型 revision、量化、`mmproj` revision、llama.cpp commit、adapter、prompt/schema 版本和每次结果必须进入版本控制或发布工件；改变其中任一项都视为新的基准版本，不得与旧结果直接混算。

#### 17.2.2 指标计算方法

评分前仅允许执行以下统一归一化：

- Unicode NFC。
- CRLF 转 LF。
- 去除行尾空白。
- 连续三个及以上空行归一化为两个空行。
- 不得忽略大小写、标点、数字、公式符号或汉字形体差异。

指标定义：

| 指标 | 计算方法 | 越高/低越好 |
|---|---|---|
| 文本 NED | 从 Markdown 中移除图片标签、HTML 结构标签和公式后，对可见正文按 `1 - LevenshteinDistance / max(reference_len, hypothesis_len)` 计算；先逐页计算，再按真值字符数加权汇总 | 越高越好 |
| 公式完整率 | 将公式提取为规范化 LaTeX 序列，按页面内顺序匹配；完整保留且通过 KaTeX 兼容语法解析的公式数 / 真值公式数 | 越高越好 |
| 表格 TEDS | 对输出和真值 HTML table 解析 DOM，移除纯样式属性后计算标准 Tree Edit Distance Similarity；按表格数平均 | 越高越好 |
| 阅读顺序准确率 | 对已标注文本块的所有有序块对计算 pairwise order accuracy；按页面平均 | 越高越好 |
| bbox 有效率 | 语法有效、坐标在 `[0,1000)`、面积大于零且可成功裁图的 bbox 数 / 输出 bbox 数 | 越高越好 |
| bbox 召回率 | 输出 bbox 与真值 bbox 按 IoU `>= 0.5` 一对一匹配后的命中数 / 真值 bbox 数 | 越高越好 |
| 重复输出率 | 被周期性重复检测器判定存在非原文重复尾部的页面数 / 总页数 | 越低越好 |
| 截断率 | 达到最大输出 token、缺少正常终止或被标记为 `truncated` 的页面数 / 总页数 | 越低越好 |
| 空白/失败率 | 无有效 Markdown 或推理失败的页面数 / 总页数 | 越低越好 |

公式、表格或 bbox 指标只在具有对应真值的页面上计算。样本数必须随报告一起给出；不允许将“不适用”计为满分。

#### 17.2.3 v0.1 发布阈值

OvisOCR2 的 `reference`、`quality` 和 `balanced` 必须分别满足下表。所有条件为 AND 关系，任一指标不达标即该 profile 不通过。

| 指标 | `reference` | `quality` | `balanced` |
|---|---:|---:|---:|
| 文本 NED | `>= 0.97` | `>= 0.95`，且较 `reference` 下降不超过 `0.02` | `>= 0.93`，且较 `reference` 下降不超过 `0.04` |
| 公式完整率 | `>= 0.95` | `>= 0.92`，且较 `reference` 下降不超过 `0.03` | `>= 0.88`，且较 `reference` 下降不超过 `0.07` |
| 表格 TEDS | `>= 0.95` | `>= 0.92`，且较 `reference` 下降不超过 `0.03` | `>= 0.88`，且较 `reference` 下降不超过 `0.07` |
| 阅读顺序准确率 | `>= 0.98` | `>= 0.96` | `>= 0.94` |
| bbox 有效率 | `>= 0.98` | `>= 0.97` | `>= 0.95` |
| bbox 召回率 | `>= 0.95` | `>= 0.92` | `>= 0.88` |
| 重复输出页面 | `0` | `0` | `0` |
| 截断页面 | `0` | `0` | `0` |
| 空白或失败页面 | `0` | `0` | `0` |

`compact` 是 8 GB 最低资源 profile，必须满足：

- 文本 NED `>= 0.90`，且较 `reference` 下降不超过 `0.07`。
- 公式完整率和表格 TEDS 均 `>= 0.82`。
- 阅读顺序准确率 `>= 0.92`。
- bbox 有效率 `>= 0.93`、bbox 召回率 `>= 0.82`。
- 重复、截断、空白或失败页面均为 `0`。

除绝对阈值外，OvisOCR2 GGUF `reference` 相对同 revision 的官方 Safetensors BF16 在同一回归集上的下降不得超过：文本 NED `0.01`、公式完整率 `0.02`、表格 TEDS `0.02`、阅读顺序准确率 `0.01`、bbox 有效率或召回率 `0.02`。超过任一差值视为 GGUF 转换、OvisOCR2 adapter 或 llama.cpp 推理路径未对齐，`reference` 不通过。

上述阈值是 v0.1 的最低发布门槛。P0 可以在完整 CLI 开发前基于业务风险收紧阈值，但不得因为某个量化版本未通过而放宽阈值。若评分实现存在缺陷，应先修复评分工具并升级基准版本，再重新运行全部 profile。

#### 17.2.4 默认 profile 判定

OvisOCR2 的默认 profile 选择必须遵循：

1. `reference` 未通过时，说明 GGUF/llama.cpp 路径相对产品真值不合格，阻断发布。
2. `quality` 未通过时，阻断发布；不得以更低量化版本替代。
3. `balanced` 通过时，16 GB 设备默认使用 `balanced`。
4. `balanced` 未通过但 `quality` 通过时，16 GB 设备默认提升到 `quality`，并同步修订其内存要求。
5. `compact` 通过时，8 GB 设备默认使用 `compact`。
6. `compact` 未通过时，阻断 v0.1 发布；不得缩减既定的 8 GB 首发支持范围，也不得静默下载更大模型并使设备进入严重 swap。
7. 24 GB 及以上设备默认使用已通过的 `quality`。

发布报告必须标明 OvisOCR2 model ID 和 revision，包含各 profile 的逐项得分、与 `reference` 的差值、样本数、失败页面及最终默认选择。不得只给出“通过/失败”而不保留原始评分结果。

### 17.3 稳定性验收

- 连续处理 100 页无内存线性增长。
- 损坏 PDF、下载中断、哈希失败、磁盘不足均返回正确错误。
- 无效 bbox 不得导致越界读取或任意路径写入。
- 同一输入、模型、prompt 和采样参数重复运行应具有可接受的一致性。
- 使用不同 model ID/revision/adapter 创建的 checkpoint 不得互相恢复。

## 18. P0 技术验证门槛

正式实现完整 CLI 前，必须先完成：

1. 固定一个已知可用的 llama.cpp commit。
2. 使用对应 `llama-mtmd-cli` 跑通 OvisOCR2 model + mmproj。
3. 在 Mac mini M2 上验证 Metal，并比较 200、250、300 DPI 在最终送模像素预算一致或不同情况下的质量和耗时。
4. 在 Windows 至少验证 CPU，优先补充 Vulkan/CUDA。
5. 对比 Transformers BF16、GGUF BF16、Q8、Q5、Q4。
6. 验证官方 bbox 提示词能稳定产出可裁剪区域。
7. 验证 Rust binding 输出与同 commit 的 `llama-mtmd-cli` 基本一致。
8. 确定最终模型来源、revision、SHA-256 和再分发策略。
9. 验证 OvisOCR2 adapter 与 PDF、Markdown、裁图和 JSON 共享流水线之间通过统一页面结构交互，模型专用原始标签不得泄漏到共享模块。

HunyuanOCR 或任何第二模型的可行性验证不属于 v0.1 P0 门槛，不得阻断 v0.1 开发或发布。

若以下任一项失败，不进入完整 CLI 开发：

- `libmtmd` 无法正确编码 OvisOCR2 页面图片。
- Rust binding 缺失必要 MTMD 能力。
- `reference`、`quality`、`compact` 或最终默认 profile 未达到第 17.2 节的对应发布阈值。
- 页面生成存在不可控循环或高截断率。
- macOS/Windows 至少一个首发平台无法稳定运行。
- OvisOCR2 缺少必需能力、不能与锁定 runtime 兼容或未达到第 17.2 节阈值。

## 19. 待决策事项

1. 正式产品名和二进制名是否采用 `ocrx`。
2. PDF 渲染使用 PDFium 还是其他可再分发实现。
3. llama.cpp Rust binding 采用 `llama-cpp-2`、后续版本还是内部 FFI 包装。
4. 首发是否包含 Windows Vulkan，还是仅 CPU/CUDA。
5. 社区 GGUF 是否允许直接作为默认下载源，或由项目自行转换并发布。
6. 模型权重是否由安装器只下载，还是允许在 GitHub Release 镜像。
7. `balanced` 最终采用 Q5_K_M 还是 Q8_0。
8. 页面失败时默认继续还是立即终止；本稿建议继续并最终返回退出码 10。
9. Skill 默认安装到用户级还是项目级；本稿建议用户级安装、支持显式项目级安装。

以下事项明确延期到 v0.1 之后，不属于本版本待决策项或发布阻断项：

- 是否以及何时支持 HunyuanOCR。
- HunyuanOCR 采用哪个代际、GGUF/`mmproj` 来源和许可证接受流程。
- HunyuanOCR 是否能稳定提供插图 bbox。
- 面向用户的多模型安装、切换和默认模型管理命令。

## 20. 参考依据

- OvisOCR2 官方模型卡：页面级图片生成自然阅读顺序 Markdown，包含文字、公式、表格和视觉区域。
- 官方提示词要求视觉区域输出归一化 bbox，并给出了从原页面裁剪图片的示例。
- llama.cpp 通过 `libmtmd` 支持独立 language GGUF 与 `mmproj` GGUF 的多模态输入。
- Rust `llama-cpp-2` 已暴露实验性的 MTMD 图像处理接口；实现时必须固定兼容版本。
- [llama.cpp 多模态文档](https://github.com/ggml-org/llama.cpp/blob/master/docs/multimodal.md)及 HunyuanOCR 支持记录：不同 OCR 模型可能需要模型专用 prompt/输入结构，应通过 `libmtmd` 和 adapter 统一接入。
- [llama.cpp 添加模型指南](https://github.com/ggml-org/llama.cpp/blob/master/docs/development/HOWTO-add-model.md)：多模态模型应复用统一接口，模型专用编码和 projector 行为需显式实现及验证。
- [HunyuanOCR 官方仓库](https://github.com/Tencent-Hunyuan/HunyuanOCR)与[模型卡](https://huggingface.co/tencent/HunyuanOCR)：模型代际、任务提示、输出约定和许可证必须按固定 revision 登记，不能由浮动主页隐式决定。
