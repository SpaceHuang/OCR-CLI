#!/bin/sh
set -eu

artifact_root="${OCRX_P0_ARTIFACT_ROOT:-/private/tmp/ocrx-p0-20260802}"
failures=0

check_file() {
    path="$1"
    if [ -f "$path" ]; then
        printf 'PASS file %s\n' "$path"
    else
        printf 'BLOCKED missing %s\n' "$path"
        failures=$((failures + 1))
    fi
}

printf '%s\n' 'ocrx macOS P0 evidence report'
printf 'artifact_root=%s\n' "$artifact_root"
sh "$(dirname "$0")/macos-environment.sh"

check_file "$artifact_root/smoke-page.pdf"
check_file "$artifact_root/smoke-100-pages.pdf"
check_file "$artifact_root/corrupt-truncated.pdf"
check_file "$artifact_root/encrypted.pdf"
check_file "$artifact_root/pdfium-chromium-7961/lib/libpdfium.dylib"
check_file "$artifact_root/OvisOCR2.Q4_K_M.gguf"
check_file "$artifact_root/OvisOCR2.mmproj-bf16.gguf"

if [ -f "$artifact_root/smoke-page.pdf" ]; then
    printf 'smoke_page_sha256=%s\n' "$(shasum -a 256 "$artifact_root/smoke-page.pdf" | awk '{print $1}')"
fi
if [ -f "$artifact_root/smoke-100-pages.pdf" ]; then
    printf 'smoke_100_pages_sha256=%s\n' "$(shasum -a 256 "$artifact_root/smoke-100-pages.pdf" | awk '{print $1}')"
fi

if [ -x "$artifact_root/rust-pdfium-smoke/target/release/rust-pdfium-smoke" ]; then
    printf '%s\n' 'RUN pdfium 300 DPI smoke'
    "$artifact_root/rust-pdfium-smoke/target/release/rust-pdfium-smoke" "$artifact_root/smoke-page.pdf" --no-save
    printf '%s\n' 'RUN pdfium 100-page smoke'
    "$artifact_root/rust-pdfium-smoke/target/release/rust-pdfium-smoke" "$artifact_root/smoke-100-pages.pdf" --no-save
else
    printf '%s\n' 'BLOCKED pdfium smoke executable missing; rebuild before claiming fresh render results'
    failures=$((failures + 1))
fi

if [ "$failures" -ne 0 ]; then
    printf 'result=BLOCKED checks=%s\n' "$failures"
    exit 2
fi
printf '%s\n' 'result=PASS for available historical P0 evidence; this does not freeze CLI resource behavior'
