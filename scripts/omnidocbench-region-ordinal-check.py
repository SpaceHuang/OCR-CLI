#!/usr/bin/env python3
"""R1b ordinal-agreement harness (m4-table-reopen plan, review RBLK-1).

Perfect-detector lower-bound check: take the B0a frozen GT table boxes as
if detection were perfect, apply a candidate reading-order rule, and
compare the generated per-class ordinals against the GT `order`-derived
ordinals on every multi-table page. Also reports the per-page ambiguity
verdict of the adaptive rule (the runtime fallback skips ambiguous pages).

Rules:
  rowmajor_yc_xc : sort by (y center, x center)              [v1.1 rule, falsified]
  rowgroup       : y-overlap band clustering, x within band
  adaptive       : row-grouped if vertically clean, else column-grouped
                   if horizontally clean, else AMBIGUOUS (runtime skips)

  python scripts/omnidocbench-region-ordinal-check.py \
      --region-truth benchmarks/omnidocbench/region-truth/region-truth.json \
      [--rule adaptive]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def boxes_with_gt_ordinal(page: dict) -> list[dict]:
    """GT table boxes carrying the GT class-ordinal.

    The B0a artifact stores regions ALREADY in GT reading order (order
    field, det-index tiebreak) with per-class ordinals assigned in that
    sequence — so the table-class subsequence IS the GT ordinal ground
    truth."""
    out = []
    ordinal = 0
    for r in page["regions"]:
        if r["class"] != "table":
            continue
        poly = r["poly"]
        out.append({
            "gt_ordinal": ordinal,
            "box": (min(poly[0::2]), min(poly[1::2]), max(poly[0::2]), max(poly[1::2])),
        })
        ordinal += 1
    return out


def overlaps(a: tuple, b: tuple, axis: int, frac: float = 0.3) -> bool:
    """True when the ranges on `axis` overlap by >= frac of the smaller."""
    a0, a1 = a[axis], a[axis + 2]
    b0, b1 = b[axis], b[axis + 2]
    inter = min(a1, b1) - max(a0, b0)
    smaller = min(a1 - a0, b1 - b0)
    if smaller <= 0:
        return False
    return inter / smaller >= frac


def cluster_bands(items: list[dict], axis: int, frac: float = 0.3) -> list[list[dict]]:
    """Greedy band clustering along `axis`: a region joins the last band
    while it overlaps that band's running extent by >= frac."""
    ordered = sorted(items, key=lambda it: it["box"][axis])
    bands: list[list[dict]] = []
    for it in ordered:
        if bands and overlaps(bands[-1][-1]["box"], it["box"], axis, frac):
            bands[-1].append(it)
            # running extent grows with the band
            bands[-1][-1] = it
        else:
            bands.append([it])
    return bands


def rule_rowmajor_yc_xc(items: list[dict]):
    ordered = sorted(items, key=lambda it: (
        (it["box"][1] + it["box"][3]) / 2,
        (it["box"][0] + it["box"][2]) / 2,
    ))
    return [it["gt_ordinal"] for it in ordered], False


def rule_rowgroup(items: list[dict]):
    bands = cluster_bands(list(items), axis=1)  # y axis
    bands.sort(key=lambda band: min(it["box"][1] for it in band))
    ordered = []
    for band in bands:
        ordered.extend(sorted(band, key=lambda it: it["box"][0]))
    return [it["gt_ordinal"] for it in ordered], False


def _clean_bands(bands: list[list[dict]], axis: int) -> bool:
    extents = []
    for band in bands:
        lo = min(it["box"][axis] for it in band)
        hi = max(it["box"][axis + 2] for it in band)
        extents.append((lo, hi))
    extents.sort()
    return all(extents[i][1] <= extents[i + 1][0] + 1e-6 for i in range(len(extents) - 1))


def _y_overlap_frac(a: tuple, b: tuple) -> float:
    inter = min(a[3], b[3]) - max(a[1], b[1])
    smaller = min(a[3] - a[1], b[3] - b[1])
    if smaller <= 0:
        return 0.0
    return max(0.0, inter) / smaller


PAIR_Y_OVERLAP_FRAC = 0.30


def _consecutive_y_overlaps(order_items: list[dict], frac: float) -> bool:
    """True when any two CONSECUTIVE tables in the generated order have
    overlapping y-ranges - their relative order came from x (or a y-center
    hairline), which the GT reading order may contradict."""
    return any(
        _y_overlap_frac(order_items[i]["box"], order_items[i + 1]["box"]) >= frac
        for i in range(len(order_items) - 1)
    )


def _bands_order(items: list[dict], axis: int, frac: float):
    bands = cluster_bands(list(items), axis=axis)
    bands.sort(key=lambda band: min(it["box"][axis] for it in band))
    clean = _clean_bands(bands, axis=axis)
    ordered = []
    for band in bands:
        ordered.extend(sorted(band, key=lambda it: it["box"][1 - axis]))
    return clean, ordered


def rule_adaptive(items: list[dict]):
    """Dual-hypothesis agreement (review RBLK-1): the page is reread-eligible
    ONLY when the row-major and column-major interpretations are BOTH clean
    AND produce the SAME order (with the consecutive-pair y-overlap veto).
    Anything else is AMBIGUOUS and skipped at runtime — editorial reading
    orders that disagree with geometry can never be resolved from boxes."""
    row_clean, row_order = _bands_order(items, axis=1, frac=0.3)
    col_clean, col_order = _bands_order(items, axis=0, frac=0.3)
    if not (row_clean and col_clean):
        return [it["gt_ordinal"] for it in row_order], True
    if row_order != col_order:
        return row_order, True
    row_ordinals = [it["gt_ordinal"] for it in row_order]
    col_ordinals = [it["gt_ordinal"] for it in col_order]
    if row_ordinals != col_ordinals:
        return row_ordinals, True
    if _consecutive_y_overlaps(row_order, PAIR_Y_OVERLAP_FRAC):
        return row_ordinals, True
    return row_ordinals, False


RULES = {
    "rowmajor_yc_xc": rule_rowmajor_yc_xc,
    "rowgroup": rule_rowgroup,
    "adaptive": rule_adaptive,
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--region-truth", required=True)
    parser.add_argument("--rule", default="adaptive", choices=sorted(RULES))
    args = parser.parse_args()
    truth = load_json(Path(args.region_truth))
    rule = RULES[args.rule]

    multi = 0
    agree = 0
    mismatch_pages = []
    ambiguous = 0
    for page in truth["pages"]:
        items = boxes_with_gt_ordinal(page)
        if len(items) < 2:
            continue  # single-table pages: no ordinal ambiguity by construction
        multi += 1
        generated, is_ambiguous = rule([dict(it) for it in items])
        if is_ambiguous:
            ambiguous += 1
        if generated == [it["gt_ordinal"] for it in items]:
            agree += 1
        else:
            mismatch_pages.append({
                "image_path": page["image_path"],
                "gt": [it["gt_ordinal"] for it in items],
                "generated": generated,
                "ambiguous": is_ambiguous,
            })

    rate = agree / multi if multi else 0.0
    print(json.dumps({
        "rule": args.rule,
        "multi_table_pages": multi,
        "ordinal_agree_pages": agree,
        "agreement_rate": round(rate, 4),
        "ambiguous_pages_skipped_at_runtime": ambiguous,
        "mismatch_pages": mismatch_pages[:12],
        "mismatch_n": len(mismatch_pages),
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
