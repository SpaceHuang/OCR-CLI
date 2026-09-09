#!/usr/bin/env python3
"""Validate the structural contract of a PDFium native candidate set."""
import hashlib
import json
import sys
from pathlib import Path

def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def fail(message):
    print(message, file=sys.stderr)
    return 3

def validate(root, emit=True):
    try:
        manifest = json.loads((root / "manifest.json").read_text())
    except Exception as error:
        return fail(f"invalid manifest: {error}")
    if manifest.get("schema_version") != 1 or manifest.get("truth_schema") != "ocrx-pdfium-native-facts-v0.1":
        return fail("invalid PDF native manifest schema")
    if manifest.get("page_count") != 30:
        return fail("PDF native candidate requires exactly 30 pages")
    required = ("manifest_role", "quality_set_type", "truth_version", "benchmark_version", "classifier_revision", "extractor_revision", "quality_thresholds")
    if any(field not in manifest for field in required) or manifest.get("quality_set_type") != "native_allowed":
        return fail("manifest is missing formal native quality-set fields")
    source = Path(manifest.get("source_pdf", ""))
    if not source.is_file() or sha256(source) != manifest.get("input_sha256"):
        return fail("source PDF is missing or input_sha256 does not match")
    pages = manifest.get("pages") or []
    expected_sources = [item.get("source_page") for item in pages]
    if len(pages) != 30 or len(set(expected_sources)) != 30:
        return fail("manifest pages must contain 30 unique source pages")
    truth_files = sorted((root / "truth").glob("page-*.json"))
    if len(truth_files) != 30:
        return fail("truth directory must contain exactly 30 page files")
    for index, path in enumerate(truth_files, 1):
        try:
            page = json.loads(path.read_text())
        except Exception as error:
            return fail(f"invalid truth JSON {path}: {error}")
        if page.get("candidate_page_number") != index or page.get("source_page_number") != expected_sources[index - 1]:
            return fail(f"page mapping mismatch: {path}")
        entry = pages[index - 1]
        if entry.get("expected_page_type") != "text" or entry.get("native_allowed") is not True or entry.get("truth_version") != manifest["truth_version"]:
            return fail(f"page quality metadata is incomplete: {path}")
        if not page.get("native_text") or page.get("bbox_coordinate_space") != "normalized_1000":
            return fail(f"missing native text or bbox coordinate space: {path}")
        blocks = page.get("blocks") or []
        orders = [block.get("source_order") for block in blocks]
        if not blocks or orders != list(range(1, len(blocks) + 1)):
            return fail(f"source_order must be contiguous in {path}")
        for block in blocks:
            bbox = block.get("bbox") or {}
            if block.get("source") != "pdf_text" or any(not isinstance(bbox.get(key), int) or not 0 <= bbox[key] <= 1000 for key in ("left", "top", "right", "bottom")) or bbox["left"] >= bbox["right"] or bbox["top"] >= bbox["bottom"]:
                return fail(f"invalid native PDF block in {path}")
        audit = page.get("visual_audit") or {}
        if audit.get("status") != "reviewed_by_model_vision":
            return fail(f"missing visual audit: {path}")
        if page.get("native_decision") == "accepted":
            return fail(f"human acceptance cannot be inferred: {path}")
    if emit:
        print(json.dumps({"valid": True, "pages": 30, "native_accepted_pages": manifest.get("visual_audit", {}).get("native_accepted_pages", []), "release_ready": False}, ensure_ascii=False, sort_keys=True))
    return 0

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: validate-pdf-native-candidate.py SET_DIR", file=sys.stderr)
        raise SystemExit(2)
    raise SystemExit(validate(Path(sys.argv[1])))
