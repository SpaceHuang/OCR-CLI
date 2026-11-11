#!/bin/bash
set -u
cd "$(dirname "$0")"
PY=./.venv/Scripts/python.exe
for split in all en zh; do
  case $split in
    all) filt="";;
    en) filt="--filter language=english";;
    zh) filt="--filter language=simplified_chinese";;
  esac
  echo "=== eval balanced-$split $(date -Is) ==="
  "$PY" ../../scripts/run-omnidocbench-eval.py --benchmark-root . \
    --gt dataset/OmniDocBench.json --predictions predictions/balanced \
    --name "omnidocbench-balanced-$split" --workers 8 $filt \
    || echo "EVAL-FAIL balanced-$split"
done
echo "BALANCED-EVAL-DONE $(date -Is)"
