#!/usr/bin/env python3
"""R2.4 content-identity calibration (m4-table-reopen plan v1.6).

Measures the two distributions the frozen identity gate separates, using
REAL pipeline-shaped data:
  legitimate  = OCR html_table block (m23 round-2 scan) vs the GT html of
                the SAME table (ideal-reread proxy);
  foreign     = OCR block vs the GT html of a DIFFERENT random table.
Freezes T1/T2 at the operating point chosen in the plan (bias to blocking:
a blocked legitimate page keeps the original — zero risk; a passed foreign
pair still faces compare-keep and B2 gate-3).

  python scripts/omnidocbench-reread-identity-calibration.py \
      --scan-root benchmarks/omnidocbench/scans/m23-m2rung-balanced \
      --dataset benchmarks/omnidocbench/dataset/OmniDocBench.json \
      --output benchmarks/omnidocbench/reports/analysis/analysis-reread-identity-calibration.json \
      [--sample 300] [--seed 20261117]
"""
from __future__ import annotations

import argparse
import json
import random
import re
from pathlib import Path


def norm(text: str) -> str:
    """FROZEN v1 normalization: strip tags -> collapse whitespace -> lower."""
    text = re.sub(r"<[^>]+>", " ", text or "")
    return re.sub(r"\s+", "", text).lower()


def grams(text: str) -> set[str]:
    s = norm(text)
    return {s[i : i + 2] for i in range(len(s) - 1)} if len(s) > 1 else ({s} if s else set())


MIN_TEXT_BIGRAMS = 8


def directional(a_text: str, b_text: str):
    """(coverage_of_a_in_b, coverage_of_b_in_a) over normalized bigram sets;
    None when either side is below the frozen min-text floor."""
    ga, gb = grams(a_text), grams(b_text)
    if len(ga) < MIN_TEXT_BIGRAMS or len(gb) < MIN_TEXT_BIGRAMS:
        return None
    inter = len(ga & gb)
    return (inter / len(ga), inter / len(gb))


def gt_tables(item: dict) -> list[dict]:
    tables = [d for d in item["layout_dets"] if d["category_type"] == "table"]
    return sorted(tables, key=lambda d: (d.get("order") is None, d.get("order") or 0))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scan-root", required=True)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--sample", type=int, default=300)
    parser.add_argument("--seed", type=int, default=20261117)
    args = parser.parse_args()

    gt = json.loads(Path(args.dataset).read_text(encoding="utf-8"))
    scan_root = Path(args.scan_root)

    def ocr_block_html(stem: str, ordinal: int):
        for dd in scan_root.glob(stem + "*"):
            pj = dd / "page-0001.json"
            if not pj.exists():
                continue
            doc = json.loads(pj.read_text(encoding="utf-8"))
            tables = [b for b in doc.get("blocks", []) if b.get("type") == "html_table"]
            return tables[ordinal].get("html", "") if ordinal < len(tables) else None
        return None

    pairs = []
    for p in gt:
        img = p["page_info"]["image_path"]
        for k, t in enumerate(gt_tables(p)):
            pairs.append((img, k, t.get("html") or ""))
    rng = random.Random(args.seed)
    sample = rng.sample(pairs, min(args.sample, len(pairs)))
    foreign_pool = [h for _, _, h in pairs if norm(h)]

    legit, foreign = [], []
    inconclusive = 0
    for img, k, gt_html in sample:
        ocr = ocr_block_html(img.rsplit(".", 1)[0], k)
        if not ocr:
            inconclusive += 1
            continue
        d1 = directional(ocr, gt_html)
        if d1:
            legit.append(d1)
        else:
            inconclusive += 1
        d2 = directional(ocr, rng.choice(foreign_pool))
        if d2:
            foreign.append(d2)

    def stats(xs):
        xs = sorted(xs)
        return {
            "n": len(xs),
            "min": round(xs[0], 4),
            "p5": round(xs[max(0, len(xs) // 20)], 4),
            "p25": round(xs[len(xs) // 4], 4),
            "median": round(xs[len(xs) // 2], 4),
        }

    # Frozen operating point (plan v1.6 D-47): bias to blocking.
    T1, T2 = 0.40, 0.25
    legit_pass = sum(1 for a, b in legit if a >= T1 and b >= T2)
    foreign_pass = sum(1 for a, b in foreign if a >= T1 and b >= T2)
    result = {
        "schema_version": 1,
        "artifact": "omnidocbench-reread-identity-calibration",
        "normalization": "strip tags -> collapse whitespace -> lowercase -> char bigrams (frozen v1)",
        "min_text_bigrams": MIN_TEXT_BIGRAMS,
        "sample": len(sample),
        "measured_legitimate_n": len(legit),
        "measured_foreign_n": len(foreign),
        "inconclusive_or_missing": inconclusive,
        "legitimate_distribution": {
            "coverage_of_original_in_reread": stats([a for a, _ in legit]),
            "coverage_of_reread_in_original": stats([b for _, b in legit]),
        },
        "foreign_distribution": {
            "coverage_of_original_in_reread": stats([a for a, _ in foreign]),
            "coverage_of_reread_in_original": stats([b for _, b in foreign]),
        },
        "frozen_thresholds": {"T1_coverage_of_original": T1, "T2_coverage_of_reread": T2},
        "operating_point": {
            "legitimate_pass": f"{legit_pass}/{len(legit)} = {legit_pass / len(legit):.3f}",
            "foreign_pass": f"{foreign_pass}/{len(foreign)} = {foreign_pass / len(foreign):.3f}",
            "rationale": "blocked legitimate = keep original (fail-closed, zero risk); passed foreign still faces compare-keep + B2 gate-3",
        },
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({k: v for k, v in result.items() if k not in ()}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
