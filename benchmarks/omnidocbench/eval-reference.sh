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
  echo "=== eval reference-$split $(date -Is) ==="
  "$PY" ../../scripts/run-omnidocbench-eval.py --benchmark-root . \
    --gt dataset/OmniDocBench.json --predictions predictions/reference \
    --name "omnidocbench-reference-$split" --workers 4 $filt \
    || echo "EVAL-FAIL reference-$split"
done
echo "REFERENCE-EVAL-DONE $(date -Is)"
