#!/usr/bin/env python3
"""Collect ocrx scan artifacts into gate-compatible PageDocument predictions.

Gate contract (docs/test/p0-30-page-truth-quality-gate.md §4.2): every
prediction page must be a PageDocument JSON aligned with the truth set.
The ocrx CLI writes full PageDocument JSON for OCR pages, but native-text
and empty pages carry their text only in `page-NNNN.md` (the routed JSON
holds just the classification). This collector bridges the two:

- OCR pages with blocks in the native JSON are copied through;
- native/empty pages are wrapped as a single markdown block from the
  per-page markdown, with termination taken from the checkpoint
  (`truncated` flag) so truncation stays measurable;
- a missing artifact for a selected page is an error, not a blank.

Usage: collect-gate-predictions.py SCAN_DIR OUTPUT_DIR --page-count N
"""
import argparse
import json
from pathlib import Path


def read_termination(scan_dir: Path, page: int) -> str:
    checkpoint = scan_dir / f"page-{page:04}.checkpoint"
    if checkpoint.is_file() and "truncated=true" in checkpoint.read_text(encoding="utf-8"):
        return "max_tokens"
    return "eos"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scan_dir", type=Path)
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("--page-count", type=int, required=True)
    args = parser.parse_args()

    args.output_dir.mkdir(parents=True, exist_ok=True)
    for page in range(1, args.page_count + 1):
        native_json = args.scan_dir / f"page-{page:04}.json"
        markdown = args.scan_dir / f"page-{page:04}.md"
        if not native_json.is_file() or not markdown.is_file():
            raise SystemExit(f"missing scan artifacts for page {page}")
        blocks = json.loads(native_json.read_text(encoding="utf-8")).get("blocks") or []
        content = markdown.read_text(encoding="utf-8")
        if not blocks:
            if not content and "truncated=false" in (
                args.scan_dir / f"page-{page:04}.checkpoint"
            ).read_text(encoding="utf-8"):
                # Empty pages legitimately produce no markdown and no blocks.
                pass
            blocks = [{"type": "markdown", "content": content}]
        document = {
            "schema_version": 1,
            "page_number": page,
            "blocks": blocks,
            "termination": read_termination(args.scan_dir, page),
            "warnings": [],
        }
        target = args.output_dir / f"page-{page:04}.json"
        target.write_text(
            json.dumps(document, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    print(f"collected {args.page_count} prediction pages into {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
