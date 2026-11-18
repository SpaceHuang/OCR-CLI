#!/usr/bin/env python3
"""T-PROJ (BLK-4/D-31) unit checks: the paired-increment projection mode.

Hand-verified cases (values computed by hand, frozen here — the next-phase
plan §4.0 acceptance "三个手工核对待验证样例"):
  M2-class page-level: one improved page + one unchanged page out of three.
  M4-class per-table:  +0.4/−0.1 paired deltas over a 4-table official set.
  M5-class per-sample: one CDM rescue over a 3-sample official set.
"""
import importlib.util
import json
import math
import subprocess
import sys
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "paired_projection", Path(__file__).with_name("omnidocbench-paired-projection.py")
)
pp = importlib.util.module_from_spec(spec)
sys.modules["paired_projection"] = pp
spec.loader.exec_module(pp)


def check(name, condition):
    if not condition:
        print(f"FAIL {name}")
        sys.exit(1)
    print(f"ok   {name}")


def close(name, actual, expected, tol=1e-9):
    if not math.isclose(actual, expected, rel_tol=0, abs_tol=tol):
        print(f"FAIL {name}: got {actual!r}, want {expected!r}")
        sys.exit(1)
    print(f"ok   {name}")


def ted(value):
    """Official TEDS detail shape: {'TEDS': x, 'TEDS_structure_only': y}."""
    return {"TEDS": value, "TEDS_structure_only": value}


# --- Case M2 (page-level text) ---------------------------------------------

OFFICIAL_TEXT = {"a.png": 0.2, "b.png": 0.3, "c.png": 0.4}
BASE_TEXT = {"a.png": 0.2, "b.png": 0.3}
CAND_TEXT = {"a.png": 0.1, "b.png": 0.3}
OFFICIAL_CDM = {"p.png_[0]": 0.5, "q.png_[0]": 1.0}
OFFICIAL_TEDS = {"t.png_[0]": ted(0.5), "u.png_[0]": ted(1.0)}
BASE_EMPTY, CAND_EMPTY = {}, {}

POP_M2 = {"schema_version": 1, "population_id": "m2-case", "text_pages": ["a.png", "b.png"]}


def project_m2():
    return pp.project(
        official_text=OFFICIAL_TEXT,
        official_cdm=OFFICIAL_CDM,
        official_teds=OFFICIAL_TEDS,
        baseline_text=BASE_TEXT,
        baseline_cdm=BASE_EMPTY,
        baseline_teds=BASE_EMPTY,
        candidate_text=CAND_TEXT,
        candidate_cdm=CAND_EMPTY,
        candidate_teds=CAND_EMPTY,
        population=POP_M2,
    )


result = project_m2()
close("M2 paired mean delta edit", result["components"]["text"]["paired_mean_delta"], -0.05)
close("M2 projected edit mean", result["components"]["text"]["projected_all_mean"], 0.8 / 3)
close("M2 projected text score", result["components"]["text"]["projected_component"], 1 - 0.8 / 3)
close("M2 delta text score", result["components"]["text"]["delta_component"], (1 - 0.8 / 3) - 0.7)
close("M2 official text score", result["components"]["text"]["official_component"], 0.7)
close("M2 official n", result["components"]["text"]["official_n"], 3)
close("M2 population n", result["components"]["text"]["population_n"], 2)
# untouched components project to themselves
close("M2 cdm passthrough", result["components"]["cdm"]["projected_component"], 0.75)
close("M2 cdm delta", result["components"]["cdm"]["delta_component"], 0.0)
close("M2 teds passthrough", result["components"]["teds"]["projected_component"], 0.75)
# baseline overall = (0.7 + 0.75 + 0.75)/3*100
close("M2 official overall", result["overall"]["official"], (0.7 + 0.75 + 0.75) / 3 * 100)
# projected overall = ((1-0.8/3) + 0.75 + 0.75)/3*100
close(
    "M2 projected overall",
    result["overall"]["projected"],
    ((1 - 0.8 / 3) + 0.75 + 0.75) / 3 * 100,
)
close(
    "M2 delta overall = delta_text/3*100",
    result["overall"]["delta"],
    ((1 - 0.8 / 3) - 0.7) / 3 * 100,
)

# --- Case M4 (per-table TEDS) ----------------------------------------------

OFFICIAL_TEDS_M4 = {"t1_[0]": ted(0.5), "t2_[0]": ted(0.6), "t3_[0]": ted(0.7), "t4_[0]": ted(0.8)}
POP_M4 = {"schema_version": 1, "population_id": "m4-case", "tables": ["t1_[0]", "t3_[0]"]}

result = pp.project(
    official_text={"a.png": 0.0},
    official_cdm=OFFICIAL_CDM,
    official_teds=OFFICIAL_TEDS_M4,
    baseline_text={"a.png": 0.0},
    baseline_cdm=BASE_EMPTY,
    baseline_teds={"t1_[0]": ted(0.5), "t3_[0]": ted(0.7)},
    candidate_text={"a.png": 0.0},
    candidate_cdm=CAND_EMPTY,
    candidate_teds={"t1_[0]": ted(0.9), "t3_[0]": ted(0.6)},
    population=POP_M4,
)
close("M4 paired mean delta TEDS", result["components"]["teds"]["paired_mean_delta"], 0.15)
close("M4 projected TEDS mean", result["components"]["teds"]["projected_component"], 0.725)
close("M4 delta TEDS", result["components"]["teds"]["delta_component"], 0.075)
close("M4 delta overall = 0.075/3*100", result["overall"]["delta"], 0.075 / 3 * 100)

