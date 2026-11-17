#!/usr/bin/env python3
"""R3 sidecar (m4-table-reopen plan R3, route A per the R1 adjudication):
batch PP-DocLayoutV2 pre-detection over the B0b table-corpus PDFs, writing
ONE regions JSON per PDF in the scan provider's frozen schema.

Output schema (read by src/ocrx-scan/src/region_provider.rs):
  {"schema_version": 1,
   "model_id": "pp-doclayout-v2",
   "revision": "<frozen>",
   "pages": [{"page": 1,
              "regions": [{"class": "table", "index_in_class": <k>,
                           "bbox_px": [x0, y0, x1, y1], "conf": <c>}]}]}

Frozen preprocessing (D-45, the values the B0'/pairing runs validated):
  800x800 stretch resize, pixel scale 255, conf >= 0.25, V2 class ids
  {table: 21, isolate_formula: 5}, and the empirically-verified w/h scale
  swap fix. bbox_px are in the SCAN RENDER PIXEL SPACE: round(page_pt *
  dpi/72) then the 2880^2 budget fit — the same space ocrx crops in
  (clamped_bitmap_size + fit_pixel_budget in src/ocrx-pdf/src/lib.rs).

  python scripts/omnidocbench-reread-regions-sidecar.py \
      --corpus benchmarks/omnidocbench/b0b-corpus/b0b-corpus.json \
      --pdfs benchmarks/omnidocbench/pdfs \
      --output-dir benchmarks/omnidocbench/reread-regions \
      --model-dir C:/t/m23/b0-ppdoclayout-v2 \
      [--dpi 300] [--limit 0] [--pages-list stems.txt]
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

import cv2
import numpy as np

SCHEMA_VERSION = 1
MODEL_ID = "pp-doclayout-v2"
MODEL_REVISION = "b73668227b14316a38f8b345d6b474e4f1f0b84d"
TARGET_SIZE = 800
PIXEL_SCALE = 255.0
CONF_THRESHOLD = 0.25
TABLE_CLASS_ID = 21
FORMULA_CLASS_ID = 5
DEFAULT_DPI = 300
# The scan's frozen normal-path render budget (MAX_RENDER_PIXELS).
RENDER_PIXEL_BUDGET = 2880 * 2880


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def scan_render_size(page_width_pt: float, page_height_pt: float, dpi: int) -> tuple[int, int]:
    """The scan bitmap size: round(pt * dpi / 72) then the 2880^2 fit.

    Mirrors clamped_bitmap_size + fit_pixel_budget in
    src/ocrx-pdf/src/lib.rs (round, floor on the fit, min 1).
    """
    raw_w = max(1.0, round(page_width_pt * dpi / 72.0))
    raw_h = max(1.0, round(page_height_pt * dpi / 72.0))
    pixels = raw_w * raw_h
    if pixels <= RENDER_PIXEL_BUDGET or RENDER_PIXEL_BUDGET == 0:
        return int(raw_w), int(raw_h)
    scale = (RENDER_PIXEL_BUDGET / pixels) ** 0.5
    return max(1, int(raw_w * scale)), max(1, int(raw_h * scale))


def page_stems(corpus: dict) -> list[str]:
    """Distinct corpus page stems, banded-cohort order (frozen: worst,
    mid, good-en, good-zh, good-other, bad-low — the artifact's band order)."""
    stems: list[str] = []
    for members in corpus["tables"].values():
        for member in members:
            stem = Path(member["image_path"]).stem
            if stem not in stems:
                stems.append(stem)
    return stems


class V2TableDetector:
    """The B0'/pairing-validated PP-DocLayoutV2 table detector (route A).

    Rendering to the scan bitmap happens through PyMuPDF (geometry-identical:
    the pixel space only depends on the computed target size); detection
    rides paddle.inference with the frozen preprocessing.
    """

    def __init__(self, model_dir: str):
        import paddle.inference as paddle_infer

        config = paddle_infer.Config(
            str(Path(model_dir) / "inference.json"),
            str(Path(model_dir) / "inference.pdiparams"),
        )
        config.disable_gpu()
        # paddle 3.4 PIR+oneDNN cannot convert this model's DoubleAttribute
        # arrays (Unimplemented at onednn_instruction.cc) — plain kernels
        # (the B0'-documented fix, b0-detect-probe-paddle.py).
        config.disable_mkldnn()
        self.predictor = paddle_infer.create_predictor(config)
        self.input_names = self.predictor.get_input_names()

    def detect_tables(self, image_bgr: np.ndarray) -> list[dict]:
        orig_h, orig_w = image_bgr.shape[:2]
        size = TARGET_SIZE
        # Preprocessing mirrors b0-detect-probe-paddle.py byte-for-byte
        # (stretch INTER_CUBIC, NCHW, pixel scale, im_shape/scale_factor
        # semantics — the swap fix in the rescale depends on these).
        canvas = cv2.resize(image_bgr, (size, size), interpolation=cv2.INTER_CUBIC)
        pix = canvas.transpose(2, 0, 1)[None].astype(np.float32)
        if PIXEL_SCALE != 1.0:
            pix = pix / PIXEL_SCALE
        im_shape = np.array([[size, size]], dtype=np.float32)
        scale_factor = np.array(
            [[size / orig_w, size / orig_h]], dtype=np.float32
        )
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
            if out.ndim == 2 and out.shape[-1] >= 6 and boxes is None:
                boxes = out
        results: list[dict] = []
        if boxes is None:
            return results
        # The exported graph's internal rescale swaps the w/h scale
        # components (verified empirically in the B0' probes): stretch,
        # x_true = x_out*(ow/oh); y_true = y_out*(oh/ow).
        fx, fy = orig_w / orig_h, orig_h / orig_w
        for row in boxes:
            class_id, score = int(row[0]), float(row[1])
            if class_id != TABLE_CLASS_ID or score < CONF_THRESHOLD:
                continue
            x0, y0, x1, y1 = row[2:6]
            results.append(
                {
                    "class": "table",
                    "index_in_class": 0,  # assigned below in detection order
                    "bbox_px": [
                        float(x0) * fx,
                        float(y0) * fy,
                        float(x1) * fx,
                        float(y1) * fy,
                    ],
                    "conf": score,
                }
            )
        for index, region in enumerate(results):
            region["index_in_class"] = index
        return results


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--pdfs", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--model-dir", required=True)
    parser.add_argument("--dpi", type=int, default=DEFAULT_DPI)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--pages-list", type=Path, default=None,
                        help="optional stems file restricting the batch")
    args = parser.parse_args(argv)

    import pymupdf

    corpus = load_json(args.corpus)
    stems = page_stems(corpus)
    if args.pages_list is not None:
        allowed = {
            line.strip()
            for line in args.pages_list.read_text(encoding="utf-8").splitlines()
            if line.strip()
        }
        stems = [stem for stem in stems if stem in allowed]
    if args.limit:
        stems = stems[: args.limit]

    args.output_dir.mkdir(parents=True, exist_ok=True)
    detector = V2TableDetector(args.model_dir)

    report = {
        "schema_version": SCHEMA_VERSION,
        "artifact": "omnidocbench-reread-regions-sidecar",
        "model": f"{MODEL_ID} (paddle.inference, {TARGET_SIZE}, real output)",
        "model_id": MODEL_ID,
        "model_revision": MODEL_REVISION,
        "dpi": args.dpi,
        "pages_total": len(stems),
        "written": 0,
        "missing_pdfs": [],
        "render_errored": [],
        "detect_errored": [],
        "zero_table_pages": 0,
        "elapsed_seconds": 0.0,
    }
    started = time.time()
    for index, stem in enumerate(stems):
        pdf_path = args.pdfs / f"{stem}.pdf"
        if not pdf_path.is_file():
            report["missing_pdfs"].append(stem)
            continue
        try:
            with pymupdf.open(str(pdf_path)) as document:
                page = document[0]
                width, height = scan_render_size(
                    page.rect.width, page.rect.height, args.dpi
                )
                pix = page.get_pixmap(matrix=pymupdf.Matrix(
                    width / page.rect.width, height / page.rect.height
                ), alpha=False)
                image = np.frombuffer(pix.samples, dtype=np.uint8).reshape(
                    pix.height, pix.width, pix.n
                )[:, :, ::-1]  # RGB -> BGR for cv2
        except Exception as error:  # noqa: BLE001
            print(f"SKIP render failure {stem}: {error}", file=sys.stderr)
            report["render_errored"].append(stem)
            continue
        try:
            regions = detector.detect_tables(image)
        except Exception as error:  # noqa: BLE001
            print(f"SKIP detection failure {stem}: {error}", file=sys.stderr)
            report["detect_errored"].append(stem)
            continue
        if not regions:
            report["zero_table_pages"] += 1
        payload = {
            "schema_version": SCHEMA_VERSION,
            "model_id": MODEL_ID,
            "revision": MODEL_REVISION,
            "pages": [{"page": 1, "regions": regions}],
        }
        out_path = args.output_dir / f"{stem}.regions.json"
        out_path.write_text(
            json.dumps(payload, ensure_ascii=False), encoding="utf-8"
        )
        report["written"] += 1
        if (index + 1) % 50 == 0:
            print(f"{index + 1}/{len(stems)}", file=sys.stderr)
    report["elapsed_seconds"] = round(time.time() - started, 1)
    report_path = args.output_dir / "sidecar-report.json"
    report_path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps({k: v for k, v in report.items() if k != "artifact"}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
