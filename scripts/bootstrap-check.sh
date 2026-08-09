#!/bin/sh
set -eu

cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo run -p ocrx -- version
cargo run -p ocrx -- doctor
