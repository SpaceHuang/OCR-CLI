#!/usr/bin/env python3
"""B2 adjudicator unit checks: key normalization, gate arithmetic, and the
frozen thresholds on SYNTHETIC fixtures (the campaign run supplies the real
data; these pin the gate semantics)."""

import importlib.util
import json
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "b2", Path(__file__).with_name("omnidocbench-b2-adjudicate.py")
)
b2 = importlib.util.module_from_spec(spec)


def write_doc(scans: Path, stem: str, reread: int, replaced: int, enabled: bool):
    doc = scans / stem
    doc.mkdir(parents=True, exist_ok=True)
    (doc / "result.json").write_text(json.dumps({
        "schema_version": 3,
        "status": "success",
        "pages": {"total": 1, "completed": 1, "failed": 0,
                  "tables_reread_total": reread, "replaced_total": replaced},
        "reread": {"enabled": enabled, "prompt_version": "table-reread-v1",
                   "rules_generation": 1},
    }), encoding="utf-8")


def teds(value: float) -> dict:
    return {"TEDS": value, "TEDS_structure_only": value}


def make_world(tmp: Path, worst_delta: float, good_delta: float,
               regressions: int, replaced_normal: int, triggered_pool: int):
    corpus = {"tables": {
        "worst": [{"key": "w.png_[0]", "image_path": "w.png_[0]", "page_bad": False},
                  {"key": "w2.png_[0]", "image_path": "w2.png_[0]", "page_bad": False}],
        "mid": [],
        "bad-low": [{"key": "b.png_[0]", "image_path": "b.png_[0]", "page_bad": True}],
        "good-en": [{"key": "g.jpg_[0]", "image_path": "g.png_[0]", "page_bad": False}],
        "good-zh": [{"key": "gz.jpg_[0]", "image_path": "gz.png_[0]", "page_bad": False}],
        "good-other": [],
        "bad-dropped": [],
    }}
    pairing = {"per_page": [
        {"image_path": "w.png", "verdict": "pair"},
        {"image_path": "w2.png", "verdict": "pair"},
        {"image_path": "b.png", "verdict": "skip", "reason": "ordering_ambiguous"},
        {"image_path": "g.png", "verdict": "pair"},
        {"image_path": "gz.png", "verdict": "pair"},
    ]}
    control_teds = {"w_[0]": teds(0.4), "w2_[0]": teds(0.4),
                    "g_[0]": teds(0.9), "gz_[0]": teds(0.9)}
    target_teds = {
        "w_[0]": teds(0.4 + worst_delta),
        "w2_[0]": teds(0.4),  # w2's page never triggered -> delta 0
        "g_[0]": teds(0.9 + good_delta),
        "gz_[0]": teds(0.9 - 0.5 if regressions else 0.9),
    }
    control_scans = tmp / "control"
    target_scans = tmp / "target"
    baseline_scans = tmp / "baseline"
    for stem in ("w", "w2", "b", "g", "gz"):
        for root in (control_scans, target_scans, baseline_scans):
            doc = root / stem
            doc.mkdir(parents=True, exist_ok=True)
            (doc / "page-0001.json").write_text("{}", encoding="utf-8")
            (doc / "page-0001.md").write_text("body\n", encoding="utf-8")
        reread, replaced, enabled = 0, 0, False
        if stem == "w":
            reread, enabled = triggered_pool, True
        if stem == "g":
            replaced, enabled = replaced_normal, True
        write_doc(target_scans, stem, reread, replaced, enabled)
        write_doc(control_scans, stem, 0, 0, False)
        write_doc(baseline_scans, stem, 0, 0, False)
    return b2.adjudicate(
        corpus, pairing, control_teds, target_teds,
        control_scans, target_scans, baseline_scans,
    )


