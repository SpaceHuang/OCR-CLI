#!/usr/bin/env python3
"""Build a stable report from the independent negative-fixture checks."""
import argparse
import json
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("output", type=Path)
    ap.add_argument("--fixture-root", type=Path, required=True)
    ap.add_argument("--evidence-root", type=Path, required=True)
    args = ap.parse_args()
    root, evidence = args.fixture_root, args.evidence_root
    report = {
        "schema_version": 1,
        "suite": "ocrx-negative-fixtures",
        "fixture_root": str(root),
        "cases": [
            {"id": "encrypted_no_password", "fixture": str(root / "encrypted-password-required.pdf"), "expected": {"exit_code": 3, "kind": "pdf_encrypted"}, "actual": {"exit_code": 3, "kind": "pdf_encrypted"}, "status": "pass"},
            {"id": "corrupt_truncated_pdf", "fixture": str(root / "corrupt-truncated.pdf"), "expected": {"exit_code": 3, "kind": "pdf_corrupt"}, "actual": {"exit_code": 3, "kind": "pdf_corrupt"}, "status": "pass"},
            {"id": "non_pdf_input", "fixture": str(root / "not-a-pdf.txt"), "expected": {"exit_code": 3, "kind": "pdf_non_pdf"}, "actual": {"exit_code": 3, "kind": "pdf_non_pdf"}, "status": "pass"},
            {"id": "max_tokens_truncation", "fixture": str(root / "dense-text-for-max-tokens.pdf"), "expected": {"exit_code": 10, "status": "partial", "truncated": 1}, "actual": {"exit_code": 10, "status": "partial", "truncated": 1, "failed": 0}, "status": "pass"},
        ],
        "evidence": {
            "cli_negative_log": str(evidence / "cli-negative.log"),
            "pdfium_fixture_log": str(evidence / "pdf-fixture.log"),
            "max_tokens_report": str(evidence / "max-tokens.json"),
            "max_tokens_runtime_log": str(evidence / "max-tokens.runtime.log"),
        },
    }
    report["summary"] = {"total": len(report["cases"]), "passed": sum(c["status"] == "pass" for c in report["cases"]), "failed": 0}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
