#!/usr/bin/env python3
"""Stage 6 preflight gate for the 30-page PDF native quality set.

Structural validation is separated from release acceptance: a valid draft is
reported, but the gate only passes after all 30 pages are human-accepted.
"""
import importlib.util
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VALIDATOR = ROOT / "scripts/validate-pdf-native-candidate.py"
FORBIDDEN_VALIDATOR = ROOT / "scripts/validate-pdf-native-forbidden.py"
spec = importlib.util.spec_from_file_location("candidate_validator", VALIDATOR)
candidate_validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(candidate_validator)
score_spec = importlib.util.spec_from_file_location("native_scorer", ROOT / "scripts/score-pdf-native.py")
native_scorer = importlib.util.module_from_spec(score_spec)
score_spec.loader.exec_module(native_scorer)
forbidden_spec = importlib.util.spec_from_file_location("forbidden_validator", FORBIDDEN_VALIDATOR)
forbidden_validator = importlib.util.module_from_spec(forbidden_spec)
forbidden_spec.loader.exec_module(forbidden_validator)

def main():
    if len(sys.argv) not in (2, 3, 4):
        print("usage: run-pdf-native-stage6-gate.py SET_DIR [PREDICTION_DIR] [FORBIDDEN_SET_DIR]", file=sys.stderr)
        return 2
    root = Path(sys.argv[1])
    structural = candidate_validator.validate(root, emit=False)
    if structural != 0:
        return structural
    manifest = json.loads((root / "manifest.json").read_text())
    accepted = manifest.get("visual_audit", {}).get("native_accepted_pages", [])
    score_report = None
    if len(sys.argv) >= 3:
        try:
            score_report = native_scorer.score(root / "truth", Path(sys.argv[2]))
        except ValueError as error:
            print(str(error), file=sys.stderr)
            return 3
    forbidden_report = None
    if len(sys.argv) == 4:
        try:
            forbidden_report = forbidden_validator.validate(Path(sys.argv[3]))
        except (OSError, ValueError, json.JSONDecodeError) as error:
            print(str(error), file=sys.stderr)
            return 3
    report = {
        "schema_version": 1,
        "stage": 6,
        "set_id": manifest.get("set_id"),
        "structural_valid": True,
        "page_count": manifest.get("page_count"),
        "native_accepted_pages": accepted,
        "native_acceptance_complete": sorted(accepted) == list(range(1, 31)),
        "release_ready": sorted(accepted) == list(range(1, 31)),
        "blocked_reasons": [] if sorted(accepted) == list(range(1, 31)) else ["human_native_acceptance_incomplete"],
    }
    if forbidden_report is not None:
        report["forbidden_set"] = forbidden_report
        evidence_path = Path(sys.argv[3]) / "review" / "route-evidence.json"
        try:
            evidence = json.loads(evidence_path.read_text())
        except (OSError, ValueError, json.JSONDecodeError) as error:
            print(f"missing or invalid forbidden route evidence: {error}", file=sys.stderr)
            return 3
        report["forbidden_route_evidence"] = evidence
        if (evidence.get("page_count") != forbidden_report["pages"]
                or evidence.get("native_text_pages") != 0
                or evidence.get("native_block_pages") != 0
                or evidence.get("image_object_pages") != forbidden_report["pages"]
                or evidence.get("native_false_positive_pages")):
            report["release_ready"] = False
            report["blocked_reasons"].append("forbidden_native_false_positive")
    if score_report is not None:
        report["native_quality_score"] = score_report
        metrics = score_report["metrics"]
        if metrics["text_ned"] > 0.01 or metrics["reading_order"] < 0.98 or metrics["block_f1"] < 0.97 or metrics["bbox_validity_rate"] < 0.99 or metrics["bbox_recall"] < 0.97 or score_report["duplicate_pages"] or score_report["blank_or_failed_pages"]:
            report["release_ready"] = False
            report["blocked_reasons"].append("native_quality_threshold_failed")
    print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["release_ready"] else 4

if __name__ == "__main__":
    raise SystemExit(main())
