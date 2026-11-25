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
# The smoke consumes machine-local T0 artifacts; without them (any other
# machine / a fresh clone) it must SKIP, never fail or run for 70+ minutes.
_t0_crops = Path(r"D:/t/m42-t0-crops")
if not (_t0_crops / "regions-manifest.json").exists():
    pytest.skip("local T0 crops not present on this machine", allow_module_level=True)

import worker  # noqa: E402

CROPS = Path(r"D:/t/m42-t0-crops")
PRED = Path(r"D:/t/slanet-det-predictions")
FREEZE = (
    Path(__file__).resolve().parents[3]
    / "benchmarks/omnidocbench/reports/analysis/analysis-m42-t0-postgate-v1p2.json"
)


def test_hello_reports_frozen_versions_and_model_hashes():
    hello = json.loads(worker.build_hello())
    assert hello["engine"] == "slanet-plus"
    assert hello["models_revision"] == "bae6e5f8"
    assert hello["protocol_revision"] == "proto2"
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
    errs = {}
    for r in results:
        if r["type"] == "error":
            errs[r["id"]] = r["message"]
            continue
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

    # M4.3 P0 (analysis-m43-p0-throw-diagnosis.json): the 9 crops that used
    # to crash worker-side (empty table_res_list indexed with [0] -> IndexError
    # -> engine_error) now take the honest empty_output reject path — the
    # pipeline completed but found no table in those sliver/fragment crops.
    # The 2 no_ruling_lines are 1417's EXTRA pseudo region and its GT r3
    # (no ruling lines). 16 regions produce HTML (engine-level; the adoption
    # gates structural/attribution live Rust-side). Zero error frames.
    EXPECT_EMPTY_STEMS = (
        "jiaocaineedrop_2017",
        "jiaocaineedrop_jiaocai_needrop_en_1349",
        "jiaocaineedrop_jiaocai_needrop_en_1417",
        "jiaocaineedrop_jiaocai_needrop_en_2211",
        "jiaocaineedrop_jiaocai_needrop_en_3713",
        "yanbaor2_89c3718f",
    )
    for rid, (page, row, parts) in ids.items():
        gt_key = row["gt_key"]
        expect_empty = page["img"].startswith(EXPECT_EMPTY_STEMS) and row["crop_png"] not in (
            "p04_r0_EXTRA_conf0.98.png",
            "p04_r3_match0_conf0.97.png",
        )
        # 0aa9 produces HTML at the ENGINE level (the attribution gate that
        # rejects it lives Rust-side) — expected ok here.
        expect_ok = gt_key in adopted_keys or gt_key.endswith("0aa9545ccbc482a37e3b55c96897f0f24cee08bb80c7addfb0c219814af44a3b.pdf_27.jpg_[0]")
        if expect_ok:
            assert rid in got_ok, f"{gt_key} expected adopted"
            assert got_ok[rid] == parts[row["region"]], f"{gt_key} html drift"
        elif expect_empty:
            assert rid in got_reject and got_reject[rid] == "empty_output", (
                f"{gt_key} expected empty_output reject: rid={rid}"
            )
        else:
            assert rid in got_reject, f"{gt_key} expected reject: rid={rid}"
            if row["crop_png"].startswith("p04_r0_EXTRA"):
                assert got_reject[rid] == "no_ruling_lines"
    assert len(got_ok) == 16, f"got {len(got_ok)} ok verdicts"
    assert len(got_reject) == 11, f"got {len(got_reject)} rejects (2 no_ruling_lines + 9 empty_output)"
    assert len(errs) == 0, f"got {len(errs)} error frames (P0 fix: none expected)"
