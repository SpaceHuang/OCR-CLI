"""End-to-end worker smoke (M4.2 T1 acceptance): in-process serve() against
real crops, expectations = the T0 POST-GATE frozen set
(analysis-m42-t0-postgate-v1p2.json). Requires paddle — local only."""

import json
import sys
import base64
import io
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

pytest.importorskip("paddleocr", reason="paddle required (local smoke)")

import worker  # noqa: E402

CROPS = Path(r"D:/t/m42-t0-crops")
PRED = Path(r"D:/t/slanet-det-predictions")
FREEZE = Path(
    r"D:/Develop/OCR-CLI/.worktree/next-phase-quality/"
    "benchmarks/omnidocbench/reports/analysis/analysis-m42-t0-postgate-v1p2.json"
)


def test_hello_reports_frozen_versions_and_model_hashes():
    hello = json.loads(worker.build_hello())
    assert hello["engine"] == "slanet-plus"
    assert hello["revision"] == "bae6e5f8"
    assert hello["paddleocr"] == "3.7.0"
    assert hello["paddlex"] == "3.7.2"
    assert hello["paddlepaddle"] == "3.4.0"
    assert len(hello["models_sha256"]) == len(worker.TABLE_MODEL_DIRS)


def test_worker_serves_matches_t0_postgate_set():
    """Batch all 27 T0 regions through serve(); the adopted/reject split must
    equal the frozen post-gate set, and adopted HTML must be byte-identical
    to the T0 predictions (same model, same input)."""
    manifest = json.loads((CROPS / "regions-manifest.json").read_text(encoding="utf-8"))
    freeze = json.loads(FREEZE.read_text(encoding="utf-8"))
    adopted_keys = set(freeze["adopted_keys"])

    class FakeStdin:
        def __init__(self, frames):
            self.buf = io.BytesIO(b"".join(f + b"\n" for f in frames))

        def readline(self):
            return self.buf.readline()

    out = io.BytesIO()
    frames = []
    for page in manifest:
        stem = page["img"].rsplit(".", 1)[0]
        for row in page["rows"]:
            png = (CROPS / row["crop_png"]).read_bytes()
            frames.append(
                json.dumps(
                    {
                        "type": "recognize",
                        "id": len(frames),
                        "png_b64": base64.b64encode(png).decode(),
                        "page": 0,
                        "ordinal": row["region"],
                    }
                ).encode()
            )
    worker.serve(FakeStdin(frames), out)

    lines = out.getvalue().decode().splitlines()
    hello = json.loads(lines[0])
    assert hello["type"] == "hello"
    results = [json.loads(l) for l in lines[1:]]
    # Evidence cache FIRST (a 70+ minute run must never lose its compute to
    # a late assertion): dump the raw verdicts before any comparison.
    Path(r"D:/t/m42-smoke-results.json").write_text(
        json.dumps({"hello": hello, "results": results}, ensure_ascii=False),
        encoding="utf-8",
    )

    got_ok = {}
    got_reject = {}
    for r in results:
        assert r["type"] == "result", r
        if r["ok"]:
            got_ok[r["id"]] = r["html"]
        else:
            got_reject[r["id"]] = r["reject"]

    # Rebuild the id -> (img, region) mapping and compare with the freeze.
    idx = 0
    ids = {}
    for page in manifest:
        stem = page["img"].rsplit(".", 1)[0]
        parts = (PRED / f"{stem}.md").read_text(encoding="utf-8").split("\n\n")
        for row in page["rows"]:
            ids[idx] = (page, row, parts)
            idx += 1
    assert len(results) == 27

    for rid, (page, row, parts) in ids.items():
        gt_key = row["gt_key"]
        if gt_key in adopted_keys:
            assert rid in got_ok, f"{gt_key} expected adopted"
            assert got_ok[rid] == parts[row["region"]], f"{gt_key} html drift"
        else:
            assert rid in got_reject, f"{gt_key} expected reject"
            if row["crop_png"].startswith("p04_r0_EXTRA"):
                assert got_reject[rid] == "no_ruling_lines"
    assert len(got_ok) == 15
    assert len(got_reject) == 12
