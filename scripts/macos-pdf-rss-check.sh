#!/bin/sh
set -eu

root="${OCRX_PDF_FIXTURE_ROOT:-benchmarks/fixtures/pdf}"
binary="${OCRX_PDFIUM_CHECK_BIN:-target/release/pdfium-check}"
library_dir="${OCRX_PDFIUM_LIBRARY_DIR:-/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib}"
input="${OCRX_PDFIUM_RSS_INPUT:-$root/normal-100-pages.pdf}"
delay_ms="${OCRX_PDFIUM_RSS_DELAY_MS:-20}"
reopen="${OCRX_PDFIUM_REOPEN_EACH_PAGE:-}"
segment_pages="${OCRX_PDFIUM_SEGMENT_PAGES:-}"
quiet="${OCRX_PDFIUM_RSS_QUIET:-}"
log_file="${TMPDIR:-/tmp}/ocrx-pdf-rss.$$.log"
trap 'rm -f "$log_file"' EXIT

python3 scripts/generate-pdf-fixtures.py "$root" >/dev/null
cargo build -p ocrx-pdf --release >/dev/null
OCRX_PDFIUM_CHECK_DELAY_MS="$delay_ms" OCRX_PDFIUM_REOPEN_EACH_PAGE="$reopen" OCRX_PDFIUM_SEGMENT_PAGES="$segment_pages" "$binary" "$library_dir" "$input" >"$log_file" 2>&1 &
pid=$!
printf 'pid=%s\n' "$pid"
samples=0
min_rss=""
max_rss=""
last_rss=""
while kill -0 "$pid" 2>/dev/null; do
    rss=$(ps -o rss= -p "$pid" | tr -d ' ' || true)
    if [ -n "$rss" ]; then
        if [ -z "$quiet" ]; then printf '%s\n' "$rss"; fi
        if [ -z "$min_rss" ] || [ "$rss" -lt "$min_rss" ]; then min_rss="$rss"; fi
        if [ -z "$max_rss" ] || [ "$rss" -gt "$max_rss" ]; then max_rss="$rss"; fi
        last_rss="$rss"
        samples=$((samples + 1))
    fi
    sleep 0.05
done
wait "$pid"
if [ "$samples" -lt 3 ]; then
    echo "BLOCKED: insufficient RSS samples ($samples)" >&2
    exit 2
fi
printf 'rss_samples=%s min_kb=%s max_kb=%s last_kb=%s\n' "$samples" "$min_rss" "$max_rss" "$last_rss"
printf '%s\n' 'rss-check=PASS sampling available; interpret trend with the saved samples'
