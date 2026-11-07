#!/usr/bin/env python3
"""Builds the frozen 213-page degeneration corpus (quality plan Q1.0).

Reads the balanced-profile baseline scan checkpoints, cross-checks the bad-page
ledger against the frozen export report, joins stems through
``pdfs/mapping.json`` to the OmniDocBench ground truth (for page attributes),
and emits:

Committed side (``--asset-dir``, default
``assets/regression/omnidocbench-degeneration-v0.1``):
  - ``pages.json``            frozen corpus page list (stem, scan dir, source
                              category, language, layout, flags, attempts,
                              warnings, render audit)
  - ``mapping-subset.json``   ``pdfs/mapping.json`` subset for the corpus
                              stems (drives the subset prediction export)
  - ``render-audit-join.json`` one-shot render-downsampling join (plan Q1.0)
  - ``manifest.json``         sha256 manifest of the emitted files
  - ``README.md``             purpose + one-command reproduction

Bulk side (``<benchmark-root>/degeneration-v0.1/``, gitignored like the rest
of the benchmark tree because it contains dataset ground-truth content):
  - ``truth-subset.json``     OmniDocBench ground-truth entries for the corpus
                              pages, directly scorable by the official
                              ``pdf_validation.py`` through
                              ``scripts/run-omnidocbench-eval.py``
  - ``predictions/``          subset prediction export (via
                              ``scripts/export-omnidocbench-predictions.py``)
  - ``eval-subset.sh``        one-command subset scoring round

The corpus is the degenerated-page set only (213 pages incl. 5 truncation
intersections). Pure truncated pages (28) are excluded per plan BLK-2; the
missing page goes to Q4. Every checkpoint-derived ledger number is
cross-checked against the frozen export report and the build fails loudly on
any mismatch.

Usage:
  python scripts/build-degeneration-corpus.py \
      --benchmark-root benchmarks/omnidocbench [--profile balanced]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

EXPECTED = {
    "degenerated": 213,
    "truncated_total": 33,
    "degenerated_truncated_intersection": 5,
    "pure_truncated": 28,
    "missing": 1,
}

RENDER_BUDGET_PIXELS = 2880 * 2880


def decode(value: str) -> str:
    """Checkpoint values are hex-encoded text; plain tokens pass through."""
    if len(value) % 2 == 0:
        try:
            return bytes.fromhex(value).decode("utf-8", "strict")
        except ValueError:
            return value
    return value


def read_checkpoint(path: Path) -> dict:
    fields: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, _, value = line.partition("=")
        fields[key] = decode(value)
    return fields


def parse_audit(audit: str) -> dict:
    parsed: dict[str, str] = {}
    for part in audit.split(";"):
        key, _, value = part.partition("=")
        parsed[key] = value
    result: dict = {"scaled": parsed.get("scaled") == "true", "ratio": parsed.get("ratio")}
    for key in ("original", "rendered", "model"):
        if key in parsed and "x" in parsed[key]:
            width, _, height = parsed[key].partition("x")
            result[key] = {"width": int(width), "height": int(height)}
    return result


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark-root", type=Path, default=Path("benchmarks/omnidocbench"))
    parser.add_argument("--profile", default="balanced")
    parser.add_argument(
        "--asset-dir", type=Path, default=Path("assets/regression/omnidocbench-degeneration-v0.1")
    )
    parser.add_argument("--skip-export", action="store_true", help="skip prediction export")
    args = parser.parse_args()

    root = args.benchmark_root
    scans = root / "scans" / args.profile

    # 1. Checkpoint ledger.
    pages: list[dict] = []
    seen_stems: set[str] = set()
    for scan_dir in sorted(scans.iterdir()):
        if not scan_dir.is_dir():
            continue
        stem = scan_dir.name.rsplit("-", 1)[0]
        if stem in seen_stems:
            print(f"ERROR: duplicate scan stem: {stem}", file=sys.stderr)
            return 2
        seen_stems.add(stem)
        checkpoint_path = scan_dir / "page-0001.checkpoint"
        if not checkpoint_path.is_file():
            continue  # recognition_failed page (no checkpoint); goes to Q4
        fields = read_checkpoint(checkpoint_path)
        # write_checkpoint joins warnings with "\n" before hex encoding.
        warnings = [w for w in fields.get("warnings", "").split("\n") if w]
        entry = {
            "stem": stem,
            "scan_dir": scan_dir.name,
            "complete": fields.get("complete") == "true",
            "degenerated": fields.get("degenerated") == "true",
            "truncated": fields.get("truncated") == "true",
            "attempts": int(fields.get("attempts", "1")),
            "warnings": warnings,
            "render": parse_audit(fields.get("audit", "")),
        }
        pages.append(entry)

    degenerated = {p["stem"] for p in pages if p["degenerated"]}
    truncated = {p["stem"] for p in pages if p["truncated"]}
    missing = []  # filled from the export report below

    # 2. Cross-check against the frozen export report.
    export = json.loads((root / "predictions" / args.profile / "export-report.json").read_text(encoding="utf-8"))
    export_deg = {e["stem"] for e in export["anomalies"] if e.get("degenerated")}
    export_tru = {e["stem"] for e in export["anomalies"] if e.get("truncated")}
    missing = sorted(set(export.get("missing", [])))
    checks = {
        "degenerated": (len(degenerated), EXPECTED["degenerated"]),
        "truncated_total": (len(truncated), EXPECTED["truncated_total"]),
        "degenerated_truncated_intersection": (
            len(degenerated & truncated),
            EXPECTED["degenerated_truncated_intersection"],
        ),
        "pure_truncated": (len(truncated - degenerated), EXPECTED["pure_truncated"]),
        "missing": (len(missing), EXPECTED["missing"]),
    }
    mismatched = {k: v for k, v in checks.items() if v[0] != v[1]}
    if degenerated != export_deg:
        mismatched["checkpoint_vs_export_degenerated"] = (len(degenerated ^ export_deg), 0)
    if truncated != export_tru:
        mismatched["checkpoint_vs_export_truncated"] = (len(truncated ^ export_tru), 0)
    if mismatched:
        print(f"ERROR: ledger mismatch vs frozen export report: {mismatched}", file=sys.stderr)
        return 2

    corpus = [p for p in pages if p["degenerated"]]

    # 3. Join stems to mapping + dataset page attributes.
    mapping = json.loads((root / "pdfs" / "mapping.json").read_text(encoding="utf-8"))
    stem2page = {p["stem"]: p for p in mapping["pages"]}
    unmapped = [p["stem"] for p in corpus if p["stem"] not in stem2page]
    if unmapped:
        print(f"ERROR: corpus stems missing from mapping.json: {unmapped[:5]}", file=sys.stderr)
        return 2

    dataset = json.loads((root / "dataset" / "OmniDocBench.json").read_text(encoding="utf-8"))
    img2attr: dict[str, dict] = {}
    for entry in dataset:
        info = entry.get("page_info", {})
        attribute = info.get("page_attribute", {})
        img2attr[info.get("image_path", "")] = {
            "source_category": attribute.get("data_source"),
            "language": attribute.get("language"),
            "layout": attribute.get("layout"),
        }

    for entry in corpus:
        mapped = stem2page[entry["stem"]]
        entry["image_path"] = mapped["image_path"]
        entry["pdf"] = mapped["pdf"]
        entry.update(img2attr.get(mapped["image_path"], {}))

    # 4. Render-audit join (plan Q1.0 one-shot analysis). A page is
    # "over-budget" when the 300-dpi render request (audit `original` page
    # box scaled to pixels) exceeds the frozen 2880x2880 PDFium pixel
    # budget, i.e. PDFium proportionally scaled the render down
    # (`rendered` < requested). Baseline scope: 171 checkpointed pages +
    # the missing page = the plan's 172.
    render_dpi = 300

    def requested_pixels(render: dict) -> int:
        original = render.get("original")
        if not original:
            return 0
        scale = render_dpi / 72.0
        return int(original["width"] * scale) * int(original["height"] * scale)

    def over_budget(render: dict) -> bool:
        return requested_pixels(render) > RENDER_BUDGET_PIXELS

    corpus_over = sum(1 for p in corpus if over_budget(p["render"]))
    all_over = sum(1 for p in pages if over_budget(p["render"]))
    render_join = {
        "schema_version": 1,
        "render_budget_pixels": RENDER_BUDGET_PIXELS,
        "render_dpi": render_dpi,
        "all_pages": {"total": len(pages), "scaled_or_over_budget": all_over},
        "corpus": {
            "total": len(corpus),
            "over_budget_render": corpus_over,
            "share_pct": round(100.0 * corpus_over / max(1, len(corpus)), 1),
            "baseline_all_pages_share_pct": round(100.0 * all_over / max(1, len(pages)), 1),
        },
        "by_source_category": {},
    }
    for entry in corpus:
        key = entry.get("source_category") or "unknown"
        bucket = render_join["by_source_category"].setdefault(
            key, {"pages": 0, "over_budget_render": 0}
        )
        bucket["pages"] += 1
        bucket["over_budget_render"] += 1 if over_budget(entry["render"]) else 0

    # 5. Emit committed side.
    asset_dir = args.asset_dir
    asset_dir.mkdir(parents=True, exist_ok=True)
    corpus_pages = {p["stem"] for p in corpus}

    pages_doc = {
        "schema_version": 1,
        "generator": "scripts/build-degeneration-corpus.py",
        "source_profile": args.profile,
        "source_benchmark_manifest": "benchmarks/omnidocbench/manifest.json",
        "purpose": "Frozen 213-page degeneration corpus for the escape-ladder experiments (quality plan Q1.0).",
        "counts": {k: v[0] for k, v in checks.items()},
        "pages": corpus,
    }
    (asset_dir / "pages.json").write_text(
        json.dumps(pages_doc, ensure_ascii=False, indent=1), encoding="utf-8"
    )

    mapping_subset = {k: v for k, v in mapping.items() if k != "pages"}
    mapping_subset["pages"] = [
        p for p in mapping["pages"] if p["stem"] in corpus_pages
    ]
    mapping_subset["derived_from"] = "pdfs/mapping.json"
    mapping_subset["purpose"] = "Degeneration corpus subset mapping (stems, images, PDFs)."
    (asset_dir / "mapping-subset.json").write_text(
        json.dumps(mapping_subset, ensure_ascii=False, indent=1), encoding="utf-8"
    )
    (asset_dir / "render-audit-join.json").write_text(
        json.dumps(render_join, ensure_ascii=False, indent=1), encoding="utf-8"
    )

    # 6. Emit bulk side (gitignored): truth subset + eval script + predictions.
    bulk = root / "degeneration-v0.1"
    (bulk / "predictions").mkdir(parents=True, exist_ok=True)
    corpus_imgs = {stem2page[s]["image_path"] for s in corpus_pages}
    truth_subset = [e for e in dataset if e.get("page_info", {}).get("image_path") in corpus_imgs]
    (bulk / "truth-subset.json").write_text(
        json.dumps(truth_subset, ensure_ascii=False, indent=1), encoding="utf-8"
    )
    eval_script = f"""#!/bin/bash
