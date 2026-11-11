# omnidocbench-degeneration-v0.1

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
