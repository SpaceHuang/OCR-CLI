#!/bin/sh
set -eu

cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
python -m unittest discover -s scripts -p "test_*.py"
python scripts/validate-architecture-boundaries.py
bash scripts/test-release-scripts.sh
cargo run -p ocrx -- version
cargo run -p ocrx -- doctor
