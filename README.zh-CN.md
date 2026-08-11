# OCR-CLI（`ocrx`）

OCR-CLI 是一款本地 Rust 命令行工具，用于通过 OvisOCR2 从 PDF 文档中提取结构化内容。它设计为本地运行，将文档数据保留在主机上，并生成面向 Markdown 格式的 OCR 产物，适合人类或自动化流程进行后续处理。

> **状态：开发预览版**
>
> 本仓库尚未发布正式的生产版本。0.1 版本目前正在 macOS arm64 上进行验证。运行时可复现性、PDFium 打包、长文档资源行为、模型质量基线以及最终发布许可文件等事项仍在冻结确认中。

## 范围

当前 v0.1 版本有意将范围控制在较小范围内：

- 仅支持一种 OCR 模型：[OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2)。
- 通过 PDFium 进行本地 PDF 输入和页面渲染。
- 通过 llama.cpp/libmtmd 以及独立的 `mmproj` GGUF 文件进行多模态推理。
- OvisOCR2 配置档：`compact`、`balanced`、`quality` 和 `reference`。
- CLI 提供 Markdown、JSON、进度、断点续扫和诊断等工作流。
- 提供 macOS arm64 的发布/Skill 打包路径。

其他 OCR 模型不属于 v0.1 公开约定的一部分。本项目尤其不声称支持 HunyuanOCR 或其他模型系列。

## 当前实现状态

本仓库包含 Rust 工作区、CLI 入口点、PDFium 集成、llama.cpp/MTMD 运行时集成、OvisOCR2 模型清单、输出类型、验证脚本以及 Agent Skill 定义。部分生产门槛仍待完成；请勿将实现或测试夹具的存在视为生产支持的保证。

已知的发布工作记录在 [`docs/plan/pre-development-macos-freeze-plan.md`](docs/plan/pre-development-macos-freeze-plan.md) 中，包括运行时版本对齐、PDFium 发布验证、资源限制、模型来源和质量证据。

## 环境要求

对于当前的开发路径，请使用：

- macOS arm64；
- Rust `1.85` 或由 [`rust-toolchain.toml`](rust-toolchain.toml) 指定的兼容工具链；
- 运行 PDF 流水线时需要兼容的 arm64 PDFium 动态库；
- 单独安装 OvisOCR2 模型的 GGUF 文件及其多模态投影器（`mmproj`）。

打包的归档文件中不包含模型权重。模型文件体积较大，且为版本化产物，因此单独管理，以便用户验证其来源和许可状态。

## 构建与引导检查

克隆仓库后运行：

```bash
cargo build --workspace
cargo test --workspace
cargo run -p ocrx -- version
cargo run -p ocrx -- doctor
```

仓库还提供了 [`scripts/`](scripts/) 下的专项检查脚本，包括兼容性、回归、发布脚本、PDFium 和 Skill 验证辅助工具。

## CLI 用法

主要命令如下：

```bash
# 显示 CLI 版本和可用帮助。
cargo run -p ocrx -- version
cargo run -p ocrx -- help

# 检查本地运行时和模型环境。
cargo run -p ocrx -- doctor
cargo run -p ocrx -- doctor --report json

# 查看或管理支持的 OvisOCR2 模型。
cargo run -p ocrx -- model list
cargo run -p ocrx -- model info ovisocr2
cargo run -p ocrx -- model recommend

# 使用自动选择的 OvisOCR2 配置档扫描 PDF。
cargo run -p ocrx -- scan /absolute/path/to/input.pdf \
  --model ovisocr2 \
  --model-profile auto \
  --non-interactive
```

对于发布版二进制文件，将 `cargo run -p ocrx --` 替换为 `ocrx`。在自动化或 Skill 工作流中，请使用绝对路径指定 PDF 输入和输出目录。已取消的扫描可以通过 `--resume` 恢复；`--resume` 和 `--overwrite` 互斥。

常用的扫描选项包括：

```text
--output-dir <DIR>          将产物写入指定目录
--pages <SELECTION>         处理指定页面
--model-profile <PROFILE>   auto|compact|balanced|quality|reference
--device <DEVICE>           auto|cpu|metal|cuda|vulkan
--report <FORMAT>           text|json
--progress <MODE>           auto|bar|plain|jsonl|off
--offline                   不执行网络操作
--non-interactive           禁用交互式提示
--resume                    恢复中断的扫描
--overwrite                 允许覆盖已有输出
--keep-pages                保留渲染后的页面图像
```

模型安装、输出模式和配置档选择的具体行为仍受 v0.1 兼容性和质量门槛的约束。

## 模型来源与许可

OCR-CLI 的发布归档文件中不包含模型权重。当前清单标识了以下模型资产：

