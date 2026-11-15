#!/usr/bin/env python3
"""B0b (next-phase plan §4.2) unit checks: the full experiment-corpus freeze.

Contract under test (page set reuses the B0a frozen list, D-32):
  tables   worst/mid/good-en/good-zh (good pages, landing per-table TEDS
           thresholds 0.5/0.85) + good-other + bad-low/bad-dropped (121-page
           ledger members);
  formulas fracture subset (good pages CDM < 0.5 + ALL bad-page formulas,
           landing per-sample CDM baseline) + protected set (seeded, >= 40
           good-page CDM >= 0.5 samples, stratified en/zh, excludes fracture)
           + formula page list;
  every member page must be in the B0a region-truth stems; keys are the
  official sample keys `<image_path>_[<cat_ordinal>]` (T-PROJ population
  compatible); artifact is canonical byte-stable; --verify byte-compares.
"""
import importlib.util
import json
import subprocess
import sys
import tempfile
from pathlib import Path


def subprocess_run(args):
    return subprocess.run(args, capture_output=True, text=True, timeout=60)

spec = importlib.util.spec_from_file_location(
    "b0b_corpus", Path(__file__).with_name("omnidocbench-freeze-b0b-corpus.py")
)
b0b = importlib.util.module_from_spec(spec)
sys.modules["b0b_corpus"] = b0b
spec.loader.exec_module(b0b)


def check(name, condition):
    if not condition:
        print(f"FAIL {name}")
        sys.exit(1)
    print(f"ok   {name}")


def det(category, order=0):
    return {
        "category_type": category,
        "poly": [0, 0, 10, 0, 10, 10, 0, 10],
        "ignore": False,
        "order": order,
        "anno_id": "box",
        "attribute": {},
        "text": "",
    }


def page(image_path, dets, lang="english", source="book"):
    return {
        "layout_dets": dets,
        "page_info": {
            "page_no": 0,
            "height": 100,
            "width": 100,
            "image_path": image_path,
            "page_attribute": {"data_source": source, "language": lang},
        },
        "extra": {},
    }


GT = [
    # good english page: worst + mid tables
    page("a.png", [det("table"), det("table"), det("text_block")], lang="english"),
    # good zh page: good-zh table
    page("b.png", [det("table")], lang="simplified_chinese"),
    # BAD page (truncated): its table -> bad-low even at 0.85
    page("c.png", [det("table")], lang="english"),
    # good english page: worst table (0.4)
    page("d.png", [det("table")], lang="english"),
    # good english page: boundary-inclusive good-en table
    page("l.png", [det("table")], lang="english"),
    # formulas: fracture (0.2) + protected-pool (0.95)
    page("e.png", [det("equation_isolated"), det("equation_semantic")]),
    # good zh formula in the protected pool
    page("f.png", [det("equation_isolated")], lang="simplified_chinese"),
    # BAD page formula -> fracture regardless of CDM
    page("g.png", [det("equation_isolated")], lang="english"),
    # extra zh pool members
    page("j.png", [det("equation_isolated"), det("equation_isolated"), det("equation_isolated")],
         lang="simplified_chinese"),
    # extra en pool members
    page("k.png", [det("equation_isolated"), det("equation_isolated")], lang="english"),
    # no regions: not in B0a list
    page("h.png", [det("text_block")]),
]

# landing official details (TEDS per table, CDM per formula sample)
TEDS = {
    "a.png_[0]": {"TEDS": 0.1},
    "a.png_[1]": {"TEDS": 0.6},
    "b.png_[0]": {"TEDS": 0.9},
    "c.png_[0]": {"TEDS": 0.85},
    "d.png_[0]": {"TEDS": 0.4},
    "l.png_[0]": {"TEDS": 0.85},
}
CDM = {
    "e.png_[0]": 0.2,
    "e.png_[1]": 0.95,
    "f.png_[0]": 0.8,
    "g.png_[0]": 0.3,
    "j.png_[0]": 0.7,
    "j.png_[1]": 0.75,
    "j.png_[2]": 0.8,
    "k.png_[0]": 0.7,
    "k.png_[1]": 0.9,
}
EXPORT = {
    "pages": [
        {"stem": "a", "degenerated": False, "truncated": False},
        {"stem": "b", "degenerated": False, "truncated": False},
        {"stem": "c", "degenerated": False, "truncated": True},
        {"stem": "d", "degenerated": False, "truncated": False},
        {"stem": "l", "degenerated": False, "truncated": False},
        {"stem": "e", "degenerated": False, "truncated": False},
        {"stem": "f", "degenerated": False, "truncated": False},
        {"stem": "g", "degenerated": True, "truncated": False},
        {"stem": "j", "degenerated": False, "truncated": False},
        {"stem": "k", "degenerated": False, "truncated": False},
    ]
}
REGION_TRUTH = {"pages": [{"image_path": f"{n}.png"} for n in "abcdefgjkl"]}


