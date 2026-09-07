#!/bin/sh
set -eu

# Emits a machine-readable report. Missing external assets are an honest
# not_run result so CI can distinguish infrastructure gaps from regressions.
binary="${OCRX_PDFIUM_CHECK_BIN:-target/release/pdfium-check}"
library_dir="${OCRX_PDFIUM_LIBRARY_DIR:-}"
input="${OCRX_PDFIUM_RSS_INPUT:-}"
run_count="${OCRX_PDFIUM_PERFORMANCE_RUNS:-5}"

case "$run_count" in
    ''|*[!0-9]*)
        printf '%s\n' '{"status":"failed","reason":"invalid_run_count"}'
        exit 2
        ;;
esac
if [ "$run_count" -lt 1 ]; then
    printf '%s\n' '{"status":"failed","reason":"invalid_run_count"}'
    exit 2
fi

if [ ! -x "$binary" ] || [ -z "$library_dir" ] || [ ! -d "$library_dir" ] || [ -z "$input" ] || [ ! -f "$input" ]; then
    printf '%s\n' '{"status":"not_run","reason":"missing_pdfium_binary_library_or_input"}'
    exit 0
fi

log_file="${TMPDIR:-/tmp}/ocrx-pdf-performance.$$.log"
trap 'rm -f "$log_file"' EXIT
run_reports=""
run=1
while [ "$run" -le "$run_count" ]; do
    : >"$log_file"
    OCRX_PDFIUM_JSON=1 "$binary" "$library_dir" "$input" >"$log_file" 2>&1 &
    pid=$!
    samples=0
    min_rss=""
    max_rss=""
    while kill -0 "$pid" 2>/dev/null; do
        rss=$(ps -o rss= -p "$pid" | tr -d ' ' || true)
        if [ -n "$rss" ]; then
            [ -z "$min_rss" ] || [ "$rss" -ge "$min_rss" ] || min_rss="$rss"
            [ -n "$min_rss" ] || min_rss="$rss"
            [ -z "$max_rss" ] || [ "$rss" -le "$max_rss" ] || max_rss="$rss"
            [ -n "$max_rss" ] || max_rss="$rss"
            samples=$((samples + 1))
        fi
        sleep 0.05
    done
    if ! wait "$pid"; then
        printf '%s\n' '{"status":"failed","reason":"pdfium_probe_failed"}'
        exit 1
    fi
    if [ "$samples" -lt 1 ]; then
        printf '%s\n' '{"status":"not_run","reason":"rss_sampling_unavailable"}'
        exit 0
    fi
    probe=$(tail -n 1 "$log_file")
    report=$(printf '{"run":%s,"rss_samples":%s,"min_rss_kb":%s,"max_rss_kb":%s,"probe":%s}' \
        "$run" "$samples" "$min_rss" "$max_rss" "$probe")
    if [ -n "$run_reports" ]; then run_reports="$run_reports,$report"; else run_reports="$report"; fi
    run=$((run + 1))
done
printf '{"status":"passed","runs":%s,"reports":[%s]}\n' "$run_count" "$run_reports"
