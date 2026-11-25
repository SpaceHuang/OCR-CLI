"""Table-engine sidecar worker (M4.2 T1, PROTOCOL.md).

Launch (Rust spawns via OCRX_TABLE_ENGINE_CMD):
    python -X utf8 -u worker.py

Startup order (D-59: the handshake precedes any model load):
    1. hash the frozen model dirs (audit identity, no paddle import)
    2. send the hello frame
    3. serve recognize/ping frames; the pipeline loads lazily on the first
       recognize that passes the ruling gate
"""

from __future__ import annotations

import base64
import hashlib
import importlib.metadata
import sys
from pathlib import Path

import engine_protocol as proto
import ruling_gate

# Model dirs the TableRecognitionPipelineV2 supply chain draws from
# (hashed for the hello identity; absence = first-run download incomplete,
# the handshake will fail closed with install guidance — README).
TABLE_MODEL_DIRS = (
    "PP-LCNet_x1_0_doc_ori",
    "PP-LCNet_x1_0_table_cls",
    "PP-OCRv4_server_det",
    "PP-OCRv4_server_rec_doc",
    "RT-DETR-L_wired_table_cell_det",
    "RT-DETR-L_wireless_table_cell_det",
    "SLANeXt_wired",
    "SLANeXt_wireless",
)


def _metadata_version(dist: str) -> str:
    try:
        return importlib.metadata.version(dist)
    except importlib.metadata.PackageNotFoundError:
        return "missing"


def _hash_model_dir(root: Path) -> dict:
    """sha256 over the dir's files, sorted by relative path (stable)."""
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if path.is_file():
            rel = path.relative_to(root).as_posix()
            digest.update(rel.encode("utf-8"))
            digest.update(path.read_bytes())
    return {"files": sum(1 for p in root.rglob("*") if p.is_file()), "sha256": digest.hexdigest()}


def build_hello() -> str:
    models_root = Path.home() / ".paddlex" / "official_models"
    models = {}
    for name in TABLE_MODEL_DIRS:
        model_dir = models_root / name
        if model_dir.is_dir():
            models[name] = _hash_model_dir(model_dir)
    return proto.Hello(
        engine=proto.ENGINE_ID,
        models_revision=proto.EXPECTED_MODELS_REVISION,
        protocol_revision=proto.EXPECTED_PROTOCOL_REVISION,
        paddleocr=_metadata_version("paddleocr"),
        paddlex=_metadata_version("paddlex"),
        paddlepaddle=_metadata_version("paddlepaddle"),
        models_sha256=models,
    ).to_json()


class TablePipeline:
    """Lazy TableRecognitionPipelineV2 wrapper — the spike-frozen config."""

    def __init__(self) -> None:
        self._pipe = None

    def _ensure_loaded(self):
        if self._pipe is None:
            from paddleocr import TableRecognitionPipelineV2  # heavy import

            self._pipe = TableRecognitionPipelineV2(
                device="cpu",
                use_doc_orientation_classify=False,
                use_doc_unwarping=False,
                enable_mkldnn=False,
            )
        return self._pipe

    def recognize(self, png_bytes: bytes) -> str:
        import numpy as np
        import cv2

        image = cv2.imdecode(np.frombuffer(png_bytes, dtype=np.uint8), cv2.IMREAD_COLOR)
        if image is None:
            raise ValueError("recognize: undecodable png")
        pipe = self._ensure_loaded()
        result = pipe.predict(image)
        res = result[0] if isinstance(result, list) else result
        res_json = (res.json if hasattr(res, "json") else {}).get("res", {})
        # M4.3 P0 fix: the pipeline completes honestly with an EMPTY
        # table_res_list when no table is found in the crop (diagnosis:
        # analysis-m43-p0-throw-diagnosis.json) — the previous [0] index
        # crashed worker-side and surfaced as engine_error. Empty list =
        # no structure = the existing empty-output reject path.
        table_res_list = res_json.get("table_res_list") or []
        if not table_res_list:
            return ""
        return table_res_list[0].get("pred_html", "") or ""


def serve(in_stream=None, out_stream=None) -> int:
    in_stream = in_stream if in_stream is not None else sys.stdin.buffer
    out_stream = out_stream if out_stream is not None else sys.stdout.buffer
    pipeline = TablePipeline()
    proto.write_frame(out_stream, build_hello())
    while True:
        line = proto.read_frame(in_stream)
        if line is None:
            return 0  # clean EOF shutdown
        try:
            request = proto.Recognize.from_json(line)
        except proto.ProtocolError:
            try:
                ping = proto.Ping.from_json(line)
            except proto.ProtocolError as error:
                proto.write_frame(out_stream, proto.error_frame(-1, str(error)))
                return 2  # unusable connection
            proto.write_frame(out_stream, proto.result_ok(ping.id, "pong"))
            continue
        try:
            png_bytes = request.png_bytes
            if not ruling_gate.has_ruling_lines(png_bytes):
                proto.write_frame(
                    out_stream, proto.result_reject(request.id, proto.REJECT_NO_RULING_LINES)
                )
                continue
            html = pipeline.recognize(png_bytes)
            if not html.strip():
                proto.write_frame(
                    out_stream, proto.result_reject(request.id, proto.REJECT_EMPTY_OUTPUT)
                )
            else:
                proto.write_frame(out_stream, proto.result_ok(request.id, html))
        except Exception as error:  # per-request fault: report, keep serving (D-63 (3))
            proto.write_frame(out_stream, proto.error_frame(request.id, f"{type(error).__name__}: {error}"))


if __name__ == "__main__":
    raise SystemExit(serve())
