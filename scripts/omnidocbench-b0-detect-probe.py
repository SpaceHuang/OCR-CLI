#!/usr/bin/env python3
"""B0' detection-gate probe (next-phase plan §4.2, D-20/O-1).

Runs the candidate layout detector (DocLayout-YOLO DocStructBench ONNX via
onnxruntime — the ORT-sidecar runtime route candidate) over the B0a frozen
region-truth pages and scores the FROZEN gate: table/formula region recall
>= 90% at IoU >= 0.7. Localization error (IoU distribution, center offset)
lands in the artifact for the runtime-route adjudication record.

Classes: detection `table` ↔ GT table dets; detection `isolated_formula`
↔ GT equation_isolated/equation_semantic dets (merged formula class).
Matching: per class, greedy IoU-descending GT↔det assignment (a GT counts
as recalled when its best-matching det has IoU >= threshold).

  python scripts/omnidocbench-b0-detect-probe.py \
      --model C:/t/m23/b0-models/doclayout_yolo_docstructbench_imgsz1024.onnx \
      --region-truth benchmarks/omnidocbench/region-truth/region-truth.json \
      --images benchmarks/omnidocbench/dataset/images \
      --output benchmarks/omnidocbench/reports/analysis/analysis-b0-detect-gate.json \
      [--limit 50]
"""
from __future__ import annotations

import argparse
import ast
import json
import sys
from collections import Counter
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort

IOU_THRESHOLD = 0.7
CONF_THRESHOLD = 0.25
RECALL_GATE = 0.90
# DocLayout-YOLO DocStructBench class names ↔ the frozen region classes.
DET_TABLE_CLASSES = {"table"}
DET_FORMULA_CLASSES = {"isolate_formula"}


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


class Detector:
    """The reference inference recipe (wybxc/DocLayout-YOLO ONNX repo)."""

    def __init__(self, model_path: str):
        model = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])
        self.session = model
        meta = dict(model.get_modelmeta().custom_metadata_map)
        self.names = ast.literal_eval(meta["names"])
        self.stride = ast.literal_eval(meta["stride"])
        self.input_name = model.get_inputs()[0].name

    def _preprocess(self, image_bgr: np.ndarray) -> tuple[np.ndarray, float, int, int]:
        orig_h, orig_w = image_bgr.shape[:2]
        # Reference recipe letterboxes to int(h/stride)*stride; extreme
        # pages (2e4 px newspaper strips) explode attention memory — clamp
        # the canvas (a taller page just downscales more).
        new_shape = min(2560, max(640, int(orig_h / self.stride) * self.stride))
        r = min(new_shape / orig_h, new_shape / orig_w)
        resized = cv2.resize(
            image_bgr, (int(round(orig_w * r)), int(round(orig_h * r))),
            interpolation=cv2.INTER_LINEAR,
        )
        pad_w = (new_shape - resized.shape[1]) % self.stride
        pad_h = (new_shape - resized.shape[0]) % self.stride
        top, left = pad_h // 2, pad_w // 2
        padded = cv2.copyMakeBorder(
            resized, top, pad_h - top, left, pad_w - left,
            cv2.BORDER_CONSTANT, value=(114, 114, 114),
        )
        pix = padded.transpose(2, 0, 1)[None].astype(np.float32) / 255.0
        return pix, r, left, top

    def detect(self, image_bgr: np.ndarray) -> list[dict]:
        orig_h, orig_w = image_bgr.shape[:2]
        pix, gain, pad_x, pad_y = self._preprocess(image_bgr)
        new_h, new_w = pix.shape[2], pix.shape[3]
        preds = self.session.run(None, {self.input_name: pix})[0]
        preds = preds[preds[..., 4] > CONF_THRESHOLD]
        out = []
        for row in preds:
            x0, y0, x1, y1 = row[:4]
            # undo letterbox padding + scale back to original pixels
            x0 = (x0 - pad_x) / gain
            x1 = (x1 - pad_x) / gain
            y0 = (y0 - pad_y) / gain
            y1 = (y1 - pad_y) / gain
            out.append({
                "class": self.names[int(row[-1])],
                "conf": float(row[4]),
                "bbox": (float(x0), float(y0), float(x1), float(y1)),
            })
        return out


def iou(a: tuple[float, float, float, float], b: tuple[float, float, float, float]) -> float:
    ix0, iy0 = max(a[0], b[0]), max(a[1], b[1])
    ix1, iy1 = min(a[2], b[2]), min(a[3], b[3])
    inter = max(0.0, ix1 - ix0) * max(0.0, iy1 - iy0)
    area_a = max(0.0, a[2] - a[0]) * max(0.0, a[3] - a[1])
    area_b = max(0.0, b[2] - b[0]) * max(0.0, b[3] - b[1])
    union = area_a + area_b - inter
    return inter / union if union > 0 else 0.0


def poly_bbox(poly: list[float]) -> tuple[float, float, float, float]:
    xs = poly[0::2]
    ys = poly[1::2]
    return (min(xs), min(ys), max(xs), max(ys))


