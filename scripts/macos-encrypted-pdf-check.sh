#!/bin/sh
set -eu

encrypted="${OCRX_ENCRYPTED_PDF:-/private/tmp/ocrx-p0-20260802/encrypted.pdf}"
library_dir="${OCRX_PDFIUM_LIBRARY_DIR:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib}"
binary="${OCRX_PDFIUM_CHECK_BIN:-target/release/pdfium-check}"
test -f "$encrypted"
cargo build -p ocrx-pdf --release >/dev/null

echo 'RUN encrypted PDF without password (must fail)'
if "$binary" "$library_dir" "$encrypted"; then
    echo 'encrypted PDF unexpectedly opened without password' >&2
    exit 1
fi

echo 'RUN encrypted PDF with password (must pass)'
OCRX_PDFIUM_PASSWORD="${OCRX_ENCRYPTED_PDF_PASSWORD:-ocrx-test}" \
    "$binary" "$library_dir" "$encrypted"
printf '%s\n' 'encrypted-pdf-check=PASS'
