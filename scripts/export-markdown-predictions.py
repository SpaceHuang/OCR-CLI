#!/usr/bin/env python3
"""Convert CLI per-page Markdown artifacts into minimal PageDocument JSON.

This is intentionally conservative: it preserves OCR text as one markdown
block and marks the page as EOS. It does not invent formulas, tables, or
visual-region bboxes that are not present in the CLI Markdown artifact.
"""
import argparse
import json
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("input_dir", type=Path)
    ap.add_argument("output_dir", type=Path)
    ap.add_argument("--page-count", type=int, required=True)
    args = ap.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for page in range(1, args.page_count + 1):
        source = args.input_dir / f"page-{page:04d}.md"
        if not source.is_file():
            raise SystemExit(f"missing {source}")
        content = source.read_text(encoding="utf-8")
        document = {
            "schema_version": 1,
            "page_number": page,
            "blocks": [{"type": "markdown", "content": content}],
            "termination": "eos",
            "warnings": [],
        }
        (args.output_dir / f"page-{page:04d}.json").write_text(
            json.dumps(document, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
