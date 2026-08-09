#!/usr/bin/env python3
"""Validate the non-distributable P0 regression-set contract."""
import json
import sys
from pathlib import Path

REQUIRED = {"text", "formula", "table", "visual", "mixed", "blank", "truncated", "failure"}

def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate-regression-set.py SET_DIR", file=sys.stderr)
        return 2
    root = Path(sys.argv[1])
    try:
        manifest = json.loads((root / "manifest.json").read_text())
    except Exception as error:
        print(f"invalid manifest: {error}", file=sys.stderr)
        return 2
    if manifest.get("schema_version") != 1 or manifest.get("page_count", 0) < 30:
        print("regression set requires schema_version=1 and at least 30 pages", file=sys.stderr)
        return 3
    if not manifest.get("input_sha256") or not manifest.get("prompt_revision") or not manifest.get("adapter_revision"):
        print("manifest is missing provenance fields", file=sys.stderr)
        return 3
    truth = root / "truth"
    files = sorted(truth.glob("page-*.json"))
    if len(files) != manifest["page_count"]:
        print("truth page count does not match manifest", file=sys.stderr)
        return 3
    categories = {category: 0 for category in REQUIRED}
    for path in files:
        page = json.loads(path.read_text())
        if page.get("schema_version") != 1 or not page.get("page_number"):
            print(f"invalid truth page: {path}", file=sys.stderr)
            return 3
        for category in page.get("categories", []):
            if category in categories:
                categories[category] += 1
    missing = [category for category, count in categories.items() if count < 2]
    if missing:
        print(f"coverage requires at least two pages per category: {','.join(missing)}", file=sys.stderr)
        return 3
    print(json.dumps({"schema_version": 1, "pages": len(files), "categories": categories}, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
