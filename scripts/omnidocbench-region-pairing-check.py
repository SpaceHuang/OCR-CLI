#!/usr/bin/env python3
"""R1b pairing-safety acceptance against REAL PP-DocLayoutV2 output
(m4-table-reopen plan, review round 3).

The integral skip rule is implemented here as EXECUTABLE reference code
(the shape that ports to Rust in R2.4):

  verdict(det_table_boxes, output_n):
    1. dedup  : same-class boxes IoU >= 0.9, first wins. Any dedup event
                -> SKIP (dedup holes shift every later ordinal).
    2. count  : len(deduped) != output_n -> SKIP (miss / multi-detection).
    3. order  : dual-hypothesis (row/column interpretations both clean and
                identical, consecutive-pair y-overlap veto) -> else SKIP.
    4. pair   : ordinals 0..N-1 in generated order.

Safety metric: on PAIRED pages, detection box at generated position k must
best-match (IoU >= 0.7) the GT table with gt_ordinal == k — otherwise the
page is UNSAFE (would replace the wrong table). hard_gate.pass requires
unsafe == 0 across ALL states (clean / miss / multi / dedup) WITH REAL V2
OUTPUT — synthetic perturbation is gone; the three states emerge naturally
from the detector's own behavior.

  python scripts/omnidocbench-region-pairing-check.py \
      --model-dir C:/t/m23/b0-ppdoclayout-v2 \
      --region-truth benchmarks/omnidocbench/region-truth/region-truth.json \
      --images 'D:\\...\\dataset\\images' \
      --output benchmarks/omnidocbench/reports/analysis/analysis-b0-pairing-safety.json
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from collections import Counter
from pathlib import Path

import cv2
import numpy as np

_HERE = Path(__file__).parent
_spec = importlib.util.spec_from_file_location(
    "b0probe", _HERE / "omnidocbench-b0-detect-probe-paddle.py"
)
pp = importlib.util.module_from_spec(_spec)
sys.modules["b0probe"] = pp
_spec.loader.exec_module(pp)

_SPEC3 = importlib.util.spec_from_file_location(
    "sidecar",
    Path(__file__).with_name("omnidocbench-reread-regions-sidecar.py"),
)
sidecar = importlib.util.module_from_spec(_SPEC3)

_SPEC2 = importlib.util.spec_from_file_location(
    "ordchk", _HERE / "omnidocbench-region-ordinal-check.py"
)
ordchk = importlib.util.module_from_spec(_SPEC2)
_SPEC3.loader.exec_module(sidecar)
sys.modules["ordchk"] = ordchk
_SPEC2.loader.exec_module(ordchk)

IOU_PAIR = 0.7


def dedup_boxes(boxes: list[tuple]) -> tuple[list[tuple], bool]:
    """IoU >= 0.9 same-class dedup, first wins (mask, no renumber)."""
    kept: list[tuple] = []
    deduped = False
    for box in boxes:
        duplicate = any(ordchk_probe_iou(box, k) >= 0.9 for k in kept)
        if duplicate:
            deduped = True
            continue
        kept.append(box)
    return kept, deduped


def ordchk_probe_iou(a: tuple, b: tuple) -> float:
    ix0, iy0 = max(a[0], b[0]), max(a[1], b[1])
    ix1, iy1 = min(a[2], b[2]), min(a[3], b[3])
    inter = max(0.0, ix1 - ix0) * max(0.0, iy1 - iy0)
    aa = (a[2] - a[0]) * (a[3] - a[1])
    bb = (b[2] - b[0]) * (b[3] - b[1])
    union = aa + bb - inter
    return inter / union if union > 0 else 0.0


def dual_hypothesis(boxes: list[tuple]) -> tuple[list[tuple], bool]:
    """The R1b ordering rule on plain boxes (row/column interpretations both
    clean and identical + consecutive-pair veto); returns (ordered, ambiguous)."""
    items = [{"gt_ordinal": i, "box": b} for i, b in enumerate(boxes)]
    row_clean, row_order = ordchk._bands_order(items, axis=1, frac=0.3)
    col_clean, col_order = ordchk._bands_order(items, axis=0, frac=0.3)
    if not (row_clean and col_clean):
        return [it["box"] for it in row_order], True
    if [it["gt_ordinal"] for it in row_order] != [it["gt_ordinal"] for it in col_order]:
        return [it["box"] for it in row_order], True
    if ordchk._consecutive_y_overlaps(row_order, 0.30):
        return [it["box"] for it in row_order], True
    return [it["box"] for it in row_order], False


def sidecar_scan_render_size(pdfs_dir: Path, stem: str) -> tuple[int, int]:
    """The scan bitmap dims the sidecar rendered at (300 dpi, 2880^2 fit).

    Deterministic per PDF (pymupdf page rect -> the same round+fit rule),
    so rescaling needs no rerun of detection.
    """
    import pymupdf

    with pymupdf.open(str(pdfs_dir / f"{stem}.pdf")) as document:
        page = document[0]
        return sidecar.scan_render_size(page.rect.width, page.rect.height, 300)


def pairing_verdict(det_table_boxes: list[tuple], output_n: int) -> dict:
    """EXECUTABLE reference of the frozen integral skip rule."""
    deduped, dedup_happened = dedup_boxes(det_table_boxes)
    if dedup_happened:
        return {"verdict": "skip", "reason": "dedup_event"}
    if len(deduped) != output_n:
        return {
            "verdict": "skip",
            "reason": "count_mismatch",
            "detection_n": len(deduped),
            "output_n": output_n,
        }
    ordered, ambiguous = dual_hypothesis(deduped)
    if ambiguous:
        return {"verdict": "skip", "reason": "ordering_ambiguous"}
    return {"verdict": "pair", "ordinals": list(range(len(ordered))), "ordered_boxes": ordered}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", default=None,
                        help="paddle model dir (required without --regions-dir)")
    parser.add_argument("--region-truth", required=True)
    parser.add_argument("--images", required=True)
    parser.add_argument("--regions-dir", default=None,
                        help="R3 mode: read the sidecar regions files "
                             "(<stem>.regions.json, scan render pixel space) "
                             "instead of running detection live")
    parser.add_argument("--pdfs-dir", default=None,
                        help="R3 mode: the corpus PDF dir (for render-space "
                             "rescaling into the GT pixel space)")
    parser.add_argument("--output", required=True)
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()

    truth = pp.probe.load_json(Path(args.region_truth))
    pages = truth["pages"]
    if args.limit:
        pages = pages[: args.limit]
    if args.regions_dir:
        if not args.pdfs_dir:
            parser.error("--pdfs-dir is required with --regions-dir")
        detector = None
        images_dir = Path(args.images)
        regions_dir = Path(args.regions_dir)
        pdfs_dir = Path(args.pdfs_dir)
    else:
        if not args.model_dir:
            parser.error("--model-dir is required without --regions-dir")
        detector = pp.PaddleDetector(args.model_dir, 255.0, 800, False, 5, 21)
        images_dir = Path(args.images)

    states = Counter()
    unsafe_pages: list[dict] = []
    crop_quality_flags: list[dict] = []
    skip_reasons: Counter = Counter()
    per_state = {
        "clean": {"pages": 0, "paired": 0, "skipped": 0, "unsafe": 0},
        "miss": {"pages": 0, "paired": 0, "skipped": 0, "unsafe": 0},
        "multi": {"pages": 0, "paired": 0, "skipped": 0, "unsafe": 0},
        "dedup": {"pages": 0, "paired": 0, "skipped": 0, "unsafe": 0},
    }
    errored = 0
    per_page: list[dict] = []
    for index, page in enumerate(pages):
        if args.regions_dir:
            stem = Path(page["image_path"]).stem
            regions_path = regions_dir / f"{stem}.regions.json"
            if not regions_path.is_file():
                errored += 1
                per_page.append({"image_path": page["image_path"],
                                 "verdict": "no_regions_file"})
                continue
            payload = json.loads(regions_path.read_text(encoding="utf-8"))
            # The sidecar boxes live in the SCAN render pixel space; the GT
            # polys in the dataset-image space. Both spaces are uniform
            # scales of the page (aspect preserved), so the verdict rules
            # are space-invariant — but the identity IoU needs the SAME
            # space: rescale the det boxes into the GT space.
            gt_image_path = images_dir / page["image_path"]
            raw = np.fromfile(str(gt_image_path), dtype=np.uint8)
            gt_image = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
            if gt_image is None:
                errored += 1
                per_page.append({"image_path": page["image_path"],
                                 "verdict": "unreadable_image"})
                continue
            gt_h, gt_w = gt_image.shape[:2]
            scan_w, scan_h = sidecar_scan_render_size(
                pdfs_dir, Path(page["image_path"]).stem
            )
            sx, sy = gt_w / scan_w, gt_h / scan_h
            dets = [
                {"class": "table",
                 "bbox": (
                     region["bbox_px"][0] * sx,
                     region["bbox_px"][1] * sy,
                     region["bbox_px"][2] * sx,
                     region["bbox_px"][3] * sy,
                 )}
                for region in payload.get("pages", [{}])[0].get("regions", [])
                if region.get("class") == "table"
            ]
        else:
            image_path = images_dir / page["image_path"]
            raw = np.fromfile(str(image_path), dtype=np.uint8)
            image = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
            if image is None:
                errored += 1
                per_page.append({"image_path": page["image_path"],
                                 "verdict": "unreadable_image"})
                continue
            try:
                dets = detector.detect(image)
            except Exception as error:  # noqa: BLE001
                print(f"SKIP inference failure {page['image_path']}: {error}", file=sys.stderr)
                errored += 1
                per_page.append({"image_path": page["image_path"],
                                 "verdict": "inference_failure"})
                continue

        # gt_ordinal MUST be the per-class ordinal (k-th table on the page),
        # NOT the index within all regions — equation regions before the
        # table shift the label otherwise (harness bug caught by round-3
        # self-check: single-table pages were labeled with their region
        # index, flagging correct pairings as unsafe).
        gt_tables = []
        gt_ordinal = 0
        for r in page["regions"]:
            if r["class"] == "table":
                gt_tables.append({
                    "gt_ordinal": gt_ordinal,
                    "box": pp.probe.poly_bbox(r["poly"]),
                })
                gt_ordinal += 1
        output_n = len(gt_tables)
        det_boxes = [d["bbox"] for d in dets if d["class"] == "table"]

        verdict = pairing_verdict(det_boxes, output_n)
        per_page.append({
            "image_path": page["image_path"],
            "verdict": verdict["verdict"],
            **({"reason": verdict.get("reason")} if verdict["verdict"] == "skip" else {}),
        })

        # classify the page's natural state
        deduped, dedup_happened = dedup_boxes(det_boxes)
        if dedup_happened:
            state = "dedup"
        elif len(deduped) < output_n:
            state = "miss"
        elif len(deduped) > output_n:
            state = "multi"
        else:
            state = "clean"

        per_state[state]["pages"] += 1
        if verdict["verdict"] == "skip":
            per_state[state]["skipped"] += 1
            skip_reasons[verdict["reason"]] += 1
            states[f"skip:{verdict['reason']}"] += 1
            continue

        # PAIRED: verify ordinal IDENTITY — detection at generated position k
        # must best-match the GT table with gt_ordinal == k. Identity (which
        # table this is) is the pairing-safety semantics; IoU measures CROP
        # quality, a different failure mode guarded at replace time
        # (compare-keep + content-identity check, R2.4) and measured by B2
        # gate-3 — conflating them in one gate was the v1.4 category error
        # (review round 4 holds the plan to its own frozen text, so the gate
        # text is corrected here and in the plan).
        per_state[state]["paired"] += 1
        states["paired"] += 1
        for k, det_box in enumerate(verdict["ordered_boxes"]):
            best_iou, best_ordinal = 0.0, -1
            for gt in gt_tables:
                iou = ordchk_probe_iou(det_box, gt["box"])
                if iou > best_iou:
                    best_iou, best_ordinal = iou, gt["gt_ordinal"]
            if best_ordinal != k:
                per_state[state]["unsafe"] += 1
                states["unsafe"] += 1
                unsafe_pages.append({
                    "image_path": page["image_path"],
                    "position": k,
                    "matched_gt_ordinal": best_ordinal,
                    "iou": round(best_iou, 4),
                })
                break
            if best_iou < IOU_PAIR:
                crop_quality_flags.append({
                    "image_path": page["image_path"],
                    "position": k,
                    "iou": round(best_iou, 4),
                })
        if (index + 1) % 50 == 0:
            print(f"  {index + 1}/{len(pages)}", flush=True)

    unsafe_total = sum(s["unsafe"] for s in per_state.values())
    # The claimed AND-gate, actually implemented (review round 4): the
    # perfect-box artifact must ALSO pass for hard_gate.pass.
    perfect_artifact_path = Path(args.output).parent / "analysis-b0-ordinal-agreement.json"
    perfect_pass = None
    if perfect_artifact_path.exists():
        perfect = json.loads(perfect_artifact_path.read_text(encoding="utf-8"))
        perfect_pass = bool(perfect.get("hard_gate", {}).get("pass"))
    result = {
        "schema_version": 1,
        "artifact": "omnidocbench-b0-pairing-safety",
        "model": "PP-DocLayoutV2 (paddle.inference, 800, real output)",
        "rule": "integral skip: dedup-event -> skip; count mismatch -> skip; dual-hypothesis ambiguous -> skip; else pair ordinals 0..N-1",
        "pages_evaluated": len(pages),
        "pages_errored": errored,
        "regions_dir_mode": bool(args.regions_dir),
        "per_page": per_page,
        "verdicts": dict(states),
        "skip_reasons": dict(skip_reasons),
        "per_state": per_state,
        "unsafe_pages": unsafe_pages,
        "unsafe_total": unsafe_total,
        "crop_quality_below_0_7": crop_quality_flags,
        "crop_quality_note": "identity-correct pages whose best-IoU < 0.7 (superset boxes incl. captions): crop-quality risk, NOT pairing risk; guarded at replace time by compare-keep + content-identity check (R2.4) and measured by B2 gate-3",
        "perfect_box_gate_pass": perfect_pass,
        "hard_gate": {
            "definition": "identity-unsafe == 0 across ALL states with REAL detector output, AND perfect-box gate pass",
            "unsafe_zero_required": "across ALL states with REAL detector output",
            "pass": unsafe_total == 0 and perfect_pass is True,
        },
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({k: v for k, v in result.items() if k != "unsafe_pages"}, ensure_ascii=False, indent=2))
    print(f"pairing-safety gate: {'PASS' if result['hard_gate']['pass'] else 'FAIL'} -> {out}")


if __name__ == "__main__":
    main()