# --- Case M5 (per-sample CDM) ----------------------------------------------

POP_M5 = {"schema_version": 1, "population_id": "m5-case", "formulas": ["c1_[0]"]}
result = pp.project(
    official_text={"a.png": 0.0},
    official_cdm={"c1_[0]": 0.2, "c2_[0]": 0.9, "c3_[0]": 0.9},
    official_teds=OFFICIAL_TEDS,
    baseline_text={"a.png": 0.0},
    baseline_cdm={"c1_[0]": 0.2},
    baseline_teds=BASE_EMPTY,
    candidate_text={"a.png": 0.0},
    candidate_cdm={"c1_[0]": 1.0},
    candidate_teds=CAND_EMPTY,
    population=POP_M5,
)
close("M5 paired mean delta CDM", result["components"]["cdm"]["paired_mean_delta"], 0.8)
close(
    "M5 projected CDM mean",
    result["components"]["cdm"]["projected_component"],
    (0.2 + 0.9 + 0.9 + 0.8) / 3,
)
close(
    "M5 delta overall = +0.8/3/3*100",
    result["overall"]["delta"],
    (0.8 / 3) / 3 * 100,
)

# --- Error paths ------------------------------------------------------------

try:
    pp.project(
        official_text=OFFICIAL_TEXT,
        official_cdm=OFFICIAL_CDM,
        official_teds=OFFICIAL_TEDS,
        baseline_text={"zzz.png": 0.1},
        baseline_cdm=BASE_EMPTY,
        baseline_teds=BASE_EMPTY,
        candidate_text=CAND_TEXT,
        candidate_cdm=CAND_EMPTY,
        candidate_teds=CAND_EMPTY,
        population=POP_M2,
    )
    print("FAIL baseline key not in official details must raise")
    sys.exit(1)
except pp.ProjectionError:
    print("ok   baseline key outside official details raises")

try:
    pp.project(
        official_text=OFFICIAL_TEXT,
        official_cdm=OFFICIAL_CDM,
        official_teds=OFFICIAL_TEDS,
        baseline_text=BASE_TEXT,
        baseline_cdm=BASE_EMPTY,
        baseline_teds=BASE_EMPTY,
        candidate_text={"a.png": 0.1},
        candidate_cdm=CAND_EMPTY,
        candidate_teds=CAND_EMPTY,
        population=POP_M2,
    )
    print("FAIL missing candidate key without allow flag must raise")
    sys.exit(1)
except pp.ProjectionError:
    print("ok   missing candidate key raises by default")

result = pp.project(
    official_text=OFFICIAL_TEXT,
    official_cdm=OFFICIAL_CDM,
    official_teds=OFFICIAL_TEDS,
    baseline_text=BASE_TEXT,
    baseline_cdm=BASE_EMPTY,
    baseline_teds=BASE_EMPTY,
    candidate_text={"a.png": 0.1},
    candidate_cdm=CAND_EMPTY,
    candidate_teds=CAND_EMPTY,
    population=POP_M2,
    allow_missing_candidate=True,
)
close(
    "allow-missing counts and treats gap as no-change",
    result["components"]["text"]["missing_candidate_n"],
    1,
)
close(
    "allow-missing projected edit mean",
    result["components"]["text"]["projected_all_mean"],
    0.8 / 3,
)

# --- CLI end-to-end on a synthetic tree -------------------------------------

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    (root / "reports").mkdir()
    files = {
        "official-text": OFFICIAL_TEXT,
        "official-cdm": OFFICIAL_CDM,
        "official-teds": OFFICIAL_TEDS,
        "baseline-text": BASE_TEXT,
        "baseline-cdm": {},
        "baseline-teds": {},
        "candidate-text": CAND_TEXT,
        "candidate-cdm": {},
        "candidate-teds": {},
    }
    args = [
        sys.executable,
        str(Path(__file__).with_name("omnidocbench-paired-projection.py")),
        "--population",
        str(root / "population.json"),
        "--output",
        str(root / "projection.json"),
    ]
    (root / "population.json").write_text(json.dumps(POP_M2), encoding="utf-8")
    for name, payload in files.items():
        path = root / f"{name}.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        args.extend([f"--{name.replace('-', '-')}", str(path)])
    proc = subprocess.run(args, capture_output=True, text=True, timeout=60)
    if proc.returncode != 0:
        print(f"FAIL cli run: {proc.stderr}")
        sys.exit(1)
    print("ok   cli run exits 0")
    out = json.loads((root / "projection.json").read_text(encoding="utf-8"))
    if out["overall"]["delta"] != result["overall"]["delta"]:
        print("FAIL cli output differs from library result")
        sys.exit(1)
    print("ok   cli output matches library result")

print("all paired-projection checks passed")
