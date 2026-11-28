"""M4.3 P2a local golden tests: the layout_regions module must reproduce the
frozen benchmark sidecar detections BYTE-IDENTICALLY (single source of truth:
the script now imports this module, and the worker's regions frame mode uses
the same implementation). Requires paddle + pymupdf + machine-local artifacts
— skips anywhere else."""

import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

pytest.importorskip("paddle", reason="paddle required (local golden test)")
pytest.importorskip("pymupdf", reason="pymupdf required (local golden test)")

BOUND = Path(r"D:/t/m43-p0/regions-bound")
INPUTS = Path(r"D:/t/m43-p0/inputs")
MODEL_DIR = Path(r"C:/Users/WIN11/AppData/Local/ocrx/models/pp-doclayout-v2")
if not (BOUND / "sidecar-report.json").exists() or not INPUTS.is_dir():
    pytest.skip("local bound sidecars not present on this machine", allow_module_level=True)

_installed = MODEL_DIR / "b73668227b14316a38f8b345d6b474e4f1f0b84d"
if not _installed.is_dir():
    pytest.skip("installed pp-doclayout-v2 model dir not found", allow_module_level=True)

import io  # noqa: E402

import layout_regions  # noqa: E402
import worker  # noqa: E402


def _golden_stems(n: int) -> list[str]:
    stems = []
    for path in sorted(BOUND.glob("*.regions.json")):
        if path.name == "sidecar-report.json":
            continue
        # `path.stem` would keep the trailing `.regions` (double-suffix name)
        stems.append(path.name[: -len(".regions.json")])
        if len(stems) == n:
            break
    return stems


def test_module_regions_are_byte_identical_to_frozen_sidecars():
    """First 3 frozen pages: the module's regions_for_pdf payload must match
    the benchmark sidecar's regions arrays byte-for-byte (same rendering,
    same frozen preprocessing, same detector)."""
    for stem in _golden_stems(3):
        frozen = json.loads((BOUND / f"{stem}.regions.json").read_text(encoding="utf-8"))
        module_payload = json.loads(
            layout_regions.LayoutRegions().regions_for_pdf(
                str(INPUTS / f"{stem}.pdf"), str(_installed)
            )
        )
        assert module_payload["schema_version"] == frozen["schema_version"]
        assert module_payload["model_id"] == frozen["model_id"]
        assert module_payload["revision"] == frozen["revision"]
        assert module_payload["source_pdf_sha256"] == frozen["source_pdf_sha256"]
        assert module_payload["pages"] == frozen["pages"], f"{stem}: regions drift"


def test_worker_regions_frame_serves_route_a_payload():
    """The worker's `regions` frame mode answers with a route-A payload over
    the in-process serve() loop (protocol + binding + coverage)."""

    class FakeStdin:
        def __init__(self, frames):
            self.buf = io.BytesIO(b"".join(f + b"\n" for f in frames))

        def readline(self):
            return self.buf.readline()

    stem = _golden_stems(1)[0]
    frames = [
        json.dumps(
            {
                "type": "regions",
                "id": 1,
                "pdf_path": str(INPUTS / f"{stem}.pdf"),
                "model_dir": str(_installed),
            }
        ).encode(),
    ]
    out = io.BytesIO()
    worker.serve(FakeStdin(frames), out)
    lines = out.getvalue().decode().splitlines()
    assert json.loads(lines[0])["type"] == "hello"
    reply = json.loads(lines[1])
    assert reply["type"] == "result" and reply["id"] == 1 and reply["ok"]
    payload = json.loads(reply["regions_json"])
    frozen = json.loads((BOUND / f"{stem}.regions.json").read_text(encoding="utf-8"))
    assert payload["source_pdf_sha256"] == frozen["source_pdf_sha256"]
    assert payload["pages"] == frozen["pages"]
