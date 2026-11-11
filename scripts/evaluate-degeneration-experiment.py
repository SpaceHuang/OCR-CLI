#!/usr/bin/env python3
"""Q1.1 corpus escape-rate evaluator (quality plan §4.1 adjudication line).

Compares an experiment scan of the frozen 213-page degeneration corpus
against the baseline ledger:

- escape rate = corpus pages committed non-degenerated in the experiment scan
  (denominator = 213; plan BLK-2). Landing line: >= 30% for Q1.1 ladder
  candidates, >= 50% for Q1.2 sampling.
- attempts distribution and the escaped/still-degenerated page lists.
- optionally emits the escaped-page subset prediction export + truth subset
  so the official scorer can verify "escaped AND scored right" (an escape
  only counts when the page passes the official same-granularity subset
  scoring).

Usage:
  python scripts/evaluate-degeneration-experiment.py \
      --benchmark-root benchmarks/omnidocbench \
      --scan scans/degeneration-v0.1-tokens2048 --label tokens2048
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]


def decode(value: str) -> str:
    if value and len(value) % 2 == 0:
        try:
            return bytes.fromhex(value).decode("utf-8", "strict")
        except ValueError:
            return value
    return value


def read_checkpoint(path: Path) -> dict:
    fields: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, _, value = line.partition("=")
        fields[key] = decode(value)
    return fields


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark-root", type=Path, default=Path("benchmarks/omnidocbench"))
    parser.add_argument("--scan", type=Path, required=True, help="experiment scan root")
    parser.add_argument("--label", required=True, help="report label, e.g. tokens2048")
    parser.add_argument(
        "--emit-subset", action="store_true",
        help="export escaped-page predictions + truth subset for official scoring",
    )
    args = parser.parse_args()

    root = args.benchmark_root
    asset = REPO / "assets/regression/omnidocbench-degeneration-v0.1/pages.json"
    corpus = json.loads(asset.read_text(encoding="utf-8"))["pages"]

    scan_dirs = {d.name.rsplit("-", 1)[0]: d for d in (root / args.scan).iterdir() if d.is_dir()}
    escaped, still, missing, pending = [], [], [], []
    attempts: dict[str, int] = {}
    for page in corpus:
        scan_dir = scan_dirs.get(page["stem"])
        if scan_dir is None:
            missing.append(page["stem"])
            continue
        if not (scan_dir / "page-0001.checkpoint").is_file():
            pending.append(page["stem"])  # scan still writing this page
            continue
        fields = read_checkpoint(scan_dir / "page-0001.checkpoint")
        attempts[fields.get("attempts", "?")] = attempts.get(fields.get("attempts", "?"), 0) + 1
        if fields.get("degenerated") == "true":
            still.append(page["stem"])
        else:
            escaped.append(
                {
                    "stem": page["stem"],
                    "truncated": fields.get("truncated") == "true",
                    "attempts": fields.get("attempts"),
                    "source_category": page.get("source_category"),
                    "language": page.get("language"),
                }
            )

    n = len(corpus)
    rate = len(escaped) / n if n else 0.0
    by_category: dict[str, list[str]] = {}
    for entry in escaped:
        by_category.setdefault(entry["source_category"] or "?", []).append(entry["stem"])
    category_rates = {
        key: {"escaped": len(value), "corpus": sum(
            1 for p in corpus if (p.get("source_category") or "?") == key)}
        for key, value in by_category.items()
    }
    doc = {
        "schema_version": 1,
        "task": "Q1.1 corpus escape-rate evaluation",
        "scan": str(args.scan),
        "label": args.label,
        "n_corpus": n,
        "escaped": len(escaped),
        "still_degenerated": len(still),
        "missing": len(missing),
        "pending": len(pending),
        "escape_rate": round(rate, 4),
        "adjudication": {
            "q1_ladder_landing_line": 0.30,
            "q1_2_sampling_landing_line": 0.50,
            "passes_q1_ladder_line": rate >= 0.30,
        },
        "attempts_distribution": attempts,
        "escape_rate_by_source_category": category_rates,
        "escaped_pages": escaped,
        "still_degenerated_pages": still,
    }
    reports = root / "reports" / "analysis"
    reports.mkdir(parents=True, exist_ok=True)
    out = reports / f"analysis-q11-{args.label}.json"
    out.write_text(json.dumps(doc, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"escape rate: {len(escaped)}/{n} = {rate:.1%}  (still degenerated: {len(still)}, "
          f"missing: {len(missing)}, pending: {len(pending)})")
    print(f"attempts distribution: {attempts}")
    for key, value in sorted(category_rates.items()):
        print(f"  {key}: {value['escaped']}/{value['corpus']}")

    if args.emit_subset and escaped:
        bulk = root / "degeneration-v0.1"
        escaped_stems = {e["stem"] for e in escaped}
        mapping = json.loads((root / "pdfs" / "mapping.json").read_text(encoding="utf-8"))
        corpus_imgs = {
            p["image_path"] for p in mapping["pages"] if p["stem"] in escaped_stems
        }
        dataset = json.loads((root / "dataset" / "OmniDocBench.json").read_text(encoding="utf-8"))
        truth = [
            e for e in dataset
            if e.get("page_info", {}).get("image_path") in corpus_imgs
        ]
        subset_dir = bulk / f"escaped-{args.label}"
        (subset_dir / "predictions").mkdir(parents=True, exist_ok=True)
        (subset_dir / "truth-subset.json").write_text(
            json.dumps(truth, ensure_ascii=False, indent=1), encoding="utf-8"
        )
        # Prediction export: the experiment scan's own page markdown, think/
        # image-link stripped by the same exporter used for the baseline.
        stems_file = subset_dir / "stems.txt"
        stems_file.write_text("\n".join(sorted(escaped_stems)) + "\n", encoding="utf-8")
        export = subprocess.run(  # noqa: F821
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "export-omnidocbench-predictions.py"),
                "--scans", str(root / args.scan),
                "--mapping", str(asset.parent / "mapping-subset.json"),
                "--out", str(subset_dir / "predictions"),
                "--stems-filter", str(stems_file),
            ],
        )
        if export.returncode != 0:
            print("ERROR: escaped-subset prediction export failed", file=sys.stderr)
            return 2
        print(f"escaped subset: {subset_dir} (truth n={len(truth)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
