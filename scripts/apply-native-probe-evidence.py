#!/usr/bin/env python3
"""Record freshly rebuilt PDFium probe evidence in the review draft."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "assets/regression/pdf-native-candidate-draft-v0.1"
EVIDENCE = {
    16: {
        "source_page": 3,
        "native_text_chars": 327,
        "native_blocks": 1,
        "visible_chars": 260,
        "unicode_validity": 1.0,
        "text_bbox_coverage": 0.000167,
        "text_coverage": 1.0,
        "image_coverage": 0.0,
        "image_objects": 0,
        "duplicate_text_geometry": False,
        "geometry_anomaly": False,
        "l2_validation": "failed",
        "validation_reason": "native_structure_unresolved",
    },
    28: {
        "source_page": 156,
        "native_text_chars": 606,
        "native_blocks": 2,
        "visible_chars": 552,
        "unicode_validity": 1.0,
        "text_bbox_coverage": 0.000782,
        "text_coverage": 1.0,
        "image_coverage": 0.0,
        "image_objects": 0,
        "duplicate_text_geometry": False,
        "geometry_anomaly": True,
        "l2_validation": "failed",
        "validation_reason": "geometry_anomaly",
    },
}

def main() -> None:
    for page_number, evidence in EVIDENCE.items():
        path = TARGET / "truth" / f"page-{page_number:04}.json"
        data = json.loads(path.read_text())
        candidate = data["native_candidate"]
        candidate.update({
            "confidence": "low",
            "planned_method": "ocr",
            "actual_method": None,
            "allow_native": False,
            "reason": evidence["validation_reason"],
            "review_status": "needs_human_review",
            "visual_audit_status": "reviewed",
            "pdfium_evidence": {
                "probe_version": "pdfium-native-probe-v1",
                **evidence,
            },
            "review_fields": [
                "verify source page mapping",
                "verify visual text against native extraction",
                "retain page outside native qualified set",
            ],
        })
        data["visual_audit"]["native_decision"] = "reject"
        data["visual_audit"]["pdfium_evidence"] = candidate["pdfium_evidence"]
        data["visual_audit"]["fields_requiring_review"] = candidate["review_fields"]
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")

    audit_path = TARGET / "review" / "visual-audit.json"
    audit = json.loads(audit_path.read_text())
    for item in audit["pages"]:
        if item["page_number"] in EVIDENCE:
            evidence = EVIDENCE[item["page_number"]]
            item["native_decision"] = "reject"
            item["reason"] = evidence["validation_reason"]
            item["pdfium_evidence"] = {
                "probe_version": "pdfium-native-probe-v1",
                **evidence,
            }
    audit_path.write_text(json.dumps(audit, ensure_ascii=False, indent=2) + "\n")

    manifest_path = TARGET / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["visual_audit"].update({
        "native_accepted_pages": 0,
        "native_rejected_pages": 30,
        "native_pending_pages": [],
        "pdfium_native_evidence_complete": True,
        "probe_evidence_pages": [16, 28],
    })
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")

    summary = TARGET / "review" / "visual-audit-summary.md"
    with summary.open("a") as stream:
        stream.write("\n## PDFium probe evidence\n\n")
        stream.write("The current worktree source was rebuilt with `cargo build --release -p ocrx-pdf --bin pdfium-check`. Fresh probe evidence rejects both previously pending pages:\n\n")
        stream.write("| Candidate page | Source page | Native chars | Visible chars | Blocks | Text bbox coverage | Geometry anomaly | L2 decision |\n| ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |\n")
        stream.write("| 16 | 3 | 327 | 260 | 1 | 0.000167 | false | failed: native_structure_unresolved |\n")
        stream.write("| 28 | 156 | 606 | 552 | 2 | 0.000782 | true | failed: geometry_anomaly |\n\n")
        stream.write("No page is currently accepted as native. The candidate set remains a review draft and must not be used as the final native qualified set.\n")

if __name__ == "__main__":
    main()
