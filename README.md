# OCR-CLI (`ocrx`)

OCR-CLI is a local Rust command-line tool for extracting structured content from PDF documents with OvisOCR2. It is designed to run locally, keep document data on the host, and produce Markdown-oriented OCR artifacts suitable for further processing by humans or automation.

> **Status: development preview**
>
> This repository is not yet a production release. Version 0.1 is currently being validated on macOS arm64. Runtime reproducibility, PDFium packaging, long-document resource behavior, model quality baselines, and final release licensing artifacts are still being frozen.

## Scope

The current v0.1 scope is intentionally narrow:

- One supported OCR model: [OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2).
- Local PDF input and page rendering through PDFium.
- Multimodal inference through llama.cpp/libmtmd and a separate `mmproj` GGUF file.
- OvisOCR2 profiles: `compact`, `balanced`, `quality`, and `reference`.
- Markdown, JSON, progress, resume, and diagnostic workflows exposed by the CLI.
- A macOS arm64 release/Skill packaging path.

Other OCR models are not part of the v0.1 public contract. In particular, the project does not claim support for HunyuanOCR or other model families.

## Current implementation status

The repository contains the Rust workspace, CLI entry point, PDFium integration, llama.cpp/MTMD runtime integration, OvisOCR2 model manifest, output types, validation scripts, and an Agent Skill definition. Some production gates remain open; do not interpret the existence of an implementation or test fixture as a production-support guarantee.

Known release work is tracked in [`docs/plan/pre-development-macos-freeze-plan.md`](docs/plan/pre-development-macos-freeze-plan.md), including runtime revision alignment, PDFium release validation, resource limits, model provenance, and quality evidence.

## Requirements

For the current development path, use:

- macOS arm64;
- Rust `1.85` or a compatible toolchain selected by [`rust-toolchain.toml`](rust-toolchain.toml);
- a compatible arm64 PDFium dynamic library when running the PDF pipeline;
- the OvisOCR2 model GGUF and its multimodal projector (`mmproj`) installed separately.

The packaged archive does not include model weights. Model files are large, revisioned artifacts and are managed separately so that users can verify their provenance and license status.

## Build and bootstrap checks

Clone the repository and run:

```bash
cargo build --workspace
cargo test --workspace
cargo run -p ocrx -- version
cargo run -p ocrx -- doctor
```

The repository also provides focused checks under [`scripts/`](scripts/), including compatibility, regression, release-script, PDFium, and Skill validation helpers.

## CLI usage

The main commands are:

```bash
# Show the CLI version and available help.
cargo run -p ocrx -- version
cargo run -p ocrx -- help

# Inspect the local runtime and model environment.
cargo run -p ocrx -- doctor
cargo run -p ocrx -- doctor --report json

# Inspect or manage the supported OvisOCR2 model.
cargo run -p ocrx -- model list
cargo run -p ocrx -- model info ovisocr2
cargo run -p ocrx -- model recommend

# Scan a PDF using the automatically selected OvisOCR2 profile.
cargo run -p ocrx -- scan /absolute/path/to/input.pdf \
  --model ovisocr2 \
  --model-profile auto \
  --non-interactive
```

For a release binary, replace `cargo run -p ocrx --` with `ocrx`. Use absolute paths for PDF inputs and output directories in automated or Skill workflows. A cancelled scan can be resumed with `--resume`; `--resume` and `--overwrite` are mutually exclusive.

Useful scan options include:

```text
--output-dir <DIR>          Write artifacts to an explicit directory
--pages <SELECTION>        Process selected pages
--model-profile <PROFILE>  auto|compact|balanced|quality|reference
--device <DEVICE>          auto|cpu|metal|cuda|vulkan
--report <FORMAT>          text|json
--progress <MODE>          auto|bar|plain|jsonl|off
--offline                  Do not perform network operations
--non-interactive          Disable interactive prompts
--resume                   Resume an interrupted scan
--overwrite                Allow replacing existing output
--keep-pages               Keep rendered page images
```

The exact behavior of model installation, output schemas, and profile selection remains subject to the v0.1 compatibility and quality gates.

## Windows GPU support (Vulkan)

Windows builds default to the CPU baseline. A Vulkan-enabled binary is built
opt-in and rides the W4.2 evidence path
(`docs/test/reports/quality-gate-ovisocr2-draft-v1-2026-10-30.md`):

```bash
# Build the GPU variant (requires VULKAN_SDK with Bin/glslc.exe on PATH).
cargo build --release -p ocrx --features vulkan
ocrx scan input.pdf --model ovisocr2 --device vulkan --non-interactive

# Package the CPU binary plus a Vulkan variant in one archive.
powershell -File scripts/package-windows-x64.ps1 -GpuPackages vulkan
```

`install.ps1` promotes `bin/ocrx-vulkan.exe` to the default `ocrx.exe` when the
machine reports a Vulkan driver and the variant launches (the CPU binary is
kept as `ocrx-cpu.exe`); otherwise it silently keeps the CPU build. `--device
auto` selects Vulkan only when the backend is compiled in and the probe
reports it; a GPU initialization failure falls back to CPU with a warning.
On the W0.4 30-page gate set, the Vulkan reference scan matched CPU output
page-for-page (mean text similarity 0.9999) at 9.6x the CPU throughput on an
RTX 4090 D. CUDA remains out of scope until the toolkit gate (W4.3) lands.

## Model provenance and licensing

OCR-CLI does not bundle model weights in its release archive. The current manifest identifies the following model assets:

