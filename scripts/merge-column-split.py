#!/usr/bin/env python3
"""Merges column-split scan outputs into per-source-page predictions and
scores them against the official evaluator (candidate C experiment).

For every split PDF that was scanned (stem pattern ``<stem>__colsplit``),
concatenates the band page markdowns in page order (= left-to-right column
order) into ``predictions/<stem>.md``, records per-page loop flags from the
band checkpoints (a page counts as loop-free when NO band degenerated),
and writes the truth subset + eval script inputs for the official scorer.

Usage:
  python scripts/merge-column-split.py --benchmark-root benchmarks/omnidocbench
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def decode(value: str) -> str:
    if value and len(value) % 2 == 0:
        try:
            return bytes.fromhex(value).decode("utf-8", "strict")
        except ValueError:
            return value
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark-root", type=Path, default=Path("benchmarks/omnidocbench"))
    args = parser.parse_args()
    root = args.benchmark_root

    manifest = json.loads(
        (root / "degeneration-v0.1/column-split/manifest.json").read_text(encoding="utf-8")
    )
    scan = root / "scans/column-split"
    scan_dirs = {d.name.rsplit("-", 1)[0]: d for d in scan.iterdir() if d.is_dir()}

    out = root / "degeneration-v0.1/column-split"
    predictions = out / "predictions"
    predictions.mkdir(parents=True, exist_ok=True)

    mapping = json.loads((root / "pdfs/mapping.json").read_text(encoding="utf-8"))
    stem2img = {p["stem"]: p["image_path"] for p in mapping["pages"]}
    dataset = json.loads((root / "dataset/OmniDocBench.json").read_text(encoding="utf-8"))

    rows = []
    missing_scan = []
    for entry in manifest["pages"]:
        stem = entry["stem"]
        doc = scan_dirs.get(f"{stem}__colsplit")
        if doc is None:
            missing_scan.append(stem)
            continue
        bands = entry["columns"]
        merged: list[str] = []
        loop_free = True
        any_md = False
        for page_no in range(1, bands + 1):
            md_path = doc / f"page-{page_no:04d}.md"
            if md_path.is_file():
                merged.append(md_path.read_text(encoding="utf-8"))
                any_md = True
            cp = doc / f"page-{page_no:04d}.checkpoint"
            if cp.is_file():
                fields = {}
                for line in cp.read_text(encoding="utf-8").splitlines():
                    k, _, v = line.partition("=")
                    fields[k] = decode(v)
                if fields.get("degenerated") == "true":
                    loop_free = False
        markdown = "\n\n".join(part for part in merged if part.strip())
        (predictions / f"{stem}.md").write_text(markdown, encoding="utf-8")
        rows.append({
            "stem": stem,
            "columns": bands,
            "loop_free": loop_free,
            "any_markdown": any_md,
            "markdown_chars": len(markdown),
        })

    truth_stems = [r["stem"] for r in rows]
    truth_imgs = {stem2img[s] for s in truth_stems}
    truth = [e for e in dataset if e.get("page_info", {}).get("image_path") in truth_imgs]
    (out / "truth-subset.json").write_text(json.dumps(truth, ensure_ascii=False, indent=1), encoding="utf-8")

    report = {
        "schema_version": 1,
        "n_sources": len(manifest["pages"]),
        "scanned": len(rows),
        "missing_scan": missing_scan,
        "loop_free_pages": sum(1 for r in rows if r["loop_free"]),
        "loop_free_ratio": round(sum(1 for r in rows if r["loop_free"]) / max(1, len(rows)), 3),
        "pages": rows,
    }
    (out / "merge-report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    print(json.dumps({k: report[k] for k in ("n_sources", "scanned", "loop_free_pages", "loop_free_ratio")},
                     ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