def match_class(gt_boxes: list[tuple[float, float, float, float]],
                det_boxes: list[tuple[float, float, float, float]]) -> dict:
    """Greedy IoU-descending assignment; returns recall/precision/iou stats."""
    pairs = sorted(
        ((iou(g, d), gi, di) for gi, g in enumerate(gt_boxes) for di, d in enumerate(det_boxes)),
        key=lambda entry: -entry[0],
    )
    taken_g, taken_d = set(), set()
    matched_ious = []
    for score, gi, di in pairs:
        if score < IOU_THRESHOLD or gi in taken_g or di in taken_d:
            continue
        taken_g.add(gi)
        taken_d.add(di)
        matched_ious.append(score)
    return {
        "gt_n": len(gt_boxes),
        "det_n": len(det_boxes),
        "matched_n": len(matched_ious),
        "recall": (len(matched_ious) / len(gt_boxes)) if gt_boxes else None,
        "precision": (len(matched_ious) / len(det_boxes)) if det_boxes else None,
        "iou_mean": (sum(matched_ious) / len(matched_ious)) if matched_ious else None,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True)
    parser.add_argument("--region-truth", required=True)
    parser.add_argument("--images", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--limit", type=int, default=0, help="first N pages only (0 = all)")
    args = parser.parse_args()

    truth = load_json(Path(args.region_truth))
    pages = truth["pages"]
    if args.limit:
        pages = pages[: args.limit]
    detector = Detector(args.model)
    images_dir = Path(args.images)

    per_class_stats: dict[str, list[dict]] = {"table": [], "formula": []}
    missed: list[dict] = []
    page_times: list[float] = []
    errored_pages: list[str] = []
    skipped_unreadable: list[str] = []
    for index, page in enumerate(pages):
        image_path = images_dir / page["image_path"]
        # np.fromfile + imdecode: path-quirk-immune on native Windows.
        raw = np.fromfile(str(image_path), dtype=np.uint8)
        image = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
        if image is None:
            print(f"SKIP unreadable image {image_path}", file=sys.stderr)
            skipped_unreadable.append(page["image_path"])
            continue
        started = cv2.getTickCount()
        try:
            dets = detector.detect(image)
        except Exception as error:  # noqa: BLE001 - record and continue
            print(f"SKIP inference failure {page['image_path']}: {error}", file=sys.stderr)
            errored_pages.append(page["image_path"])
            continue
        page_times.append((cv2.getTickCount() - started) / cv2.getTickFrequency())
        groups = {
            "table": ([], []),
            "formula": ([], []),
        }
        for region in page["regions"]:
            gt_class = "table" if region["class"] == "table" else "formula"
            groups[gt_class][0].append(poly_bbox(region["poly"]))
        for det in dets:
            if det["class"] in DET_TABLE_CLASSES:
                groups["table"][1].append(det["bbox"])
            elif det["class"] in DET_FORMULA_CLASSES:
                groups["formula"][1].append(det["bbox"])
        for class_name, (gt_boxes, det_boxes) in groups.items():
            stats = match_class(gt_boxes, det_boxes)
            per_class_stats[class_name].append(stats)
            if stats["gt_n"] > stats["matched_n"]:
                missed.append({
                    "image_path": page["image_path"],
                    "class": class_name,
                    "missed_n": stats["gt_n"] - stats["matched_n"],
                    "gt_n": stats["gt_n"],
                    "matched_n": stats["matched_n"],
                })
        if (index + 1) % 25 == 0:
            print(f"  {index + 1}/{len(pages)} pages scored", flush=True)

    summary = {}
    for class_name, entries in per_class_stats.items():
        scored = [e for e in entries if e["recall"] is not None]
        gt_total = sum(e["gt_n"] for e in entries)
        matched_total = sum(e["matched_n"] for e in entries)
        det_total = sum(e["det_n"] for e in entries)
        ious = [e for e in entries if e["iou_mean"] is not None]
        summary[class_name] = {
            "gt_total": gt_total,
            "det_total": det_total,
            "matched_total": matched_total,
            "recall": matched_total / gt_total if gt_total else None,
            "precision": matched_total / det_total if det_total else None,
            "mean_iou_of_matched": (
                sum(e["iou_mean"] * e["matched_n"] for e in ious) / matched_total
                if matched_total else None
            ),
            "gate_pass": bool(gt_total and matched_total / gt_total >= RECALL_GATE),
        }

    artifact = {
        "schema_version": 1,
        "artifact": "omnidocbench-b0-detect-gate",
        "model": Path(args.model).name,
        "runtime": "onnxruntime-cpu",
        "gate": f"recall >= {RECALL_GATE:.0%} @ IoU >= {IOU_THRESHOLD} (conf >= {CONF_THRESHOLD})",
        "pages_scored": len(pages),
        "pages_errored": errored_pages,
        "pages_skipped_unreadable": skipped_unreadable,
        "page_time_ms_mean": (sum(page_times) / len(page_times) * 1000) if page_times else None,
        "summary": summary,
        "missed_top": missed[:40],
        "missed_n": len(missed),
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(artifact, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    overall_pass = all(entry["gate_pass"] for entry in summary.values())
    print(f"B0' detection gate: {'PASS' if overall_pass else 'FAIL'} -> {out}")


if __name__ == "__main__":
    main()
