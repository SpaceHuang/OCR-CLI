#!/usr/bin/env python3
"""Summarize pages needing manual review for formula/table/visual alignment."""
import argparse
import json
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("truth", type=Path)
    ap.add_argument("score", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()
    score = json.loads(args.score.read_text(encoding="utf-8"))
    rows = []
    for number, page in enumerate(score["pages"], 1):
        truth = json.loads((args.truth / f"page-{number:04d}.json").read_text(encoding="utf-8"))
        counts = {kind: sum(block.get("type") == kind for block in truth.get("blocks", [])) for kind in ("formula", "html_table", "visual_region")}
        if any(counts.values()):
            rows.append({"page": number, "truth": counts, "metrics": {key: page[key] for key in ("formula_completeness", "table_teds", "bbox_precision", "bbox_recall", "text_ned")}})
    args.output.write_text(json.dumps({"schema_version": 1, "pages": rows}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {len(rows)} diagnostic pages to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
