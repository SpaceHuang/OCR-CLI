#!/bin/sh
set -eu

library="${OCRX_PDFIUM_LIBRARY:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib/libpdfium.dylib}"
expected_sha256="${OCRX_PDFIUM_SHA256:-}"
test -f "$library"
file "$library"
file "$library" | grep -q 'arm64'
otool -L "$library"
hash=$(shasum -a 256 "$library" | awk '{print $1}')
printf 'pdfium_sha256=%s\n' "$hash"
if [ -n "$expected_sha256" ] && [ "$hash" != "$expected_sha256" ]; then
    echo "PDFium hash mismatch" >&2
    exit 1
fi
printf '%s\n' 'pdfium-binary-check=PASS'

echo 'RUN missing PDFium library (must fail)'
if OCRX_PDFIUM_LIBRARY_DIR="/private/tmp/ocrx-p0-20260802/does-not-exist" \
    target/release/pdfium-check \
    /private/tmp/ocrx-p0-20260802/does-not-exist \
    benchmarks/fixtures/pdf/normal-1-page.pdf; then
    echo 'missing PDFium library unexpectedly succeeded' >&2
    exit 1
fi
printf '%s\n' 'pdfium-missing-library-check=PASS'