def build(seed=20261114, protected_target=6):
    return b0b.build_b0b_corpus(
        dataset=GT,
        landing_teds=TEDS,
        landing_cdm=CDM,
        export_report=EXPORT,
        region_truth=REGION_TRUTH,
        seed=seed,
        protected_target=protected_target,
    )


result = build()

bands = {member["key"]: band for band, members in result["tables"].items() for member in members}
check("worst band below 0.5 (good page)", bands["a.png_[0]"] == "worst")
check("mid band 0.5..0.85 (good page)", bands["a.png_[1]"] == "mid")
check("good-zh band", bands["b.png_[0]"] == "good-zh")
check("good-en boundary 0.85 inclusive", bands["l.png_[0]"] == "good-en")
check("bad-page table lands in bad-low at any score", bands["c.png_[0]"] == "bad-low")
check("bad page ledger frozen", result["bad_pages"] == ["c", "g"])

fracture = {m["key"] for m in result["formulas"]["fracture"]}
check("fracture keeps good-page CDM<0.5", "e.png_[0]" in fracture)
check("fracture keeps ALL bad-page formulas", "g.png_[0]" in fracture)
check("fracture excludes CDM>=0.5 good formulas", "f.png_[0]" not in fracture)
check(
    "fracture carries landing baselines and origin tags",
    all("baseline" in m and "origin" in m for m in result["formulas"]["fracture"]),
)

protected = [m["key"] for m in result["formulas"]["protected"]]
check("protected set meets the target", len(protected) == 6)
check("protected excludes fracture members", not set(protected) & fracture)
check(
    "protected draws from good CDM>=0.5 pool",
    set(protected) <= {"e.png_[1]", "f.png_[0]", "j.png_[0]", "j.png_[1]", "j.png_[2]", "k.png_[0]", "k.png_[1]"},
)
check(
    "protected set is deterministic",
    [m["key"] for m in build()["formulas"]["protected"]] == protected,
)

check(
    "formula page list covers equation-bearing B0a pages",
    result["formulas"]["pages"] == ["e.png", "f.png", "g.png", "j.png", "k.png"],
)
check(
    "counts recorded",
    result["counts"]["table_samples"] == 6 and result["counts"]["formula_samples"] == 9,
)
check(
    "canonical bytes deterministic",
    b0b.canonical_bytes(build()) == b0b.canonical_bytes(build()),
)

# A member outside the B0a frozen list must fail loudly (页集合复用).
try:
    b0b.build_b0b_corpus(
        dataset=GT + [page("z.png", [det("table")])],
        landing_teds={"z.png_[0]": {"TEDS": 0.1}},
        landing_cdm=CDM,
        export_report=EXPORT,
        region_truth=REGION_TRUTH,
        seed=1,
        protected_target=6,
    )
    print("FAIL member outside B0a list must raise")
    sys.exit(1)
except b0b.B0bError:
    print("ok   member outside B0a list raises")

# An official sample key missing from the GT derivation must fail loudly.
try:
    b0b.build_b0b_corpus(
        dataset=GT,
        landing_teds={**TEDS, "ghost.png_[9]": {"TEDS": 0.1}},
        landing_cdm=CDM,
        export_report=EXPORT,
        region_truth=REGION_TRUTH,
        seed=1,
        protected_target=6,
    )
    print("FAIL official key without GT member must raise")
    sys.exit(1)
except b0b.B0bError:
    print("ok   official key without GT member raises")

# --- CLI end-to-end: freeze + verify + tamper -------------------------------

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    paths = {
        "dataset": GT,
        "landing-teds": TEDS,
        "landing-cdm": CDM,
        "export-report": EXPORT,
        "region-truth": REGION_TRUTH,
    }
    args = [sys.executable, str(Path(__file__).with_name("omnidocbench-freeze-b0b-corpus.py"))]
    for name, payload in paths.items():
        path = root / f"{name}.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        args.extend([f"--{name}", str(path)])
    args.extend(["--protected-target", "6", "--seed", "20261114", "--output-dir", str(root / "b0b")])
    for round_index in range(2):
        proc = subprocess_run(args)
        if proc.returncode != 0:
            print(f"FAIL cli freeze round {round_index}: {proc.stderr}")
            sys.exit(1)
    print("ok   cli freeze runs and is idempotent")
    args_verify = args + ["--verify"]
    proc = subprocess_run(args_verify)
    if proc.returncode != 0:
        print(f"FAIL verify untouched: {proc.stderr}")
        sys.exit(1)
    print("ok   verify passes untouched")
    artifact = root / "b0b" / "b0b-corpus.json"
    artifact.write_bytes(artifact.read_bytes() + b" ")
    proc = subprocess_run(args_verify)
    if proc.returncode == 0:
        print("FAIL tampered artifact must fail verify")
        sys.exit(1)
    print("ok   tampered artifact fails verify")


print("all b0b checks passed")
