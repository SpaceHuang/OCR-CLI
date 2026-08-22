#!/bin/sh
set -eu

root="${OCRX_PDF_FIXTURE_ROOT:-benchmarks/fixtures/pdf}"
library="${OCRX_PDFIUM_LIBRARY:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib/libpdfium.dylib}"
binary="${OCRX_CLI_BIN:-target/release/ocrx}"
work_dir="${TMPDIR:-/tmp}/ocrx-cli-negative-scan-check-$$"

cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT HUP INT TERM

python3 scripts/generate-pdf-fixtures.py "$root"
cargo build -p ocrx --release
mkdir -p "$work_dir"

assert_input_error() {
    fixture="$1"
    expected_kind="$2"
    case_name="$3"
    output="$work_dir/$case_name.json"
    if OCRX_PDFIUM_LIBRARY="$library" \
        "$binary" scan "$fixture" --model-profile compact --offline \
        --non-interactive --report json --progress off \
        --output-dir "$work_dir/$case_name-output" >"$output" 2>"$work_dir/$case_name.stderr"; then
        echo "$case_name unexpectedly succeeded" >&2
        exit 1
    fi
    test "$(cat "$output")" != ""
    grep -q '"code":3' "$output"
    grep -q "\"kind\":\"$expected_kind\"" "$output"
    grep -q 'unable to read PDF page count' "$output"
}

assert_input_error "$root/encrypted-password-required.pdf" pdf_encrypted encrypted
assert_input_error "$root/corrupt-truncated.pdf" pdf_corrupt corrupt
assert_input_error "$root/not-a-pdf.txt" pdf_non_pdf non-pdf

printf '%s\n' 'cli-negative-scan-check=PASS'
