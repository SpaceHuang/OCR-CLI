#!/usr/bin/env python3
"""Candidate-C column-split experiment (quality plan Q1.1c/D-10).

For each residual newspaper page (still degenerated after the composed
ladder), detects text columns on the page image via vertical whitespace
valleys (classic projection-profile layout analysis - no model, no prompt
change), and emits one split PDF per source page: N pages, each a vertical
crop band in left-to-right column order. Scanning that PDF (plain path)
and concatenating the per-column markdown in page order reconstructs the
page in reading order.

Outputs (under <benchmark-root>/degeneration-v0.1/column-split/):
  split-pdfs/       one <stem>__colsplit.pdf per source page (N band pages)
  manifest.json     per-page column boundaries + band count + skips
  skips.json        pages where no reliable valley was found (not split)

Usage:
  python scripts/build-column-split.py --benchmark-root benchmarks/omnidocbench
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from PIL import Image
from pypdf import PdfReader, PdfWriter


def detect_columns(image_path: Path, min_cols: int = 2, max_cols: int = 4,
                   min_col_frac: float = 0.14, valley_frac: float = 0.008):
    """Vertical whitespace-valley column detection on a page image.

    Returns column x boundaries in image pixels as [(x0, x1), ...] left to
    right, or None when no reliable valley set exists.
    """
    image = Image.open(image_path).convert("L")
    target_w = 900
    scale = target_w / image.width
    small = image.resize((target_w, max(1, int(image.height * scale))))
    pixels = small.load()
    width, height = small.size
    # Ink profile: fraction of dark pixels per x column (middle 90% rows -
    # drops page furniture like headers/footers spanning all columns).
    row_lo, row_hi = int(height * 0.05), int(height * 0.95)
    ink = [0.0] * width
    for x in range(width):
        dark = 0
        for y in range(row_lo, row_hi, 2):
            if pixels[x, y] < 160:
                dark += 1
        ink[x] = dark / max(1, (row_hi - row_lo) // 2)
    # Smooth the profile.
    smooth = [sum(ink[max(0, i - 3):i + 4]) / len(ink[max(0, i - 3):i + 4]) for i in range(width)]
    in_band_lo, in_band_hi = int(width * 0.08), int(width * 0.92)
    # Valleys: contiguous runs of near-empty profile inside the middle band.
    # Adaptive threshold: gutters sit well below the median text density on
    # noisy JPEG sources; an absolute 0.02 misses them.
    import statistics
    band = smooth[in_band_lo:in_band_hi]
    threshold = max(0.015, statistics.median(band) * 0.45)
    runs = []
    run_start = None
    for x in range(in_band_lo, in_band_hi):
        if smooth[x] <= threshold:
            if run_start is None:
                run_start = x
        else:
            if run_start is not None:
                runs.append((run_start, x))
                run_start = None
    if run_start is not None:
        runs.append((run_start, in_band_hi))
    # Candidate valley centers: interior runs only (not the page margins).
    valleys = [((a + b) // 2, b - a) for a, b in runs[1:-1]] if len(runs) >= 3 else []
    valleys = [v for v in valleys if v[1] >= max(2, int(width * valley_frac))]
    # Greedily keep valleys that leave both neighbors >= min column width.
    cuts: list[int] = []
    for center, _ in valleys:
        bounds = [in_band_lo] + cuts + [in_band_hi]
        if all(center - left >= width * min_col_frac and right - center >= width * min_col_frac
               for left, right in zip(bounds, bounds[1:])):
            cuts.append(center)
    cuts.sort()
    if len(cuts) + 1 < min_cols:
        return None
    bounds = [in_band_lo] + cuts + [in_band_hi]
    return [(bounds[i], bounds[i + 1]) for i in range(len(bounds) - 1)]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark-root", type=Path, default=Path("benchmarks/omnidocbench"))
    parser.add_argument("--residual-list", type=Path, default=None,
                        help="file with one stem per line (default: degeneration-v0.1/residual-newspaper.txt)")
    args = parser.parse_args()
    root = args.benchmark_root
    residual_list = args.residual_list or (root / "degeneration-v0.1/residual-newspaper.txt")
    stems = [s.strip() for s in residual_list.read_text(encoding="utf-8").splitlines() if s.strip()]

    mapping = json.loads((root / "pdfs" / "mapping.json").read_text(encoding="utf-8"))
    stem2page = {p["stem"]: p for p in mapping["pages"]}
    out = root / "degeneration-v0.1" / "column-split"
    split_dir = out / "split-pdfs"
    split_dir.mkdir(parents=True, exist_ok=True)

    manifest = {"schema_version": 1, "pages": [], "skips": []}
    for stem in stems:
        page = stem2page.get(stem)
        if page is None:
            manifest["skips"].append({"stem": stem, "reason": "missing mapping"})
            continue
        image_path = root / page["source"].replace("\\", "/")
        columns = detect_columns(image_path)
        if columns is None:
            manifest["skips"].append({"stem": stem, "reason": "no reliable valley"})
            continue
        pdf_path = root / page["pdf"].replace("\\", "/")
        reader = PdfReader(str(pdf_path))
        if len(reader.pages) != 1:
            manifest["skips"].append({"stem": stem, "reason": f"pages={len(reader.pages)}"})
            continue
        base = reader.pages[0]
        media = base.mediabox
        px_w = float(page["width"])
        pt_per_px = float(media.width) / px_w
        writer = PdfWriter()
        bands = []
        for x0_px, x1_px in columns:
            new_page = writer.add_blank_page(width=int(media.width), height=int(media.height))
            # Merge the source page content onto the band via crop: pypdf
            # cannot paste pages, so instead crop the ORIGINAL page object.
            # (add_blank_page would be empty - use a cropped copy instead.)
            writer.remove_page(writer.pages[-1])
            band = PdfReader(str(pdf_path)).pages[0]
            band.cropbox.lower_left = (x0_px * pt_per_px, 0.0)
            band.cropbox.upper_right = (x1_px * pt_per_px, float(media.height))
            writer.add_page(band)
            bands.append({"x0_px": x0_px, "x1_px": x1_px})
        out_pdf = split_dir / f"{stem}__colsplit.pdf"
        with out_pdf.open("wb") as fh:
            writer.write(fh)
        manifest["pages"].append({"stem": stem, "columns": len(columns), "bands": bands,
                                  "split_pdf": str(out_pdf)})
    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"split: {len(manifest['pages'])} pages  skipped: {len(manifest['skips'])}")
    for skip in manifest["skips"][:6]:
        print("  skip:", skip["stem"][:50], "-", skip["reason"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
