#!/bin/sh
set -eu

root="${OCRX_PDF_FIXTURE_ROOT:-benchmarks/fixtures/pdf}"
binary="${OCRX_PDFIUM_CHECK_BIN:-target/release/pdfium-check}"
library_dir="${OCRX_PDFIUM_LIBRARY_DIR:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib}"

python3 scripts/generate-pdf-fixtures.py "$root"
cargo build -p ocrx-pdf --release

for pdf in "$root/normal-1-page.pdf" "$root/normal-100-pages.pdf" "$root/varied-page-sizes.pdf" "$root/complex-20-pages.pdf"; do
    echo "RUN $pdf"
    "$binary" "$library_dir" "$pdf"
done

echo "RUN encrypted fixture without password (must fail)"
if "$binary" "$library_dir" "$root/encrypted-password-required.pdf"; then
    echo "ERROR encrypted PDF unexpectedly opened without password" >&2
    exit 1
fi

echo "RUN encrypted fixture with password (must pass)"
OCRX_PDFIUM_PASSWORD=ocrx-test "$binary" "$library_dir" "$root/encrypted-password-required.pdf"

png_dir="${TMPDIR:-/tmp}/ocrx-pdf-png-check"
rm -rf "$png_dir"
OCRX_PDFIUM_SAVE_DIR="$png_dir" "$binary" "$library_dir" "$root/complex-20-pages.pdf"
test -f "$png_dir/page-0001.png"
file "$png_dir/page-0001.png" | grep -q 'PNG image data'
printf 'png_sha256=%s\n' "$(shasum -a 256 "$png_dir/page-0001.png" | awk '{print $1}')"
rm -rf "$png_dir"

echo "RUN corrupt fixture (must fail)"
if "$binary" "$library_dir" "$root/corrupt-truncated.pdf"; then
    echo "ERROR corrupt PDF unexpectedly succeeded" >&2
    exit 1
fi

echo "RUN non-PDF fixture (must fail)"
if "$binary" "$library_dir" "$root/not-a-pdf.txt"; then
    echo "ERROR non-PDF unexpectedly succeeded" >&2
    exit 1
fi

echo "fixture-check=PASS"
