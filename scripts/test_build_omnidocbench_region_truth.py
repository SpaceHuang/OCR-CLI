#!/usr/bin/env python3
"""B0a (BLK-5/D-32) unit checks: the GT-single-source region-truth builder.

Rule under test: a page is selected when it carries >= 1 GT `table` det or
>= 1 GT `equation_isolated`/`equation_semantic` det; the frozen artifact is
the stem list + per-region GT polys with per-class ordinals, byte-for-byte
deterministic (B0b must reuse the same frozen page list).
"""
import importlib.util
import json
import math
import subprocess
import sys
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "region_truth", Path(__file__).with_name("build-omnidocbench-region-truth.py")
)
rt = importlib.util.module_from_spec(spec)
sys.modules["region_truth"] = rt
spec.loader.exec_module(rt)


def check(name, condition):
    if not condition:
        print(f"FAIL {name}")
        sys.exit(1)
    print(f"ok   {name}")


def det(category, poly, order=0, anno="box"):
    return {
        "category_type": category,
        "poly": poly,
        "ignore": False,
        "order": order,
        "anno_id": anno,
        "attribute": {},
        "text": "",
    }


def page(image_path, dets, width=1000, height=2000):
    return {
        "layout_dets": dets,
        "page_info": {
            "page_no": 0,
            "height": height,
            "width": width,
            "image_path": image_path,
            "page_attribute": {"data_source": "book", "language": "english"},
        },
        "extra": {},
    }


TABLE_POLY = [100.0, 100.0, 400.0, 100.0, 400.0, 300.0, 100.0, 300.0]
EQ_POLY = [100.0, 500.0, 900.0, 500.0, 900.0, 700.0, 100.0, 700.0]
TEXT_POLY = [0.0, 0.0, 50.0, 0.0, 50.0, 40.0, 0.0, 40.0]

DATASET = [
    # selected: table + equation + noise classes
    page(
        "page-a.png",
        [
            det("text_block", TEXT_POLY, 1, "t0"),
            det("table", TABLE_POLY, 2, "t1"),
            det("equation_isolated", EQ_POLY, 3, "t2"),
            det("header", TEXT_POLY, 0, "t3"),
        ],
    ),
    # selected: equation_semantic only
    page("page-b.png", [det("equation_semantic", EQ_POLY, 1, "u0")]),
    # NOT selected: text-only page
    page("page-c.png", [det("text_block", TEXT_POLY, 1, "v0")]),
    # selected: two tables -> per-class ordinals 0,1
    page(
        "page-d.png",
        [
            det("table", TABLE_POLY, 5, "w0"),
            det("table", EQ_POLY, 2, "w1"),
        ],
    ),
]

MAPPING = {
    "pages": [
        {"stem": "stem-a", "image_path": "page-a.png"},
        {"stem": "stem-b", "image_path": "page-b.png"},
        {"stem": "stem-c", "image_path": "page-c.png"},
        {"stem": "stem-d", "image_path": "page-d.png"},
    ]
}


_UNSET = object()


def build(dataset=None, mapping=_UNSET):
    return rt.build_region_truth(
        dataset=DATASET if dataset is None else dataset,
        mapping=MAPPING if mapping is _UNSET else mapping,
    )


result = build()
check(
    "selection rule keeps only table/equation pages",
    [p["image_path"] for p in result["pages"]] == ["page-a.png", "page-b.png", "page-d.png"],
)
page_a = result["pages"][0]
check(
    "noise classes filtered from regions",
    [r["class"] for r in page_a["regions"]] == ["table", "equation_isolated"],
)
page_d = result["pages"][2]
check(
    "per-class ordinal follows document order",
    [(r["class"], r["index_in_class"]) for r in page_d["regions"]]
    == [("table", 0), ("table", 1)],
)
check(
    "table ordinal sorted by reading order not det order",
    page_d["regions"][0]["anno_id"] == "w1",
)
check(
    "counts are recorded",
    result["counts"]
    == {
        "pages": 3,
        "pages_with_table": 2,
        "pages_with_equation": 2,
        "table_regions": 3,
        "equation_regions": 2,
        "out_of_bounds_regions": 0,
    },
)
check(
    "stems join the mapping",
    [p["stem"] for p in result["pages"]] == ["stem-a", "stem-b", "stem-d"],
)