| 资产 | 来源与版本 | 许可/状态 |
| --- | --- | --- |
| OvisOCR2 基础模型 | [ATH-MaaS/OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2) | 模型卡声明为 Apache-2.0 |
| GGUF 量化版本 | [Abiray/OvisOCR2-GGUF](https://huggingface.co/Abiray/OvisOCR2-GGUF)，版本 `8a22290a0c42dbcf84739d6d4c2763f877494ae0` | 模型仓库声明为 Apache-2.0；再分发前请核实上游条款 |
| 多模态投影器 | 来自所选 GGUF 仓库的 `mmproj-F16.gguf` | 必须与其来源和许可元数据一同保留 |
| 基础模型谱系 | [Qwen/Qwen3.5-0.8B](https://huggingface.co/Qwen) | 请参阅适用的上游模型卡和许可 |

机器可读的清单（包括配置档大小、源 URL 和 SHA-256 值）存放在 [`assets/manifests/ovisocr2/`](assets/manifests/ovisocr2/) 中。模型许可与 OCR-CLI 源代码许可相互独立。用户和分发者需自行遵守其下载或再分发的每个模型文件所适用的条款。

## PDFium 与运行时依赖

OCR-CLI 使用了以下重要的上游组件：

- [PDFium](https://github.com/chromium/pdfium)，通过 [`pdfium-render`](https://crates.io/crates/pdfium-render) 访问，用于 PDF 渲染；
- [llama.cpp](https://github.com/ggml-org/llama.cpp) 及其[多模态/libmtmd 支持](https://github.com/ggml-org/llama.cpp/blob/master/tools/mtmd/README.md)，通过 Rust 绑定访问，用于本地多模态推理；
- [`Cargo.toml`](Cargo.toml) 和 [`Cargo.lock`](Cargo.lock) 中列出的 Rust 生态依赖。

当发布归档文件包含 `lib/libpdfium.dylib` 时，归档文件必须包含适用的 PDFium 和 Chromium 第三方声明。PDFium 的再分发要求保留相关版权声明、许可条件和免责声明。最终发布版本还必须附带完整的第三方许可清单；请在再分发前参阅仓库的发布检查清单。

## 安装与打包

当前的打包路径针对 macOS arm64：

```bash
OCRX_PDFIUM_LIBRARY=/absolute/path/to/libpdfium.dylib \
  bash scripts/package-macos-arm64.sh
```

生成的归档文件包含 CLI 二进制文件、OCR-CLI Skill、PDFium 库和校验和。它不包含 OvisOCR2 权重。本地源码安装方式：

```bash
cargo build --release -p ocrx
bash scripts/install.sh
```

安装脚本目前仍为开发预览版工具。发布归档文件前，请验证目标架构、PDFium 版本、发布校验和、模型清单和第三方声明。

## Agent Skill

Skill 定义文件位于 [`assets/skills/ocrx/SKILL.md`](assets/skills/ocrx/SKILL.md)。它指示 Agent 执行以下操作：

1. 首先运行 `ocrx doctor --report json`；
2. 仅使用已安装的本地 OvisOCR2 配置档；
3. 将 PDF 和 OCR 数据保留在本地；
4. 使用绝对路径指定输入和输出；
5. 要求人工验证 OCR 输出和提取的图像。

该 Skill 不会自动下载模型文件。

## 局限性与安全注意事项

OCR 输出是概率性的，可能包含不正确或不完整的文本、公式、表格、阅读顺序或视觉区域坐标。重要文档的结果必须经过人工复核。请勿将 OCR-CLI 作为法律、财务、医疗、合规或归档决策的唯一依据。

PDF 的复杂性可能影响内存使用和运行时间。加密 PDF、格式异常的 PDF、异常页面尺寸以及非常长的文档可能需要额外的验证。本项目目前不承诺对每种实际 PDF 都有稳定的资源上限。

OvisOCR2 官方模型卡也指出，尽管有过滤和质量保证流程，解析错误仍可能发生。有关模型特定的局限性，请参阅 [OvisOCR2 模型卡](https://huggingface.co/ATH-MaaS/OvisOCR2)。

## 致谢

OCR-CLI 的诞生离不开以下项目和贡献者：

- [ATH-MaaS/OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2) 以及 [OvisOCR2 技术报告](https://arxiv.org/abs/2607.13639)的作者；
- [Abiray/OvisOCR2-GGUF](https://huggingface.co/Abiray/OvisOCR2-GGUF)，提供了所选的 GGUF 量化和多模态投影器分发；
- [Qwen](https://huggingface.co/Qwen)，提供 OvisOCR2 所使用的基础模型谱系；
- [llama.cpp](https://github.com/ggml-org/llama.cpp) 及其贡献者，提供本地推理和 libmtmd；
- [PDFium](https://github.com/chromium/pdfium)、Chromium 贡献者以及 [`pdfium-render`](https://crates.io/crates/pdfium-render) 项目；
- Rust 编译器、Cargo 以及项目依赖元数据中列出的开源 crate。

如果您在学术工作中使用 OvisOCR2，请使用[官方模型卡](https://huggingface.co/ATH-MaaS/OvisOCR2#citation)提供的引用格式进行引用。

## 许可

OCR-CLI 源代码拟按 Apache License, Version 2.0 分发，如 Rust 工作区元数据中所声明。主要第三方组件和发布审查清单记录在 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) 中。在公开发布之前，仓库仍须包含完整的 Apache-2.0 许可文本以及针对该发布的第三方声明审查。

模型权重、PDFium 二进制文件、llama.cpp 衍生组件以及其他依赖项可能受单独的许可和声明约束。请参阅上游来源以及最终发布版的第三方归属文件，了解各组件适用的条款。

## 更多文档

- [OvisOCR2 兼容性矩阵](assets/compatibility/ovisocr2-macos-arm64.json)
- [回归集格式](assets/regression/README.md)

实现计划和内部开发笔记保留在仓库中供贡献者使用，但不属于最终用户文档的组成部分。