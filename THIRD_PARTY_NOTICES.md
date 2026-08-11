# Third-Party Notices

This file records the principal third-party software, model, and binary artifacts used or distributed by OCR-CLI (`ocrx`). It is provided for attribution and release review. Each component remains subject to its own license and upstream terms.

OCR-CLI source code is intended to be distributed under the Apache License, Version 2.0. That license applies to OCR-CLI source code only; it does not automatically apply to model weights, PDFium binaries, Rust dependencies, or other third-party artifacts.

## OvisOCR2

OCR-CLI uses OvisOCR2 as its only formally supported OCR model for v0.1.

- Original model: [ATH-MaaS/OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2)
- Technical report: [OvisOCR2 Technical Report](https://arxiv.org/abs/2607.13639)
- Selected GGUF distribution: [Abiray/OvisOCR2-GGUF](https://huggingface.co/Abiray/OvisOCR2-GGUF)
- Selected GGUF revision: `8a22290a0c42dbcf84739d6d4c2763f877494ae0`
- Project manifest: [`assets/manifests/ovisocr2/`](assets/manifests/ovisocr2/)
- Declared model license: Apache License 2.0 (`Apache-2.0`), as declared by the upstream model repositories at the time of review.

The GGUF model and `mmproj` files are not part of the OCR-CLI source-code license. Users and distributors must retain the upstream model metadata and comply with the terms of the exact model revision being used. Model files must not be redistributed with an OCR-CLI release unless their provenance, license, and redistribution status have been reviewed for that release.

OvisOCR2 is based on the Qwen model family. See the [Qwen organization](https://huggingface.co/Qwen) and the specific upstream base-model card for the license and terms applicable to the relevant model lineage.

## llama.cpp and libmtmd

OCR-CLI uses llama.cpp through Rust bindings for local inference and uses the llama.cpp multimodal/MTMD functionality to process document images and the multimodal projector.

- Project: [ggml-org/llama.cpp](https://github.com/ggml-org/llama.cpp)
- Multimodal documentation: [libmtmd / MTMD documentation](https://github.com/ggml-org/llama.cpp/blob/master/tools/mtmd/README.md)
- License: see the upstream [llama.cpp `LICENSE`](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE)
- Additional upstream notices: see the upstream [`licenses/`](https://github.com/ggml-org/llama.cpp/tree/master/licenses) directory

The exact llama.cpp revision and the Rust binding revision must be recorded in the release manifest before publishing a production binary. The current Cargo versions are declared in [`src/ocrx-runtime/Cargo.toml`](src/ocrx-runtime/Cargo.toml).

## PDFium

OCR-CLI uses PDFium for PDF rendering through the [`pdfium-render`](https://crates.io/crates/pdfium-render) Rust crate. Some macOS release archives may contain a `libpdfium.dylib` binary.

- Project: [Chromium PDFium](https://github.com/chromium/pdfium)
- License: [PDFium `LICENSE`](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/2028/LICENSE)
- Chromium third-party license guidance: [Chromium open-source licenses](https://www.chromium.org/chromium-projects/)
- Rust wrapper: [`pdfium-render`](https://crates.io/crates/pdfium-render)

PDFium redistribution requires preserving the applicable copyright notices, license conditions, and disclaimers in source and binary distributions. Because PDFium is connected to the Chromium dependency tree, a release containing `libpdfium.dylib` must be accompanied by the complete notices applicable to the exact PDFium build and revision. The binary revision and SHA-256 must be recorded in the release artifacts.

## Rust dependencies

OCR-CLI is built with Cargo. Direct dependencies are declared in the workspace and crate manifests, including:

- [`image`](https://crates.io/crates/image)
- [`ctrlc`](https://crates.io/crates/ctrlc)
- [`pdfium-render`](https://crates.io/crates/pdfium-render)
- [`llama-cpp-2`](https://crates.io/crates/llama-cpp-2)
- [`llama-cpp-sys-2`](https://crates.io/crates/llama-cpp-sys-2)

Each crate and its transitive dependencies retain their own license terms. The authoritative dependency versions for a build are pinned in [`Cargo.lock`](Cargo.lock). A release process must generate or review a complete Cargo dependency license inventory, including transitive dependencies, and include that inventory with the release artifacts.

## Release checklist

Before publishing a binary or archive, maintainers should verify that the release contains or links to:

1. The complete Apache License 2.0 text for OCR-CLI source code.
2. The exact OvisOCR2 and GGUF model cards, revisions, hashes, and license metadata.
3. The exact llama.cpp revision and the applicable upstream `LICENSE` and `licenses/` notices.
4. The exact PDFium revision, binary hash, license, and Chromium/PDFium third-party notices when a PDFium binary is included.
5. A complete license inventory for Cargo dependencies and their transitive dependencies.
6. Any additional notices required by the selected release binary, model, projector, or packaging artifact.

Links in this file are references to upstream materials, not replacements for license text or notices that must legally accompany a redistributed binary. The release-specific notice bundle must be reviewed against the actual files shipped in that release.
