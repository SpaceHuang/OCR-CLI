#!/usr/bin/env python3
"""B0' dedicated-MFD probe: Pix2Text MFD 1.5 (YOLO11m, AGPL — gate evidence
only, product licensing needs review) over the B0a frozen subset.

Raw Ultralytics export: output0 [1, 4+nc, anchors] in letterbox pixels
(xywh), scores per class — decode + NMS here (the export ships no NMS).

  python scripts/omnidocbench-b0-detect-probe-mfd.py \
      --model C:/t/m23/b0-mfd/pix2text-mfd-1.5.onnx \
      --region-truth benchmarks/omnidocbench/region-truth/region-truth.json \
      --images 'D:\\...\\dataset\\images' \
      --output benchmarks/omnidocbench/reports/analysis/analysis-b0-detect-gate-mfd.json \
      [--limit N] [--conf 0.25]
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path

import cv2
import numpy as np

_HERE = Path(__file__).parent
_spec = importlib.util.spec_from_file_location(
    "b0probe", _HERE / "omnidocbench-b0-detect-probe.py"
)
probe = importlib.util.module_from_spec(_spec)
sys.modules["b0probe"] = probe
_spec.loader.exec_module(probe)

ISOLATED_INDEX = 1   # 'isolated' = display formula
EMBEDDING_INDEX = 0  # 'embedding' = inline formula (also counts: GT merges semantic)
TARGET = 768


def nms(boxes: np.ndarray, scores: np.ndarray, iou_del: float = 0.45) -> list[int]:
    x0, y0, x1, y1 = boxes[:, 0], boxes[:, 1], boxes[:, 2], boxes[:, 3]
    areas = (x1 - x0).clip(0) * (y1 - y0).clip(0)
    order = scores.argsort()[::-1]
    keep: list[int] = []
    while order.size:
        i = order[0]
        keep.append(int(i))
        if order.size == 1:
            break
        rest = order[1:]
        ix0 = np.maximum(x0[i], x0[rest])
        iy0 = np.maximum(y0[i], y0[rest])
        ix1 = np.minimum(x1[i], x1[rest])
        iy1 = np.minimum(y1[i], y1[rest])
        inter = (ix1 - ix0).clip(0) * (iy1 - iy0).clip(0)
        union = areas[i] + areas[rest] - inter
        order = rest[(inter / union.clip(1e-9)) <= iou_del]
    return keep


class MfdDetector:
    def __init__(self, model_path: str, conf: float):
        import onnxruntime as ort

        self.session = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])
        self.input_name = self.session.get_inputs()[0].name
        self.conf = conf

    def detect(self, image_bgr: np.ndarray) -> list[dict]:
        orig_h, orig_w = image_bgr.shape[:2]
        r = min(TARGET / orig_w, TARGET / orig_h)
        rw, rh = int(round(orig_w * r)), int(round(orig_h * r))
        resized = cv2.resize(image_bgr, (rw, rh), interpolation=cv2.INTER_LINEAR)
        canvas = np.full((TARGET, TARGET, 3), 114, dtype=np.uint8)
        canvas[:rh, :rw] = resized
        pix = canvas.transpose(2, 0, 1)[None].astype(np.float32) / 255.0
        out = self.session.run(None, {self.input_name: pix})[0]  # [1, 6, A]
        preds = out[0].T  # [A, 6]
        boxes_lt = preds[:, :4]
        scores = preds[:, 4:]
        cls = scores.argmax(axis=1)
        conf = scores.max(axis=1)
        mask = conf >= self.conf
        boxes_lt, cls, conf = boxes_lt[mask], cls[mask], conf[mask]
        if boxes_lt.size == 0:
            return []
        # xywh (letterbox px) -> xyxy
        x0 = boxes_lt[:, 0] - boxes_lt[:, 2] / 2
        y0 = boxes_lt[:, 1] - boxes_lt[:, 3] / 2
        x1 = boxes_lt[:, 0] + boxes_lt[:, 2] / 2
        y1 = boxes_lt[:, 1] + boxes_lt[:, 3] / 2
        keep = nms(np.stack([x0, y0, x1, y1], axis=1), conf)
        results = []
        for i in keep:
            results.append({
                "class": "formula" if cls[i] in (ISOLATED_INDEX, EMBEDDING_INDEX) else "other",
                "conf": float(conf[i]),
                "bbox": (
                    float(x0[i] / r), float(y0[i] / r),
                    float(x1[i] / r), float(y1[i] / r),
                ),
            })
        return results


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True)
    parser.add_argument("--region-truth", required=True)
    parser.add_argument("--images", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--conf", type=float, default=0.25)
    args = parser.parse_args()

    truth = probe.load_json(Path(args.region_truth))
    pages = truth["pages"]
    if args.limit:
        pages = pages[: args.limit]
    detector = MfdDetector(args.model, args.conf)
    images_dir = Path(args.images)

    stats: list[dict] = []
    missed: list[dict] = []
    times: list[float] = []
    errored: list[str] = []
    for index, page in enumerate(pages):
        image_path = images_dir / page["image_path"]
        raw = np.fromfile(str(image_path), dtype=np.uint8)
        image = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
        if image is None:
            errored.append(page["image_path"])
            continue
        started = cv2.getTickCount()
        try:
            dets = detector.detect(image)
        except Exception as error:  # noqa: BLE001
            print(f"SKIP {page['image_path']}: {error}", file=sys.stderr)
            errored.append(page["image_path"])
            continue
        times.append((cv2.getTickCount() - started) / cv2.getTickFrequency())
        gt_boxes = [
            probe.poly_bbox(r["poly"])
            for r in page["regions"]
            if r["class"].startswith("equation")
        ]
        det_boxes = [d["bbox"] for d in dets if d["class"] == "formula"]
        entry = probe.match_class(gt_boxes, det_boxes)
        stats.append(entry)
        if entry["gt_n"] > entry["matched_n"]:
            missed.append({
                "image_path": page["image_path"],
                "missed_n": entry["gt_n"] - entry["matched_n"],
                "gt_n": entry["gt_n"],
            })
        if (index + 1) % 50 == 0:
            print(f"  {index + 1}/{len(pages)}", flush=True)

    gt_total = sum(e["gt_n"] for e in stats)
    det_total = sum(e["det_n"] for e in stats)
    matched = sum(e["matched_n"] for e in stats)
    ious = [e for e in stats if e["iou_mean"] is not None]
    summary = {
        "formula": {
            "gt_total": gt_total,
            "det_total": det_total,
            "matched_total": matched,
            "recall": matched / gt_total if gt_total else None,
            "precision": matched / det_total if det_total else None,
            "mean_iou_of_matched": (
                sum(e["iou_mean"] * e["matched_n"] for e in ious) / matched
                if matched else None
            ),
            "gate_pass": bool(gt_total and matched / gt_total >= probe.RECALL_GATE),
        },
    }
    artifact = {
        "schema_version": 1,
        "artifact": "omnidocbench-b0-detect-gate-mfd",
        "model": Path(args.model).name,
        "runtime": "onnxruntime-cpu",
        "note": "dedicated MFD (Pix2Text 1.5, AGPL) — formula classes isolated+embedding merged",
        "gate": f"recall >= {probe.RECALL_GATE:.0%} @ IoU >= {probe.IOU_THRESHOLD} (conf >= {args.conf})",
        "pages_scored": len(pages),
        "pages_errored": errored,
        "page_time_ms_mean": (sum(times) / len(times) * 1000) if times else None,
        "summary": summary,
        "missed_n": len(missed),
        "missed_top": missed[:40],
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(artifact, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print(f"B0' MFD gate: {'PASS' if summary['formula']['gate_pass'] else 'FAIL'} -> {out}")


if __name__ == "__main__":
    main()
