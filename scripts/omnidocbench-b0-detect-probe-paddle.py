#!/usr/bin/env python3
"""B0' candidate-2 gate probe: PP-DocLayout-L via paddle.inference (paddle
runtime; the ORT-route conversion is a post-gate concern). Reuses the frozen
matching/scoring of omnidocbench-b0-detect-probe.py — only the detector
differs (RT-DETR: 640×640 stretch resize, direct box output, 23 classes,
`formula` = index 7, `table` = index 8).

  python scripts/omnidocbench-b0-detect-probe-paddle.py \
      --model-dir C:/t/m23/b0-ppdoclayout \
      --region-truth benchmarks/omnidocbench/region-truth/region-truth.json \
      --images 'D:\...\dataset\images' \
      --output benchmarks/omnidocbench/reports/analysis/analysis-b0-detect-gate-ppdoclayout.json \
      [--limit 5] [--pixel-scale 255]
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

FORMULA_INDEX = 7
TABLE_INDEX = 8
TARGET_SIZE = 640


class PaddleDetector:
    def __init__(self, model_dir: str, pixel_scale: float, target_size: int = 640,
                 keep_ratio: bool = False, formula_index: int = 7, table_index: int = 8):
        import paddle.inference as paddle_infer

        model_dir = Path(model_dir)
        config = paddle_infer.Config(
            str(model_dir / "inference.json"), str(model_dir / "inference.pdiparams")
        )
        config.disable_gpu()
        # paddle 3.4 PIR+oneDNN cannot convert this model's DoubleAttribute
        # arrays (Unimplemented at onednn_instruction.cc) — plain kernels.
        config.disable_mkldnn()
        self.predictor = paddle_infer.create_predictor(config)
        self.input_names = self.predictor.get_input_names()
        self.pixel_scale = pixel_scale
        self.target_size = target_size
        self.keep_ratio = keep_ratio
        self.formula_index = formula_index
        self.table_index = table_index

    def detect(self, image_bgr: np.ndarray) -> list[dict]:
        orig_h, orig_w = image_bgr.shape[:2]
        size = self.target_size
        if self.keep_ratio:
            # letterbox: scale by min ratio, pad to the square canvas
            r = min(size / orig_w, size / orig_h)
            rw, rh = int(round(orig_w * r)), int(round(orig_h * r))
            resized = cv2.resize(image_bgr, (rw, rh), interpolation=cv2.INTER_CUBIC)
            canvas = np.full((size, size, 3), 114, dtype=np.uint8)
            canvas[:rh, :rw] = resized
            self._r = r
        else:
            canvas = cv2.resize(image_bgr, (size, size), interpolation=cv2.INTER_CUBIC)
            self._r = 1.0
        pix = canvas.transpose(2, 0, 1)[None].astype(np.float32)
        if self.pixel_scale != 1.0:
            pix = pix / self.pixel_scale
        im_shape = np.array([[size, size]], dtype=np.float32)
        scale_factor = np.array([[size / orig_w, size / orig_h]], dtype=np.float32)
        handles = {
            "image": pix,
            "im_shape": im_shape,
            "scale_factor": scale_factor,
        }
        for name in self.input_names:
            self.predictor.get_input_handle(name).copy_from_cpu(handles[name])
        self.predictor.run()
        boxes = None
        for name in self.predictor.get_output_names():
            out = self.predictor.get_output_handle(name).copy_to_cpu()
            # L/plus export 6-column rows; V2 appends two more (ignored).
            if out.ndim == 2 and out.shape[-1] >= 6 and boxes is None:
                boxes = out
        results: list[dict] = []
        if boxes is None:
            return results
        # The exported graph's internal rescale swaps the w/h scale
        # components (verified empirically on portrait + landscape pages).
        # stretch:  x_true = x_out*(ow/oh);          y_true = y_out*(oh/ow)
        # letterbox: x_true = x_out*size/(r*oh);     y_true = y_out*size/(r*ow)
        if self.keep_ratio:
            fx = size / (self._r * orig_h)
            fy = size / (self._r * orig_w)
        else:
            fx, fy = orig_w / orig_h, orig_h / orig_w
        for row in boxes:
            class_id, score = int(row[0]), float(row[1])
            x0, y0, x1, y1 = row[2:6]
            if score < probe.CONF_THRESHOLD:
                continue
            results.append({
                "class": {self.formula_index: "isolate_formula", self.table_index: "table"}.get(class_id, "other"),
                "conf": score,
                "bbox": (
                    float(x0) * fx,
                    float(y0) * fy,
                    float(x1) * fx,
                    float(y1) * fy,
                ),
            })
        return results


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--region-truth", required=True)
    parser.add_argument("--images", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--pixel-scale", type=float, default=255.0)
    parser.add_argument("--target-size", type=int, default=640,
                        help="640 for PP-DocLayout-L, 800 for plus (exported graph shape)")
    parser.add_argument("--keep-ratio", action="store_true",
                        help="letterbox instead of stretch resize")
    parser.add_argument("--formula-index", type=int, default=7)
    parser.add_argument("--table-index", type=int, default=8)
    args = parser.parse_args()

    truth = probe.load_json(Path(args.region_truth))
    pages = truth["pages"]
    if args.limit:
        pages = pages[: args.limit]
    detector = PaddleDetector(args.model_dir, args.pixel_scale, args.target_size,
                              args.keep_ratio, args.formula_index, args.table_index)
    images_dir = Path(args.images)

    per_class_stats: dict[str, list[dict]] = {"table": [], "formula": []}
    missed: list[dict] = []
    page_times: list[float] = []
    errored_pages: list[str] = []
    skipped_unreadable: list[str] = []
    for index, page in enumerate(pages):
        image_path = images_dir / page["image_path"]
        raw = np.fromfile(str(image_path), dtype=np.uint8)
        image = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
        if image is None:
            print(f"SKIP unreadable image {image_path}", file=sys.stderr)
            skipped_unreadable.append(page["image_path"])
            continue
        started = cv2.getTickCount()
        try:
            dets = detector.detect(image)
        except Exception as error:  # noqa: BLE001
            print(f"SKIP inference failure {page['image_path']}: {error}", file=sys.stderr)
            errored_pages.append(page["image_path"])
            continue
        page_times.append((cv2.getTickCount() - started) / cv2.getTickFrequency())
        groups = {"table": ([], []), "formula": ([], [])}
        for region in page["regions"]:
            gt_class = "table" if region["class"] == "table" else "formula"
            groups[gt_class][0].append(probe.poly_bbox(region["poly"]))
        for det in dets:
            if det["class"] == "table":
                groups["table"][1].append(det["bbox"])
            elif det["class"] == "isolate_formula":
                groups["formula"][1].append(det["bbox"])
        for class_name, (gt_boxes, det_boxes) in groups.items():
            stats = probe.match_class(gt_boxes, det_boxes)
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
            "gate_pass": bool(gt_total and matched_total / gt_total >= probe.RECALL_GATE),
        }
    artifact = {
        "schema_version": 1,
        "artifact": "omnidocbench-b0-detect-gate",
        "model": f"PP-DocLayout-L (paddle.inference, pixel-scale={args.pixel_scale}, size={args.target_size})",
        "runtime": "paddle-cpu",
        "gate": f"recall >= {probe.RECALL_GATE:.0%} @ IoU >= {probe.IOU_THRESHOLD} (conf >= {probe.CONF_THRESHOLD})",
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
    print(f"B0' detection gate (PP-DocLayout-L): {'PASS' if overall_pass else 'FAIL'} -> {out}")


if __name__ == "__main__":
    main()
