"""Scan-throughput measurement harness (P0 baseline protocol, plan §3).

Runs `ocrx scan` against a fixture with a fresh output directory per
repetition, captures wall-clock time, and collects `result.json` plus the
opt-in `gpu-resource.json` (OCRX_GPU_REPORT=1) into one JSON record per run.
No production code is involved; the harness only drives the released CLI.

Usage:
  python scripts/measure-scan-throughput.py \
      --ocrx D:/ocrx-tgt-p0/release/ocrx.exe \
      --input benchmarks/fixtures/pdf/figure-1-page.pdf \
      --profile reference --max-output-tokens 2048 \
      --repeat 3 --label r05-t2048 \
      --out docs/test/reports/perf-baseline-2026-11-01-data

The record layout is stable so report generation can be re-run offline:
  { label, run_index, exit_code, wall_ms, result: {...}, gpu_report: {...} }
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path


def run_once(ocrx: Path, args: list[str], out_root: Path, label: str, index: int) -> dict:
    output_dir = out_root / f"{label}-{index:02d}"
    if output_dir.exists():
        shutil.rmtree(output_dir)
    cmd = [str(ocrx), "scan", str(args.pop("input")), "--output-dir", str(output_dir)]
    for key, value in args.items():
        if value is None:
            continue
        flag = "model-profile" if key == "profile" else key.replace("_", "-")
        cmd += [f"--{flag}", str(value)]
    env = {**__import__("os").environ, "OCRX_GPU_REPORT": "1"}
    started = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=7200)
    wall_ms = round((time.perf_counter() - started) * 1000)
    record: dict = {
        "label": label,
        "run_index": index,
        "exit_code": proc.returncode,
        "wall_ms": wall_ms,
        "command": cmd,
    }
    result_path = output_dir / "result.json"
    if result_path.exists():
        record["result"] = json.loads(result_path.read_text(encoding="utf-8"))
    else:
        record["result_error"] = "result.json missing"
        record["stderr_tail"] = proc.stderr[-2000:]
    gpu_path = output_dir / "gpu-resource.json"
    if gpu_path.exists():
        record["gpu_report"] = json.loads(gpu_path.read_text(encoding="utf-8"))
    return record


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ocrx", required=True, type=Path)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--profile", default="reference")
    parser.add_argument("--pages", default=None)
    parser.add_argument("--max-output-tokens", dest="max_output_tokens", default=None)
    parser.add_argument("--device", default=None)
    parser.add_argument("--pdf-mode", dest="pdf_mode", default=None)
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--label", required=True)
    parser.add_argument("--out", required=True, type=Path)
    ns = parser.parse_args()

    args = {
        "input": ns.input.resolve(),
        "profile": ns.profile,
        "pages": ns.pages,
        "max_output_tokens": ns.max_output_tokens,
        "device": ns.device,
        "pdf_mode": ns.pdf_mode,
    }
    ns.out.mkdir(parents=True, exist_ok=True)
    records = []
    for index in range(1, ns.repeat + 1):
        record = run_once(ns.ocrx, dict(args), ns.out, ns.label, index)
        records.append(record)
        result = record.get("result", {})
        gpu = record.get("gpu_report", {})
        print(
            f"[{ns.label} #{index}] exit={record['exit_code']} wall_ms={record['wall_ms']} "
            f"elapsed_ms={result.get('elapsed_ms')} tokens={gpu.get('output_tokens')} "
            f"tok/s={gpu.get('output_tokens_per_sec')} load_ms={gpu.get('load_time_ms')} "
            f"first_token_ms={gpu.get('first_token_ms')}",
            flush=True,
        )
    out_file = ns.out / f"{ns.label}.json"
    out_file.write_text(json.dumps(records, indent=2), encoding="utf-8")
    print(f"records -> {out_file}")
    return 0 if all(r["exit_code"] == 0 for r in records) else 1


if __name__ == "__main__":
    sys.exit(main())
