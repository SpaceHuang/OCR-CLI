#!/usr/bin/env python3
"""Package raw PDFium native facts into a reviewable 30-page source draft."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "assets/regression/pdf-native-text-candidate-v0.1"
SOURCE_PDF = Path("/Users/space/Documents/Develop/OCR-CLI/test-data/books/商业金融/《商业的本质》杰克•韦尔奇【文字版_PDF电子书_下载】.pdf")
SOURCE_PAGES = [
    5, 6, 7, 10, 11, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 35, 36, 37, 38, 39,
]

def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def main() -> None:
    TARGET.mkdir(parents=True, exist_ok=True)
    (TARGET / "inputs").mkdir(exist_ok=True)
    (TARGET / "review").mkdir(exist_ok=True)
    truth = TARGET / "truth"
    truth.mkdir(exist_ok=True)
    source_hash = digest(SOURCE_PDF)
    pages = []
    for candidate_page, source_page in enumerate(SOURCE_PAGES, start=1):
        raw_path = truth / f"page-{source_page:04}.json"
        raw = json.loads(raw_path.read_text())
        raw["candidate_page_number"] = candidate_page
        raw["source_page_number"] = source_page
        raw["review_status"] = "needs_human_review"
        raw["native_decision"] = "pending_l2_extractor_fix"
        raw["review_notes"] = [
            "Selected from a text-heavy source PDF by PDFium native probe.",
            "Current probe exposes one coarse block and very low bbox coverage; do not accept as L2 yet.",
            "Confirm visual layout, headers/footers, reading order, block roles, and bbox after extractor improvements.",
        ]
        raw_path.rename(truth / f"page-{candidate_page:04}.json")
        (truth / f"page-{candidate_page:04}.json").write_text(json.dumps(raw, ensure_ascii=False, indent=2) + "\n")
        pages.append({"candidate_page": candidate_page, "source_page": source_page, "source_sha256": source_hash})
    manifest = {
        "schema_version": 1,
        "set_id": "pdf-native-text-candidate-v0.1",
        "status": "draft_needs_l2_and_human_acceptance",
        "purpose": "30-page native text qualified-set candidate from a text-heavy source PDF",
        "page_count": 30,
        "input_sha256": source_hash,
        "source_pdf": str(SOURCE_PDF),
        "renderer": {"dpi": 300, "pixel_budget": 1048576},
        "required_quality_tier": "l2",
        "classifier_version": "pdf-route-v1",
        "extractor_version": "pdf-text-v1",
        "native_route": {"allowed_final_method": "native_text", "all_pages_require_acceptance": True},
        "truth_schema": "ocrx-pdfium-native-facts-v0.1",
        "license": {"redistributable": False, "provenance": "Local test-data source PDF; not redistributable."},
        "selection": {"source_pages": SOURCE_PAGES, "selection_basis": "text-heavy pages with PDFium native text, >=20 native blocks, text_bbox_coverage >= 0.15, image_coverage < 0.10, and no geometry anomaly"},
        "pages": pages,
        "known_limitations": [
            "Current extractor emits one coarse block per page.",
            "Current native probe reports very low text_bbox_coverage and does not yet prove L2 block structure.",
            "This is not release truth until the extractor and human visual review pass.",
        ],
    }
    (TARGET / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")

if __name__ == "__main__":
    main()
