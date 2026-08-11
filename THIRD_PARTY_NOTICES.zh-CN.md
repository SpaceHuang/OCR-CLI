# 第三方声明

本文件记录了 OCR-CLI（`ocrx`）使用或分发的第三方软件、模型和二进制制品。本文件旨在提供归属信息和发布审查参考。每个组件均受其各自的许可和上游条款约束。

OCR-CLI 源代码拟按 Apache License, Version 2.0 分发。该许可仅适用于 OCR-CLI 源代码；不自动适用于模型权重、PDFium 二进制文件、Rust 依赖项或其他第三方制品。

## OvisOCR2

OCR-CLI 使用 OvisOCR2 作为 v0.1 唯一正式支持的 OCR 模型。

- 原始模型：[ATH-MaaS/OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2)
- 技术报告：[OvisOCR2 技术报告](https://arxiv.org/abs/2607.13639)
- 所选 GGUF 分发：[Abiray/OvisOCR2-GGUF](https://huggingface.co/Abiray/OvisOCR2-GGUF)
- 所选 GGUF 版本：`8a22290a0c42dbcf84739d6d4c2763f877494ae0`
- 项目清单：[`assets/manifests/ovisocr2/`](assets/manifests/ovisocr2/)
- 声明的模型许可：Apache License 2.0（`Apache-2.0`），由上游模型仓库在审查时声明。

GGUF 模型和 `mmproj` 文件不属于 OCR-CLI 源代码许可的范围。用户和分发者必须保留上游模型元数据，并遵守所使用具体模型版本的条款。除非已针对该发布版本审查了模型文件的来源、许可和再分发状态，否则不得随 OCR-CLI 发布版本一同再分发模型文件。

OvisOCR2 基于 Qwen 模型家族。请参阅 [Qwen 组织](https://huggingface.co/Qwen) 以及对应上游基础模型卡，了解适用于相关模型谱系的许可和条款。

## llama.cpp 与 libmtmd

OCR-CLI 通过 Rust 绑定使用 llama.cpp 进行本地推理，并使用 llama.cpp 的多模态/MTMD 功能处理文档图像和多模态投影器。

- 项目：[ggml-org/llama.cpp](https://github.com/ggml-org/llama.cpp)
- 多模态文档：[libmtmd / MTMD 文档](https://github.com/ggml-org/llama.cpp/blob/master/tools/mtmd/README.md)
- 许可：请参阅上游 [llama.cpp `LICENSE`](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE)
- 其他上游声明：请参阅上游 [`licenses/`](https://github.com/ggml-org/llama.cpp/tree/master/licenses) 目录

在发布生产版本二进制文件之前，必须在发布清单中记录确切的 llama.cpp 版本和 Rust 绑定版本。当前 Cargo 版本在 [`src/ocrx-runtime/Cargo.toml`](src/ocrx-runtime/Cargo.toml) 中声明。

## PDFium

OCR-CLI 通过 [`pdfium-render`](https://crates.io/crates/pdfium-render) Rust crate 使用 PDFium 进行 PDF 渲染。部分 macOS 发布归档文件可能包含 `libpdfium.dylib` 二进制文件。

- 项目：[Chromium PDFium](https://github.com/chromium/pdfium)
- 许可：[PDFium `LICENSE`](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/2028/LICENSE)
- Chromium 第三方许可指南：[Chromium 开源许可证](https://www.chromium.org/chromium-projects/)
- Rust 封装：[`pdfium-render`](https://crates.io/crates/pdfium-render)

再分发 PDFium 时，必须在源码和二进制分发中保留适用的版权声明、许可条件和免责声明。由于 PDFium 与 Chromium 依赖树相关联，包含 `libpdfium.dylib` 的发布版本必须附带适用于该 PDFium 构建版本和修订版的所有声明。二进制文件的修订版本和 SHA-256 必须记录在发布制品中。

## Rust 依赖项

OCR-CLI 使用 Cargo 构建。直接依赖项在工作区和 crate 清单中声明，包括：

- [`image`](https://crates.io/crates/image)
- [`ctrlc`](https://crates.io/crates/ctrlc)
- [`pdfium-render`](https://crates.io/crates/pdfium-render)
- [`llama-cpp-2`](https://crates.io/crates/llama-cpp-2)
- [`llama-cpp-sys-2`](https://crates.io/crates/llama-cpp-sys-2)

每个 crate 及其传递依赖项均保留各自的许可条款。构建的权威依赖版本锁定在 [`Cargo.lock`](Cargo.lock) 中。发布流程必须生成或审查完整的 Cargo 依赖项许可清单（包括传递依赖项），并将该清单随发布制品一并提供。

## 发布检查清单

发布二进制文件或归档文件之前，维护者应核实发布版本包含或链接了以下内容：

1. OCR-CLI 源代码的完整 Apache License 2.0 许可文本。
2. 确切的 OvisOCR2 和 GGUF 模型卡、版本、哈希值和许可元数据。
3. 确切的 llama.cpp 版本及适用的上游 `LICENSE` 和 `licenses/` 声明。
4. 确切的 PDFium 版本、二进制哈希值、许可以及 Chromium/PDFium 第三方声明（当包含 PDFium 二进制文件时）。
5. Cargo 依赖项及其传递依赖项的完整许可清单。
6. 所选发布二进制文件、模型、投影器或打包制品所要求的任何其他声明。

本文件中的链接是对上游材料的引用，不能替代法律上必须随再分发二进制文件一同提供的许可文本或声明。每次发布特有的声明包必须针对该次发布中实际交付的文件进行审查。