| Asset | Source and revision | License/status |
| --- | --- | --- |
| OvisOCR2 base model | [ATH-MaaS/OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2) | Apache-2.0 as declared by the model card |
| GGUF quantization | [Abiray/OvisOCR2-GGUF](https://huggingface.co/Abiray/OvisOCR2-GGUF), revision `8a22290a0c42dbcf84739d6d4c2763f877494ae0` | Apache-2.0 as declared by the model repository; verify the upstream terms before redistribution |
| Multimodal projector | `mmproj-F16.gguf` from the selected GGUF repository | Must be kept with its source and license metadata |
| Base-model lineage | [Qwen/Qwen3.5-0.8B](https://huggingface.co/Qwen) | Refer to the applicable upstream model card and license |

The machine-readable manifest, including profile sizes, source URLs, and SHA-256 values, is stored at [`assets/manifests/ovisocr2/`](assets/manifests/ovisocr2/). A model license is separate from the OCR-CLI source-code license. Users and distributors are responsible for complying with the terms that apply to each model file they download or redistribute.

## PDFium and runtime dependencies

OCR-CLI uses the following important upstream components:

- [PDFium](https://github.com/chromium/pdfium), accessed through [`pdfium-render`](https://crates.io/crates/pdfium-render), for PDF rendering;
- [llama.cpp](https://github.com/ggml-org/llama.cpp) and its [multimodal/libmtmd support](https://github.com/ggml-org/llama.cpp/blob/master/tools/mtmd/README.md), accessed through Rust bindings, for local multimodal inference;
- Rust ecosystem dependencies listed in [`Cargo.toml`](Cargo.toml) and [`Cargo.lock`](Cargo.lock).

When a release archive contains `lib/libpdfium.dylib`, the archive must include the applicable PDFium and Chromium third-party notices. PDFium redistribution requires preservation of the relevant copyright notices, license conditions, and disclaimers. The final release must also ship a complete third-party license inventory; see the repository release checklist before redistribution.

## Installation and packaging

The current packaging path targets macOS arm64:

```bash
OCRX_PDFIUM_LIBRARY=/absolute/path/to/libpdfium.dylib \
  bash scripts/package-macos-arm64.sh
```

The generated archive contains the CLI binary, the OCR-CLI Skill, the PDFium library, and checksums. It does not contain OvisOCR2 weights. For a local source installation:

```bash
cargo build --release -p ocrx
bash scripts/install.sh
```

The installation scripts are still development-preview tooling. Verify the target architecture, PDFium revision, release checksum, model manifest, and third-party notices before publishing an archive.

## Agent Skill

The Skill definition is available at [`assets/skills/ocrx/SKILL.md`](assets/skills/ocrx/SKILL.md). It instructs an agent to:

1. run `ocrx doctor --report json` first;
2. use only an installed local OvisOCR2 profile;
3. keep PDF and OCR data local;
4. use absolute input and output paths;
5. request human verification of OCR output and extracted images.

The Skill does not download model files automatically.

## Limitations and safety

OCR output is probabilistic and may contain incorrect or incomplete text, formulas, tables, reading order, or visual-region coordinates. Results for important documents must be manually reviewed. Do not rely on OCR-CLI as the sole source of truth for legal, financial, medical, compliance, or archival decisions.

PDF complexity can affect memory use and runtime. Encrypted PDFs, malformed PDFs, unusual page sizes, and very long documents may require additional validation. The project does not currently promise a stable resource ceiling for every real-world PDF.

The official OvisOCR2 model card also notes that parsing errors can occur despite filtering and quality-assurance procedures. See the [OvisOCR2 model card](https://huggingface.co/ATH-MaaS/OvisOCR2) for model-specific limitations.

## Acknowledgements

OCR-CLI would not be possible without the following projects and contributors:

- [ATH-MaaS/OvisOCR2](https://huggingface.co/ATH-MaaS/OvisOCR2) and the authors of the [OvisOCR2 technical report](https://arxiv.org/abs/2607.13639);
- [Abiray/OvisOCR2-GGUF](https://huggingface.co/Abiray/OvisOCR2-GGUF) for the selected GGUF quantization and multimodal projector distribution;
- [Qwen](https://huggingface.co/Qwen) for the base-model lineage used by OvisOCR2;
- [llama.cpp](https://github.com/ggml-org/llama.cpp) and its contributors for local inference and libmtmd;
- [PDFium](https://github.com/chromium/pdfium), Chromium contributors, and the [`pdfium-render`](https://crates.io/crates/pdfium-render) project;
- the Rust compiler, Cargo, and the open-source crates listed in the project dependency metadata.

If you use OvisOCR2 in academic work, please cite the technical report using the citation provided by the [official model card](https://huggingface.co/ATH-MaaS/OvisOCR2#citation).

## License

The OCR-CLI source code is intended to be distributed under the Apache License, Version 2.0, as declared in the Rust workspace metadata. Principal third-party components and the release review checklist are documented in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). The repository must still include the complete Apache-2.0 license text and a release-specific review of third-party notices before a public release is published.

Model weights, PDFium binaries, llama.cpp-derived components, and other dependencies may be subject to separate licenses and notices. See the upstream sources and the final release's third-party attribution files for the terms that apply to each component.

## Further documentation

- [OvisOCR2 compatibility matrix](assets/compatibility/ovisocr2-macos-arm64.json)
- [Regression-set format](assets/regression/README.md)

Implementation plans and internal development notes are maintained in the repository for contributors but are not part of the end-user documentation.
