#!/bin/sh
set -eu

library_dir="${OCRX_PDFIUM_LIBRARY_DIR:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib}"
pdf="${1:-test-data/苏科版-江苏凤凰科学技术出版社/七年级/义务教育教科书·数学七年级上册.pdf}"
out_root="${TMPDIR:-/tmp}/ocrx-pdf-workers.$$"
worker="${OCRX_PDFIUM_WORKER_BIN:-target/release/pdfium-worker}"
trap 'rm -rf "$out_root"' EXIT

cargo build -p ocrx-pdf --release >/dev/null
mkdir -p "$out_root"
pages=$(pdfinfo "$pdf" | awk '/^Pages:/ {print $2}')
segment=20
start=0
workers=0
while [ "$start" -lt "$pages" ]; do
    end=$((start + segment))
    [ "$end" -gt "$pages" ] && end="$pages"
    dir="$out_root/segment-$workers"
    mkdir -p "$dir"
    "$worker" "$library_dir" "$pdf" "$start" "$end" "$dir" >/dev/null
    rendered=$(find "$dir" -type f -name '*.png' -print | wc -l | tr -d ' ')
    expected=$((end - start))
    [ "$rendered" -eq "$expected" ]
    test -z "$(find "$dir" -type f -name '*.part.png' -print -quit)"
    workers=$((workers + 1))
    start="$end"
done
printf 'worker_processes=%s pages=%s\n' "$workers" "$pages"
printf '%s\n' 'pdf-worker-check=PASS each segment rendered in an isolated process'

printf '%s\n' 'RUN invalid worker range (must fail)'
if "$worker" "$library_dir" "$pdf" 1 1 "$out_root/invalid" >/dev/null 2>&1; then
    echo 'invalid worker range unexpectedly succeeded' >&2
    exit 1
fi
printf '%s\n' 'pdf-worker-invalid-range-check=PASS'