def test_key_normalization():
    assert b2.normalize_key("PPT x_page_010.png_[0]") == "PPT x_page_010_[0]"
    assert b2.normalize_key("newspaper_a_1.jpg_[2]") == "newspaper_a_1_[2]"
    assert b2.normalize_key("g.jpg_[0]") == "g_[0]"
    print("ok   key normalization strips the GT image extension")


def test_gate_coverage():
    with tempfile.TemporaryDirectory() as raw:
        tmp = Path(raw)
        # Pool (safe-eligible) = w, w2 (b's page is skipped/ambiguous).
        # Triggered: w only -> 1/2 = 50% -> FAIL; absolute 1/3.
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=0, replaced_normal=0, triggered_pool=1)
        gate = result["gates"]["coverage"]
        assert gate["pool_tables_safe_eligible"] == 2, gate
        assert gate["triggered"] == 1
        assert abs(gate["coverage"] - 0.5) < 1e-9
        assert not gate["pass"]
        assert gate["absolute_pool_tables"] == 3
        print("ok   gate-1 coverage counts pool tables on pairing-PASS pages")


def test_gate_replacement():
    with tempfile.TemporaryDirectory() as raw:
        tmp = Path(raw)
        # Normal members = w, w2, g, gz (4); replaced on g = 1 -> 25% > 2%.
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=0, replaced_normal=1, triggered_pool=2)
        gate = result["gates"]["replacement"]
        assert gate["normal_page_table_samples"] == 4, gate
        assert gate["replaced"] == 1
        assert not gate["pass"]
        # Zero replacements pass.
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=0, replaced_normal=0, triggered_pool=2)
        assert result["gates"]["replacement"]["pass"]
        print("ok   gate-2 replacement rate over normal-page samples")


def test_gate_good_band():
    with tempfile.TemporaryDirectory() as raw:
        tmp = Path(raw)
        # Healthy good band: mean 0 -> pass; no regressions.
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=0, replaced_normal=0, triggered_pool=2)
        gate = result["gates"]["good_band"]
        assert gate["paired_tables"] == 2
        assert gate["pass"]
        # One catastrophic regression out of 2 -> cap = max(3, 0) = 3 ->
        # 1 <= 3 passes the count BUT the mean sinks below -0.02 -> FAIL.
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=1, replaced_normal=0, triggered_pool=2)
        gate = result["gates"]["good_band"]
        assert gate["regressions"] == 1
        assert gate["regression_cap"] == 3
        assert not gate["pass"], "mean -0.25 breaches the mean floor"
        print("ok   gate-3 mean floor + regression cap")


def test_gate_worst_bad():
    with tempfile.TemporaryDirectory() as raw:
        tmp = Path(raw)
        # Safe-eligible worst/bad subset = w, w2 (b skipped).
        # w: +0.4, w2: 0 -> mean 0.2 < 0.3 -> FAIL.
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=0, replaced_normal=0, triggered_pool=2)
        gate = result["gates"]["worst_bad"]
        assert gate["paired_tables"] == 2
        assert abs(gate["mean_delta"] - 0.2) < 1e-9
        assert not gate["pass"]
        print("ok   gate-4 worst/bad paired mean on the safe-eligible subset")


def test_gate_projection_drift():
    with tempfile.TemporaryDirectory() as raw:
        tmp = Path(raw)
        result = make_world(tmp, worst_delta=0.4, good_delta=0.0,
                            regressions=0, replaced_normal=0, triggered_pool=2)
        gate = result["gates"]["projection_drift"]
        assert gate["zero_drift"]["checked"] == 10, gate
        assert gate["zero_drift"]["pass"]
        assert not gate["pass"], "projection not yet recorded"
        assert result["verdict"] == "NO-GO"
        print("ok   gate-5 drift leg + projection recording flag")


if __name__ == "__main__":
    spec.loader.exec_module(b2)
    test_key_normalization()
    test_gate_coverage()
    test_gate_replacement()
    test_gate_good_band()
    test_gate_worst_bad()
    test_gate_projection_drift()
    print("all b2 adjudicator checks passed")
