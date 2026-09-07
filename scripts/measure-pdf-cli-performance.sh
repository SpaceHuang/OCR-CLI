#!/bin/sh
set -eu

# Measures a real ocrx scan while sampling peak RSS. The command is intentionally
# explicit so the report remains reproducible and does not depend on shell state.
binary=${OCRX_CLI_BINARY:-target/release/ocrx}
input=${OCRX_PERF_INPUT:?set OCRX_PERF_INPUT}
pages=${OCRX_PERF_PAGES:?set OCRX_PERF_PAGES}
output_root=${OCRX_PERF_OUTPUT_ROOT:?set OCRX_PERF_OUTPUT_ROOT}
profile=${OCRX_PERF_PROFILE:-quality}
mode=${OCRX_PERF_MODE:-ocr}
device=${OCRX_PERF_DEVICE:-metal}
run_count=${OCRX_PERF_RUNS:-5}

case "$run_count" in ''|*[!0-9]*) echo '{"status":"failed","reason":"invalid_run_count"}'; exit 2;; esac
[ "$run_count" -gt 0 ] || { echo '{"status":"failed","reason":"invalid_run_count"}'; exit 2; }

mkdir -p "$output_root"
run=1
reports=""
while [ "$run" -le "$run_count" ]; do
    output="$output_root/run-$run"
    log="$output_root/run-$run.log"
    rm -rf "$output"
    start=$(date +%s)
    OCRX_PDFIUM_LIBRARY="${OCRX_PDFIUM_LIBRARY:?set OCRX_PDFIUM_LIBRARY}" \
        "$binary" scan "$input" --pages "$pages" --pdf-mode "$mode" \
        --model-profile "$profile" --device "$device" --non-interactive --offline \
        --output-dir "$output" --report json --progress off >"$log" 2>&1 &
    pid=$!
    peak=0
    while kill -0 "$pid" 2>/dev/null; do
        rss=$(ps -o rss= -p "$pid" | tr -d ' ' || true)
        case "$rss" in ''|*[!0-9]*) ;; *) [ "$rss" -gt "$peak" ] && peak="$rss";; esac
        sleep 0.2
    done
    wait "$pid" || true
    end=$(date +%s)
    result=$(tail -n 1 "$log")
    size=$(du -sk "$output" 2>/dev/null | awk '{print $1 * 1024}')
    report=$(printf '{"run":%s,"wall_seconds":%s,"peak_rss_kb":%s,"output_size_bytes":%s,"result":%s}' \
        "$run" "$((end - start))" "$peak" "${size:-0}" "$result")
    [ -n "$reports" ] && reports="$reports,$report" || reports="$report"
    run=$((run + 1))
done
summary=$(printf '{"schema_version":1,"status":"passed","runs":[%s]}\n' "$reports")
printf '%s\n' "$summary" >"$output_root/report.json"
printf '%s\n' "$summary"
