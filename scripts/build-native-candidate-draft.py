#!/usr/bin/env python3
"""Build a review-only native-set candidate from the existing OCR truth set.

This intentionally never claims that a page is native-safe.  It carries over
the existing content truth and adds the v0.2 routing/provenance fields so a
human can accept, reject, or correct each page after PDFium inspection.
"""
import copy
import hashlib
import json
import shutil
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets/regression/ovisocr2-draft-v1"
TARGET = ROOT / "assets/regression/pdf-native-candidate-draft-v0.1"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    if TARGET.exists():
        raise SystemExit(f"refusing to overwrite existing output: {TARGET}")
    truth_source = SOURCE / "truth"
    truth_target = TARGET / "truth"
    truth_target.mkdir(parents=True)
    source_manifest = json.loads((SOURCE / "manifest.json").read_text())
    source_map = json.loads((SOURCE / "page-map.json").read_text())
    map_by_page = {item["page_number"]: item for item in source_map["pages"]}

    for source_path in sorted(truth_source.glob("page-*.json")):
        page = json.loads(source_path.read_text())
        page_number = page["page_number"]
        page["schema_version"] = 2
        page["native_candidate"] = {
            "page_type": "uncertain",
            "confidence": "low",
            "planned_method": "native_text",
            "actual_method": None,
            "reason": "native_validation_pending",
            "source": "pdf_text",
            "classifier_version": "pdf-route-v1",
            "extractor_version": "pdf-text-v1",
            "review_status": "needs_human_review",
            "allow_native": None,
            "review_notes": [
                "Candidate derived from OCR truth; PDFium native text evidence is not asserted.",
                "Confirm visible text, L2 structure, order, direction, rotation, and bbox against the source page.",
            ],
        }
        page["draft_provenance"] = {
            "source_truth_set": source_manifest["set_id"],
            "source_page_map": map_by_page.get(page_number),
            "generated_by": "build-native-candidate-draft.py",
            "generated_status": "review_only_not_release_truth",
        }
        (truth_target / source_path.name).write_text(
            json.dumps(page, ensure_ascii=False, indent=2) + "\n"
        )

    manifest = copy.deepcopy(source_manifest)
    manifest.update(
        {
            "set_id": "pdf-native-candidate-draft-v0.1",
            "purpose": "review-only candidate for v0.2 native text qualified set",
            "status": "draft_needs_human_acceptance",
            "truth_schema": "ocrx-pdf-native-candidate-v0.1",
            "native_route": {
                "classifier_version": "pdf-route-v1",
                "extractor_version": "pdf-text-v1",
                "required_quality_tier": "l2",
                "allowed_final_method": "native_text",
                "candidate_pages_require_pdfium_and_human_acceptance": True,
            },
            "review_contract": {
                "all_pages_require_human_acceptance": True,
                "reject_if_scanned_or_mixed": True,
                "reject_if_native_validation_fails": True,
                "reject_if_truncated_category_present": True,
                "required_per_page_evidence": [
                    "pdfium_visible_character_count",
                    "unicode_validity",
                    "text_bbox_coverage",
                    "image_bbox_coverage",
                    "native_validation_result",
                    "l2_block_roles_and_boundaries",
                    "reading_order",
                    "direction_and_rotation",
                ],
            },
            "source_draft": {
                "set_id": source_manifest["set_id"],
                "manifest_sha256": sha256(SOURCE / "manifest.json"),
                "truth_pages_copied": source_manifest["page_count"],
            },
        }
    )
    (TARGET / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    (TARGET / "README.md").write_text(
        """# PDF native qualified-set candidate (review draft)\n\n"
        "This directory is a review-only candidate generated from\n"
        "`ovisocr2-draft-v1`. It is **not** release truth and does not claim that\n"
        "any page is safe for native extraction. Every page is marked\n"
        "`needs_human_review` until PDFium evidence and visual inspection confirm\n"
        "the L2 native requirements.\n\n"
        "The source set contains OCR/visual truth, not native-PDF truth. The\n"
        "`native_candidate` object on each page records the provisional route and\n"
        "the evidence that must be filled in or corrected during acceptance.\n\n"
        "Pages carrying `truncated`, scanned, image-only, mixed, hidden-text,\n"
        "duplicate-text, or native-validation-failure evidence must be rejected\n"
        "from the native qualified set and moved to the forbidden/native-negative\n"
        "set.\n"""
    )


if __name__ == "__main__":
    main()
