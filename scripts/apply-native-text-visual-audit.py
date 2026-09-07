#!/usr/bin/env python3
"""Record the independent visual audit in the native-text candidate draft.

The native extraction remains the raw fact source. Audit findings are additive
and explicitly separated so a later human reviewer can accept or reject them.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "assets/regression/pdf-native-text-candidate-v0.1"
SOURCE_PAGES = [5, 6, 7, 10, 11, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23,
                24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 35, 36, 37, 38, 39]

SPECIAL = {
    6: {
        "severity": "block_structure",
        "finding": "Visual title is THE REAL-LIFE MBA; native blocks split it into THE, a blank block, and REAL-LIFE MBA.",
        "expected": "Merge title fragments into one logical block and remove the blank block.",
    },
    11: {
        "severity": "spacing",
        "finding": "Footnote marker [1] follows 个人贡献者 visually; block text has a spacing difference.",
        "expected": "Preserve the visual relation as 个人贡献者[1]。 or the renderer's equivalent footnote association.",
    },
    29: {
        "severity": "spacing",
        "finding": "SPIRIT section contains deliberate visual spacing in 行 动 模 式 and 服务（service）; block text compresses some spaces.",
        "expected": "Retain visual spacing in the display text or record a normalized-text equivalence explicitly.",
    },
}

def main() -> None:
    audit_pages = []
    for candidate_page, source_page in enumerate(SOURCE_PAGES, 1):
        path = TARGET / "truth" / f"page-{candidate_page:04}.json"
        data = json.loads(path.read_text())
        finding = SPECIAL.get(candidate_page)
        data["bbox_coordinate_space"] = "normalized_1000"
        data["page_size_points"] = {"width": 612, "height": 792}
        data["watermark"] = {
            "present": True,
            "description": "Bottom-right grey 网络加速器 / t.tizi8.com / t.it520.cc / t.hi6.uk watermark visible in rendered page",
            "included_in_native_text": False,
            "policy": "exclude_from_native_text_body_pending_human_acceptance",
        }
        data["visual_audit"] = {
            "status": "reviewed_by_model_vision",
            "content_accuracy": "mostly_accurate",
            "reading_order": "continuous_top_to_bottom",
            "native_text_matches_visible_body": True,
            "finding": finding or "No page-specific text discrepancy found; confirm bbox and watermark policy during human acceptance.",
        }
        data["visual_corrections"] = [finding] if finding else []
        data["review_status"] = "needs_human_review"
        data["native_decision"] = "pending_l2_block_review"
        data["review_notes"] = [
            "Independent visual audit completed for this page; audit did not use prior regression truth.",
            "Bboxes are normalized 0-1000, not PDF point coordinates.",
            "Native body text is visually plausible, but final L2 acceptance remains pending human review.",
        ]
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
        audit_pages.append({"candidate_page": candidate_page, "source_page": source_page,
                            "status": "reviewed_by_model_vision",
                            "content_accuracy": "mostly_accurate",
                            "page_specific_finding": finding})

    manifest_path = TARGET / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["status"] = "draft_reviewed_pending_human_l2_acceptance"
    manifest["manifest_role"] = "pdf_native_quality_set"
    manifest["quality_set_type"] = "native_allowed"
    manifest["prompt_revision"] = "not_applicable_native_pdfium"
    manifest["adapter_revision"] = "not_applicable_native_pdfium"
    manifest["runtime_revision"] = "not_applicable_native_pdfium"
    manifest["model_revision"] = None
    manifest["truth_version"] = "pdf-native-truth-v1"
    manifest["benchmark_version"] = "pdf-native-benchmark-v1"
    manifest["classifier_revision"] = manifest.get("classifier_version")
    manifest["extractor_revision"] = manifest.get("extractor_version")
    manifest["required_categories"] = ["text"]
    manifest["native_allowed"] = True
    manifest["quality_thresholds"] = {
        "text_ned_max": 0.01, "reading_order_min": 0.98,
        "block_precision_min": 0.97, "block_recall_min": 0.97,
        "block_f1_min": 0.97, "bbox_validity_rate_min": 0.99,
        "bbox_recall_min": 0.97, "duplicate_pages_max": 0,
        "blank_or_failed_pages_max": 0,
    }
    manifest["bbox_coordinate_space"] = "normalized_1000"
    manifest["page_size_points"] = {"width": 612, "height": 792}
    manifest["visual_audit"] = {
        "reviewed_pages": 30,
        "review_method": "model_vision_page_by_page",
        "prior_regression_truth_used": False,
        "native_accepted_pages": [],
        "human_acceptance_required": True,
    }
    manifest["watermark_policy"] = "Visible image watermark is recorded but excluded from native text body truth; human must confirm this policy."
    for page_entry in manifest["pages"]:
        page = json.loads((TARGET / "truth" / f"page-{page_entry['candidate_page']:04}.json").read_text())
        page_entry["expected_page_type"] = "text"
        page_entry["native_allowed"] = True
        page_entry["truth_version"] = manifest["truth_version"]
        page_entry["expected_native_blocks"] = len(page.get("blocks", []))
        page_entry["expected_native_text_chars"] = len(page.get("native_text", ""))
    manifest["known_limitations"] = [
        "Native text and normalized-1000 bboxes are recorded as raw evidence; no page is final L2 truth.",
        "Three page-specific block/spacing findings are recorded in truth.visual_corrections.",
        "Every page contains a visible bottom-right watermark not included in native_text.",
    ]
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")

    (TARGET / "review" / "visual-audit.json").write_text(
        json.dumps({"schema_version": 1, "method": "model_vision_page_by_page",
                    "prior_regression_truth_used": False, "pages": audit_pages},
                   ensure_ascii=False, indent=2) + "\n")
    (TARGET / "review" / "visual-audit-summary.md").write_text(
        "# Visual audit status\n\n"
        "30/30 pages were checked page by page with model vision against rendered source pages. "
        "The visible body text and reading order are mostly consistent with `native_text`; no page is "
        "marked final because human L2 acceptance is still required.\n\n"
        "## Data clarifications\n\n"
        "- `bbox` uses normalized 0-1000 coordinates (`bbox_coordinate_space`), while page size is recorded in PDF points.\n"
        "- All pages show a bottom-right grey watermark. It is recorded as visual metadata and excluded from native text body truth pending human confirmation.\n"
        "- Candidate pages 6, 11, and 29 contain explicit block/spacing findings in `truth/*.json`.\n"
        "- The complete machine-readable audit is in `review/visual-audit.json`.\n"
    )

if __name__ == "__main__":
    main()