# One-command subset scoring round (quality plan Q1.0 fast eval loop).
set -u
cd "$(dirname "$0")/../../.." || exit 2
PY=benchmarks/omnidocbench/.venv/Scripts/python.exe
"$PY" scripts/run-omnidocbench-eval.py --benchmark-root benchmarks/omnidocbench \\
  --gt benchmarks/omnidocbench/degeneration-v0.1/truth-subset.json \\
  --predictions benchmarks/omnidocbench/degeneration-v0.1/predictions \\
  --name degeneration-v0.1 --workers 8 || exit 1
echo SUBSET-EVAL-DONE
"""
    (bulk / "eval-subset.sh").write_text(eval_script, encoding="utf-8", newline="\n")

    if not args.skip_export:
        # Subset export: every scan stem outside the corpus mapping is
        # expected, so pass the full scan stem list as the extra-forgiveness
        # filter instead of duplicating the export's strip logic here.
        stems_filter = bulk / "stems-all.txt"
        stems_filter.write_text("\n".join(sorted(seen_stems)) + "\n", encoding="utf-8")
        export_cmd = [
            sys.executable,
            str(Path(__file__).resolve().parent / "export-omnidocbench-predictions.py"),
            "--scans", str(scans),
            "--mapping", str(asset_dir / "mapping-subset.json"),
            "--out", str(bulk / "predictions"),
            "--stems-filter", str(stems_filter),
        ]
        completed = subprocess.run(export_cmd)
        if completed.returncode != 0:
            print("ERROR: subset prediction export failed", file=sys.stderr)
            return 2

    # 7. Manifest (sha256 of committed artifacts).
    manifest = {
        "schema_version": 1,
        "generator": "scripts/build-degeneration-corpus.py",
        "source_profile": args.profile,
        "counts": {k: v[0] for k, v in checks.items()},
        "files": {
            name: sha256(asset_dir / name)
            for name in ("pages.json", "mapping-subset.json", "render-audit-join.json")
        },
    }
    (asset_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1), encoding="utf-8"
    )

    readme = f"""# omnidocbench-degeneration-v0.1

