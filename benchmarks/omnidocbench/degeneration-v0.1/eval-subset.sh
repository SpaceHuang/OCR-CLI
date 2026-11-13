#!/bin/bash
# One-command subset scoring round (quality plan Q1.0 fast eval loop).
set -u
cd "$(dirname "$0")/../../.." || exit 2
PY=benchmarks/omnidocbench/.venv/Scripts/python.exe
"$PY" scripts/run-omnidocbench-eval.py --benchmark-root benchmarks/omnidocbench \
  --gt benchmarks/omnidocbench/degeneration-v0.1/truth-subset.json \
  --predictions benchmarks/omnidocbench/degeneration-v0.1/predictions \
  --name degeneration-v0.1 --workers 8 || exit 1
echo SUBSET-EVAL-DONE
