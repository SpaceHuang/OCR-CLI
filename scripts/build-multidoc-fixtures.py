#!/usr/bin/env python3
"""Build the phase-2 multi-document fixtures (plan T0.3).

Two scenarios land under assets/regression/multidoc-v1/ (untracked, rebuilt
on demand — same distribution rule as the golden set: never commit fixture
bytes, always the rebuild script plus a recorded manifest):

- ``md-small-20``: 20 documents x 2 image-only pages each. The X1 gate
  scenario (window under-fill + per-call fixed tax dominate).
- ``md-mixed``: 8 documents of 1/2/4/8 pages (two of each). The X2 mixed
  composition spike scenario.

Every page is a rasterized text page (no text layer), so routing always
falls to OCR — the multi-document gains under test are OCR-window gains,
not native-text routing. Content is deterministic per (doc, page); the
script writes manifest.json with each file's sha256 so a measurement
report can pin exactly what it ran on.

Usage: python scripts/build-multidoc-fixtures.py [--force]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

import pymupdf

REPO_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_VERSION = 1

# Deterministic body text: the same paragraph rotated per page index so every
# page differs (degenerate-copy detection would fire on identical pages) while
# the fixtures stay reproducible byte-for-byte.
BODY = (
    "Fixture page for the multi-document throughput scenario. "
    "Paragraph {n}: the OCR pipeline under test rasterizes this text into an "
    "image, renders it at 300 DPI through PDFium, and decodes it through the "
    "windowed inference actor. Content is filler; determinism is the point. "
    "Line of measured words for the repetition detector to chew on, {n} of {total}."
)

HEADER = "multidoc-v1 doc {doc:02d} page {page:02d}"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_page_pdf(path: Path, doc_index: int, page_count: int) -> None:
    """One raster-only PDF of `page_count` pages for `doc_index` (0-based)."""
    document = pymupdf.open()
    document.set_metadata(
        {
            "title": f"multidoc-v1 fixture {doc_index:02d}",
            "author": "ocrx phase-2 fixtures",
            "producer": "ocrx-multidoc-fixture",
            "creationDate": "D:20261103000000Z",
            "modDate": "D:20261103000000Z",
        }
    )
    for page_index in range(page_count):
        # Draft the page as text, rasterize it, then embed only the pixmap:
        # the final page has no text layer, so routing must pick OCR.
        draft = pymupdf.open()
        draft_page = draft.new_page(width=595, height=842)  # A4 portrait, pt
        draft_page.insert_text(
            (72, 96),
            HEADER.format(doc=doc_index, page=page_index),
            fontsize=18,
        )
        for paragraph in range(6):
            draft_page.insert_text(
                (72, 160 + paragraph * 72),
                BODY.format(n=paragraph, total=6),
                fontsize=11,
            )
        pixmap = draft_page.get_pixmap(dpi=144)
        draft.close()
        page = document.new_page(width=595, height=842)
        page.insert_image(page.rect, pixmap=pixmap)
    document.save(path, deflate=True)
    document.close()


def build_scenario(root: Path, name: str, page_counts: list[int], force: bool) -> dict:
    scenario_dir = root / name
    if scenario_dir.exists() and not force:
        print(f"refusing to overwrite {scenario_dir} (pass --force)", file=sys.stderr)
        raise SystemExit(2)
    scenario_dir.mkdir(parents=True, exist_ok=True)
    entries = []
    for doc_index, page_count in enumerate(page_counts):
        doc_name = f"doc-{doc_index:02d}-{page_count}p.pdf"
        doc_path = scenario_dir / doc_name
        write_page_pdf(doc_path, doc_index, page_count)
        entries.append(
            {
                "file": doc_name,
                "pages": page_count,
                "sha256": sha256(doc_path),
            }
        )
        print(f"wrote {doc_path} ({page_count} pages)")
    return {"scenario": name, "documents": entries}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--force", action="store_true", help="overwrite existing fixtures")
    args = parser.parse_args()

    root = REPO_ROOT / "assets" / "regression" / "multidoc-v1"
    scenarios = [
        build_scenario(root, "md-small-20", [2] * 20, args.force),
        build_scenario(root, "md-mixed", [1, 1, 2, 2, 4, 4, 8, 8], args.force),
    ]
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "generator": "scripts/build-multidoc-fixtures.py",
        "scenarios": scenarios,
    }
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {manifest_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
