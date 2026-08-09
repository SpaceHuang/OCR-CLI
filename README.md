# OCR CLI (`ocrx`)

Rust workspace for OCR CLI v0.1. The repository is currently in pre-development bootstrap: the command-line entry point and crate boundaries exist, while real PDFium, llama.cpp/MTMD, model management, output, and Skill implementations are gated by the freeze plan.

## Bootstrap checks

```bash
cargo build --workspace
cargo test --workspace
cargo run -p ocrx -- version
cargo run -p ocrx -- doctor
```

See [`docs/plan/pre-development-macos-freeze-plan.md`](docs/plan/pre-development-macos-freeze-plan.md) for the development entry criteria.
