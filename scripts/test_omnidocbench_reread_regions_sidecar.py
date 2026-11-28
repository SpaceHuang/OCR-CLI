#!/usr/bin/env python3
"""R3 sidecar unit checks: scan render size (mirrors the Rust pipeline),
cohort stems (distinct, frozen band order), provider-schema contract, and
the frozen preprocessing constants. Detection itself rides paddle (covered
by the B0'/pairing probe runs)."""

import importlib.util
import json
import sys
from pathlib import Path

# M4.3 P2a: the implementation moved to the sidecar package (single source
# of truth shared with the worker's regions frame mode); this batch CLI is a
# thin shell over it.
spec = importlib.util.spec_from_file_location(
    "sidecar",
    Path(__file__).resolve().parents[1] / "sidecar" / "table-engine" / "layout_regions.py",
)
sidecar = importlib.util.module_from_spec(spec)
sys.modules["sidecar"] = sidecar
spec.loader.exec_module(sidecar)


def test_scan_render_size_matches_the_rust_pipeline():
    # 612x792 pt at 300 dpi rounds to 2550x3300 which OVERSHOOTS the
    # 2880^2 budget -> proportional-floor fit.
    w, h = sidecar.scan_render_size(612.0, 792.0, 300)
    assert (w, h) == (2531, 3276), (w, h)
    assert w * h <= 2880 * 2880
    w, h = sidecar.scan_render_size(2000.0, 3000.0, 300)
    assert w * h <= 2880 * 2880
    raw_w, raw_h = round(2000.0 * 300 / 72), round(3000.0 * 300 / 72)
    scale = (2880 * 2880 / (raw_w * raw_h)) ** 0.5
    assert (w, h) == (max(1, int(raw_w * scale)), max(1, int(raw_h * scale)))
    print("ok   scan render size mirrors clamped_bitmap_size + fit_pixel_budget")


def test_page_stems_are_distinct_in_frozen_band_order():
    corpus = {
        "tables": {
            "worst": [{"image_path": "a.png"}],
            "mid": [{"image_path": "a.png"}, {"image_path": "b.png"}],
            "good-en": [],
            "good-zh": [{"image_path": "c.png"}],
            "good-other": [],
            "bad-low": [],
            "bad-dropped": [],
        }
    }
    assert sidecar.page_stems(corpus) == ["a", "b", "c"]
    print("ok   page stems distinct in frozen band order")


def test_provider_schema_assembly_contract():
    regions = [
        {"class": "table", "index_in_class": 0,
         "bbox_px": [10.0, 20.0, 300.0, 400.0], "conf": 0.93},
        {"class": "table", "index_in_class": 1,
         "bbox_px": [10.0, 500.0, 300.0, 800.0], "conf": 0.88},
    ]
    payload = {
        "schema_version": sidecar.SCHEMA_VERSION,
        "model_id": sidecar.MODEL_ID,
        "revision": sidecar.MODEL_REVISION,
        "pages": [{"page": 1, "regions": regions}],
    }
    assert payload["schema_version"] == 1
    assert payload["model_id"] == "pp-doclayout-v2"
    assert payload["revision"] == (
        "b73668227b14316a38f8b345d6b474e4f1f0b84d"
    ), "the frozen HF revision (matches the Rust manifest constant)"
    page = payload["pages"][0]
    assert page["page"] == 1
    assert [r["index_in_class"] for r in page["regions"]] == [0, 1]
    assert all(r["class"] == "table" for r in page["regions"])
    assert all(len(r["bbox_px"]) == 4 for r in page["regions"])
    assert json.loads(json.dumps(payload)) == payload
    print("ok   provider schema contract (serde_json round-trip)")


def test_frozen_preprocessing_constants():
    assert sidecar.TARGET_SIZE == 800
    assert sidecar.PIXEL_SCALE == 255.0
    assert sidecar.CONF_THRESHOLD == 0.25
    assert sidecar.TABLE_CLASS_ID == 21
    assert sidecar.FORMULA_CLASS_ID == 5
    assert sidecar.DEFAULT_DPI == 300
    assert sidecar.RENDER_PIXEL_BUDGET == 2880 * 2880
    print("ok   frozen preprocessing constants (D-45 manifest values)")


if __name__ == "__main__":
    spec.loader.exec_module(sidecar)
    test_scan_render_size_matches_the_rust_pipeline()
    test_page_stems_are_distinct_in_frozen_band_order()
    test_provider_schema_assembly_contract()
    test_frozen_preprocessing_constants()
    print("all reread-regions sidecar checks passed")