# Determinism: identical dicts -> identical canonical bytes.
check(
    "canonical bytes are deterministic",
    rt.canonical_bytes(build()) == rt.canonical_bytes(build()),
)

# Malformed poly fails loudly.
try:
    build(
        dataset=[
            page("page-x.png", [det("table", [1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 1, "bad")])
        ]
    )
    print("FAIL malformed poly must raise")
    sys.exit(1)
except rt.RegionTruthError:
    print("ok   malformed poly raises")

# NaN poly fails loudly.
try:
    build(dataset=[page("page-x.png", [det("table", [math.nan] * 8, 1, "bad")])])
    print("FAIL NaN poly must raise")
    sys.exit(1)
except rt.RegionTruthError:
    print("ok   NaN poly raises")

# Out-of-bounds poly is recorded, not fatal.
result = build(
    dataset=[page("page-x.png", [det("table", [-50.0, -50.0, 200.0, -50.0, 200.0, 100.0, -50.0, 100.0], 1, "oob")])],
    mapping=None,
)
check(
    "out-of-bounds counted in the artifact",
    result["counts"]["out_of_bounds_regions"] == 1,
)

# Mapping must cover every selected page (denominator integrity).
try:
    build(mapping={"pages": [{"stem": "stem-a", "image_path": "page-a.png"}]})
    print("FAIL incomplete mapping must raise")
    sys.exit(1)
except rt.RegionTruthError:
    print("ok   incomplete mapping raises")

# --- CLI end-to-end: build -> verify round trip + tamper detection ----------

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    dataset_path = root / "dataset.json"
    dataset_path.write_text(json.dumps(DATASET), encoding="utf-8")
    mapping_path = root / "mapping.json"
    mapping_path.write_text(json.dumps(MAPPING), encoding="utf-8")
    out_dir = root / "region-truth"
    script = str(Path(__file__).with_name("build-omnidocbench-region-truth.py"))
    for round_index in range(2):
        proc = subprocess.run(
            [
                sys.executable,
                script,
                "--dataset",
                str(dataset_path),
                "--mapping",
                str(mapping_path),
                "--output-dir",
                str(out_dir),
            ],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if proc.returncode != 0:
            print(f"FAIL cli build round {round_index}: {proc.stderr}")
            sys.exit(1)
    print("ok   cli build runs and is idempotent")
    first = (out_dir / "region-truth.json").read_bytes()
    stems = (out_dir / "region-truth-stems.txt").read_text(encoding="utf-8")
    check(
        "stems file lists selected pages",
        stems.splitlines() == ["page-a.png", "page-b.png", "page-d.png"],
    )
    # Verify mode passes on the untouched artifact.
    proc = subprocess.run(
        [
            sys.executable,
            script,
            "--dataset",
            str(dataset_path),
            "--mapping",
            str(mapping_path),
            "--output-dir",
            str(out_dir),
            "--verify",
        ],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if proc.returncode != 0:
        print(f"FAIL verify on untouched artifact: {proc.stderr}")
        sys.exit(1)
    print("ok   verify passes on the untouched artifact")
    # Tamper: any byte change must fail verification.
    (out_dir / "region-truth.json").write_bytes(first + b" ")
    proc = subprocess.run(
        [
            sys.executable,
            script,
            "--dataset",
            str(dataset_path),
            "--mapping",
            str(mapping_path),
            "--output-dir",
            str(out_dir),
            "--verify",
        ],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if proc.returncode == 0:
        print("FAIL tampered artifact must fail verify")
        sys.exit(1)
    print("ok   tampered artifact fails verify")

print("all region-truth checks passed")
