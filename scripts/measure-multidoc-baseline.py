#!/usr/bin/env python3
"""Measure the multi-document scan baseline: one `ocrx scan` per document.

The X1 gate (plan §4 X1.4④) compares a single orchestrated invocation
against this script's wall clock on the same fixtures, binary, and device.
Each document is scanned into a fresh output dir; per-document elapsed_ms
comes from result.json (excludes CLI process spin-up only) and the wall
clock of the whole loop is recorded separately (process + model load +
verify per call is exactly the fixed tax X1 amortizes).

Usage:
  python scripts/measure-multidoc-baseline.py --ocrx <path-to-ocrx> \
      [--scenario md-small-20] [--label vulkan-reference] [--runs 1]

Output: docs/test/reports/phase2-t0-<date>-data/multidoc-<scenario>-<label>.json
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = REPO_ROOT / "assets" / "regression" / "multidoc-v1"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ocrx", required=True, type=Path)
    parser.add_argument("--scenario", default="md-small-20", choices=["md-small-20", "md-mixed"])
    parser.add_argument("--label", default="run", help="device/profile tag for the output file")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--profile", default="reference")
    parser.add_argument("--output-root", type=Path, default=None)
    parser.add_argument("--extra-args", nargs="*", default=[])
    args = parser.parse_args()

    scenario_dir = FIXTURE_ROOT / args.scenario
    documents = sorted(scenario_dir.glob("doc-*.pdf"))
    if not documents:
        print(f"no fixtures under {scenario_dir}; run scripts/build-multidoc-fixtures.py", file=sys.stderr)
        return 2
    ocrx = args.ocrx.resolve()
    if not ocrx.is_file():
        print(f"ocrx binary not found: {ocrx}", file=sys.stderr)
        return 2

    date_tag = dt.date.today().isoformat()
    data_dir = REPO_ROOT / "docs" / "test" / "reports" / f"phase2-t0-{date_tag}-data"
    data_dir.mkdir(parents=True, exist_ok=True)
    out_path = data_dir / f"multidoc-{args.scenario}-{args.label}.json"

    run_records = []
    for run_index in range(args.runs):
        output_root = args.output_root or Path(tempfile.mkdtemp(prefix="ocrx-md-baseline-"))
        output_root.mkdir(parents=True, exist_ok=True)
        loop_started = time.perf_counter()
        per_document = []
        for index, document in enumerate(documents):
            output_dir = output_root / f"run-{run_index}" / f"doc-{index:02d}"
            started = time.perf_counter()
            command = [
                str(ocrx),
                "scan",
                str(document),
                "--model-profile",
                args.profile,
                "--output-dir",
                str(output_dir),
                "--non-interactive",
                *args.extra_args,
            ]
            completed = subprocess.run(command, capture_output=True, text=True)
            elapsed = time.perf_counter() - started
            exit_code = completed.returncode
            per_document.append(
                {
                    "file": document.name,
                    "exit_code": exit_code,
                    "wall_s": round(elapsed, 3),
                }
            )
            status = "ok" if exit_code == 0 else f"exit {exit_code}"
            print(f"run {run_index} {document.name}: {elapsed:.2f}s ({status})")
            if exit_code not in (0, 10):
                print(completed.stdout[-2000:], file=sys.stderr)
                print(completed.stderr[-2000:], file=sys.stderr)
        loop_wall = time.perf_counter() - loop_started
        run_records.append(
            {
                "run": run_index,
                "loop_wall_s": round(loop_wall, 3),
                "documents": per_document,
            }
        )
        print(f"run {run_index} loop wall: {loop_wall:.2f}s")
        if args.output_root is None:
            shutil.rmtree(output_root, ignore_errors=True)

    record = {
        "schema_version": 1,
        "tool": "scripts/measure-multidoc-baseline.py",
        "mode": "individual-invocations",
        "binary": str(ocrx),
        "scenario": args.scenario,
        "profile": args.profile,
        "extra_args": args.extra_args,
        "document_count": len(documents),
        "runs": run_records,
    }
    out_path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
