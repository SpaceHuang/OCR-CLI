#!/bin/sh
set -eu

binary="${OCRX_PDFIUM_CHECK_BIN:-target/release/pdfium-check}"
library_dir="${OCRX_PDFIUM_LIBRARY_DIR:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib}"
cargo build -p ocrx-pdf --release >/dev/null

for pdf in test-data/*/*/*.pdf; do
    pages=$(pdfinfo "$pdf" | awk '/^Pages:/ {print $2}')
    printf 'REAL_PDF file=%s pages=%s\n' "$pdf" "$pages"
    for page in 0 $((pages / 2)) $((pages - 1)); do
        OCRX_PDFIUM_PAGE="$page" "$binary" "$library_dir" "$pdf"
    done
done
printf '%s\n' 'real-pdf-check=PASS sampled first/middle/last pages for all repository PDFs'
