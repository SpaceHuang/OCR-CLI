#!/usr/bin/env python3
"""Run the deterministic scorer for fixed profile/DPI/pixel-budget cases."""
import json, subprocess, sys
from pathlib import Path

CASES = [(p, dpi, budget) for p in ("compact", "balanced", "quality", "reference") for dpi in (200, 250, 300) for budget in (1048576,)]

def main():
    if len(sys.argv) != 4:
        print("usage: run-quality-gate.py MANIFEST TRUTH_ROOT PREDICTION_ROOT", file=sys.stderr); return 2
    manifest, truth, predictions = map(Path, sys.argv[1:])
    report = {"schema_version": 1, "cases": []}
    for profile, dpi, budget in CASES:
        pred = predictions / profile / str(dpi)
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("score-regression.py")), str(manifest), str(truth), str(pred)], capture_output=True, text=True)
        if result.returncode != 0:
            print(result.stderr, file=sys.stderr); return result.returncode
        report["cases"].append({"profile": profile, "dpi": dpi, "pixel_budget": budget, "score": json.loads(result.stdout)})
    print(json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
    return 0

if __name__ == "__main__": raise SystemExit(main())