Frozen 213-page degeneration corpus (quality plan Q1.0): every balanced-profile
baseline page whose checkpoint carries `degenerated=true`, including the 5
degeneration-and-truncation intersection pages. Pure truncated pages (28) and
the missing page are excluded (plan BLK-2 / Q4).

## Contents

- `pages.json` - frozen page list (stem, scan dir, source category, language,
  layout, degenerated/truncated flags, attempts, warnings, render audit).
- `mapping-subset.json` - `pdfs/mapping.json` subset for the corpus stems.
- `render-audit-join.json` - one-shot render-downsampling join (plan Q1.0).
- `manifest.json` - sha256 manifest of the above.

The bulk side of the corpus (ground-truth subset and prediction export) stays
out of git like the rest of the benchmark data because the OmniDocBench
dataset is research-use-only; regenerate it with one command:

```bash
python scripts/build-degeneration-corpus.py --benchmark-root benchmarks/omnidocbench
```

That emits `benchmarks/omnidocbench/degeneration-v0.1/` with
`truth-subset.json` (scorable by the official `pdf_validation.py` through
`scripts/run-omnidocbench-eval.py`) and `predictions/` (subset prediction
export), plus `eval-subset.sh` for one scoring round (~11.6 min equivalent).

## Reproduction checks

The build fails loudly unless the checkpoint ledger matches the frozen export
report exactly: 213 degenerated (5 truncation intersections), 33 truncated,
28 pure truncated, 1 missing. Baseline numbers verified 2026-11-07 against
`scripts/omnidocbench-projection.py` (77.9909 / 84.5674 / 91.1439).
"""
    (asset_dir / "README.md").write_text(readme, encoding="utf-8", newline="\n")

    print(f"corpus: {len(corpus)} degenerated pages -> {asset_dir}")
    print(f"render-audit join: corpus over-budget {corpus_over}/{len(corpus)} "
          f"vs all pages {all_over}/{len(pages)}")
    print(f"bulk side: {bulk} (truth n={len(truth_subset)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
