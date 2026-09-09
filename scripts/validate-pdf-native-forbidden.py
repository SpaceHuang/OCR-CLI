#!/usr/bin/env python3
"""Validate the structural contract of a PDF native-forbidden set."""
import hashlib, json, sys
from pathlib import Path

def validate(root):
    manifest = json.loads((root / "manifest.json").read_text())
    if manifest.get("schema_version") != 1 or manifest.get("quality_set_type") != "native_forbidden":
        raise ValueError("invalid native-forbidden manifest")
    if manifest.get("page_count", 0) < 20 or manifest.get("native_allowed") is not False:
        raise ValueError("native-forbidden set requires at least 20 pages and native_allowed=false")
    source = Path(manifest.get("source_pdf", ""))
    digest = hashlib.sha256(source.read_bytes()).hexdigest() if source.is_file() else ""
    if not digest or digest != manifest.get("input_sha256"):
        raise ValueError("forbidden source PDF hash mismatch")
    pages = manifest.get("pages") or []
    if len(pages) != manifest["page_count"] or len({p.get("source_page") for p in pages}) != len(pages):
        raise ValueError("forbidden pages must be unique and match page_count")
    for page in pages:
        if page.get("expected_page_type") not in ("image", "mixed", "uncertain") or page.get("native_allowed") is not False:
            raise ValueError("forbidden page has an invalid expected type or native policy")
    return {"valid": True, "pages": len(pages), "native_allowed": False, "expected_types": sorted({p["expected_page_type"] for p in pages})}

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: validate-pdf-native-forbidden.py SET_DIR", file=sys.stderr); raise SystemExit(2)
    try: print(json.dumps(validate(Path(sys.argv[1])), sort_keys=True))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr); raise SystemExit(3)
