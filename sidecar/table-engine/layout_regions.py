"""Layout regions generation (M4.3 P2a, D-71 route A schema).

Single source of truth for the frozen PP-DocLayout-V2 table detection:
`scripts/omnidocbench-reread-regions-sidecar.py` is a thin CLI over this
module (byte-identical preprocessing — the frozen B0' reference), and the
table-engine worker's `regions` frame mode consumes `regions_for_pdf`.

Route A payload contract (validated Rust-side by region_provider.rs):
    {
      "schema_version": 1,
      "model_id": "pp-doclayout-v2",
      "revision": "<frozen>",
      "source_pdf_sha256": "<sha256 of the pdf bytes>",
      "pages": [{"page": 1, "regions": [...]}, ...]   # 1-based, every page
    }

Heavy imports (paddle.inference / pymupdf) are lazy — the protocol/fixture
tests import this module anywhere.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from pathlib import Path

SCHEMA_VERSION = 1
MODEL_ID = "pp-doclayout-v2"
MODEL_REVISION = "b73668227b14316a38f8b345d6b474e4f1f0b84d"
TARGET_SIZE = 800
PIXEL_SCALE = 255.0
CONF_THRESHOLD = 0.25
TABLE_CLASS_ID = 21
# Formula regions are recorded in the frozen B0' constant set but not yet
# consumed by the table-only v1 detector (M5 owns the formula arm).
FORMULA_CLASS_ID = 5
DEFAULT_DPI = 300
# The scan's frozen normal-path render budget (MAX_RENDER_PIXELS).
RENDER_PIXEL_BUDGET = 2880 * 2880


def scan_render_size(page_width_pt: float, page_height_pt: float, dpi: int) -> tuple[int, int]:
    """Mirror clamped_bitmap_size + fit_pixel_budget in
    src/ocrx-pdf/src/lib.rs (round, floor on the fit, min 1).
    """
    raw_w = max(1.0, round(page_width_pt * dpi / 72.0))
    raw_h = max(1.0, round(page_height_pt * dpi / 72.0))
    pixels = raw_w * raw_h
    if pixels <= RENDER_PIXEL_BUDGET or RENDER_PIXEL_BUDGET == 0:
        return int(raw_w), int(raw_h)
    scale = (RENDER_PIXEL_BUDGET / pixels) ** 0.5
    return max(1, int(raw_w * scale)), max(1, int(raw_h * scale))


def render_page_bgr(pdf_path: Path, page_index: int, dpi: int) -> "np.ndarray":  # noqa: F821
    """Render one page to the scan bitmap space (PyMuPDF, geometry-identical:
    the pixel space only depends on the computed target size)."""
    import pymupdf

    with pymupdf.open(str(pdf_path)) as document:
        page = document[page_index]
        width, height = scan_render_size(page.rect.width, page.rect.height, dpi)
        pix = page.get_pixmap(
            matrix=pymupdf.Matrix(width / page.rect.width, height / page.rect.height),
            alpha=False,
        )
        import numpy as np

        rgb = np.frombuffer(pix.samples, dtype=np.uint8).reshape(
            pix.height, pix.width, pix.n
        )
        return rgb[:, :, ::-1]  # RGB -> BGR for cv2


class V2TableDetector:
    """The B0'/pairing-validated PP-DocLayoutV2 table detector (route A).

    Detection rides paddle.inference with the frozen preprocessing.
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

    def detect_tables(self, image_bgr) -> list[dict]:
        import cv2
        import numpy as np

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


class LayoutRegions:
    """Lazy layout detector wrapper for the worker's regions frame mode."""

    def __init__(self) -> None:
        self._detector: V2TableDetector | None = None
        self._model_dir: str | None = None

    def _ensure_loaded(self, model_dir: str) -> V2TableDetector:
        if self._detector is None or self._model_dir != model_dir:
            self._detector = V2TableDetector(model_dir)
            self._model_dir = model_dir
        return self._detector

    def regions_for_pdf(self, pdf_path: str, model_dir: str, dpi: int = DEFAULT_DPI) -> str:
        """Route-A payload for EVERY page of the pdf (1-based, coverage
        complete — the Rust provider validates page coverage against the
        document). Returns the JSON string (the frame carries it verbatim)."""
        import pymupdf

        path = Path(pdf_path)
        detector = self._ensure_loaded(model_dir)
        source_sha = hashlib.sha256(path.read_bytes()).hexdigest()
        pages = []
        with pymupdf.open(str(path)) as document:
            page_count = document.page_count
        for page_index in range(page_count):
            image = render_page_bgr(path, page_index, dpi)
            regions = detector.detect_tables(image)
            pages.append({"page": page_index + 1, "regions": regions})
        payload = {
            "schema_version": SCHEMA_VERSION,
            "model_id": MODEL_ID,
            "revision": MODEL_REVISION,
            "source_pdf_sha256": source_sha,
            "pages": pages,
        }
        return json.dumps(payload, ensure_ascii=False)


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


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv=None) -> int:
    """The benchmark batch CLI (scripts/omnidocbench-reread-regions-sidecar.py
    delegates here — identical behavior, one detection implementation)."""
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
            image = render_page_bgr(pdf_path, 0, args.dpi)
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
        # M4.2 RBLK-1: bind the sidecar to its SOURCE PDF (sha256 over the
        # pdf bytes) — the consumer validates this against the scan input
        # before trusting any region.
        source_sha = hashlib.sha256(pdf_path.read_bytes()).hexdigest()
        payload = {
            "schema_version": SCHEMA_VERSION,
            "model_id": MODEL_ID,
            "revision": MODEL_REVISION,
            "source_pdf_sha256": source_sha,
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
