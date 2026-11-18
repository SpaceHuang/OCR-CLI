#!/usr/bin/env python3
"""T-PROJ acceptance ①: the legacy θ-scenario projection stays byte-for-byte
reproducible (next-phase plan §4.0, D-31). The golden output below was
captured from `scripts/omnidocbench-projection.py` on the synthetic
benchmark-root fixture this test rebuilds; any change to the legacy
projection's stdout fails this lock.

The paired-increment mode lives in omnidocbench-paired-projection.py
(test_omnidocbench_paired_projection.py); the legacy script is untouched.
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

GOLDEN = """\
profile=balanced  text n=4  cdm n=2  teds n=2
官方分量：text(1-edit)=0.7500  CDM(all)=0.7500  TEDS(all)=0.7500
当前 Overall = 75.0000
坏页账本：|D|=1 |T|=0 |D∩T|=0 纯T=0 缺失=0

[Q1 语料（退化页 1 页，含 0 交集）] 文本坏页=1 公式坏样本=0 表格坏样本=0
  正常均值：text=0.8333 CDM=0.7500 TEDS=0.7500  可修复缺口=2.7778
  θ=0.00 → Overall 75.0000  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=0.25 → Overall 75.6944  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=0.50 → Overall 76.3889  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=0.75 → Overall 77.0833  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=1.00 → Overall 77.7778

[全部坏页（1 页）+ 缺失 0 页] 文本坏页=1 公式坏样本=0 表格坏样本=0
  正常均值：text=0.8333 CDM=0.7500 TEDS=0.7500  可修复缺口=2.7778
  θ=0.00 → Overall 75.0000  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=0.25 → Overall 75.6944  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=0.50 → Overall 76.3889  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=0.75 → Overall 77.0833  （Overall 88 需 θ=1.00，85 需 θ=1.00）
  θ=1.00 → Overall 77.7778
"""


def build_fixture(root: Path) -> None:
    (root / "predictions" / "balanced").mkdir(parents=True)
    (root / "pdfs").mkdir()
    (root / "reports").mkdir()
    (root / "predictions" / "balanced" / "export-report.json").write_text(
        json.dumps(
            {
                "anomalies": [
                    {"stem": "p1.png", "degenerated": True, "truncated": False}
                ],
                "missing": [],
            }
        ),
        encoding="utf-8",
    )
    (root / "pdfs" / "mapping.json").write_text(
        json.dumps(
            {
                "pages": [
                    {"stem": stem, "image_path": stem}
                    for stem in ("p1.png", "p2.png", "p3.png", "p4.png")
                ]
            }
        ),
        encoding="utf-8",
    )
    reports = root / "reports"
    (
        reports / "omnidocbench-balanced-all_quick_match_text_block_per_page_edit.json"
    ).write_text(
        json.dumps({"p1.png": 0.5, "p2.png": 0.0, "p3.png": 0.25, "p4.png": 0.25}),
        encoding="utf-8",
    )
    (
        reports
        / "omnidocbench-balanced-all_quick_match_display_formula_per_sample_CDM.json"
    ).write_text(json.dumps({"c1.png_[0]": 0.5, "c2.png_[0]": 1.0}), encoding="utf-8")
    (
        reports / "omnidocbench-balanced-all_quick_match_table_per_table_TEDS.json"
    ).write_text(
        json.dumps(
            {
                "t1.png_[0]": {"TEDS": 0.5, "TEDS_structure_only": 0.0},
                "t2.png_[0]": {"TEDS": 1.0, "TEDS_structure_only": 1.0},
            }
        ),
        encoding="utf-8",
    )


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        build_fixture(root)
        proc = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("omnidocbench-projection.py")),
                "--benchmark-root",
                str(root),
            ],
            capture_output=True,
            text=True,
            timeout=60,
        )
    if proc.returncode != 0:
        print(f"FAIL legacy projection exited {proc.returncode}: {proc.stderr}")
        sys.exit(1)
    actual = proc.stdout.replace("\r\n", "\n")
    if actual != GOLDEN:
        print("FAIL legacy θ-scenario output drifted from the frozen golden")
        for index, (want, got) in enumerate(zip(GOLDEN.splitlines(), actual.splitlines())):
            if want != got:
                print(f"  first diff line {index}: want {want!r} got {got!r}")
        sys.exit(1)
    print("ok   legacy θ-scenario projection is byte-for-byte reproducible")


if __name__ == "__main__":
    main()
