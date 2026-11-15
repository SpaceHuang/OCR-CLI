#!/usr/bin/env python3
"""E1 boundary-sample joiner (root-cause plan SP-T02, v1.2 pairing rules).

Strictly joins the scan-side and direct-side BoundarySample JSONL records
on `page_number` (both sides emit 1-based numbers; the scan side is
restricted to attempt == 1 — ladder retries feed the replay/control
datasets and never participate in, or invalidate, the dual-path pairing).

Exit codes:
  0 — valid experiment; verdict is DIFF or IDENTICAL (report on stdout)
  1 — INVALID pairing (missing/duplicate records, input-hash mismatch);
      no root-cause conclusion may be drawn
  2 — usage error
"""
import argparse
import json
import sys


def load_samples(path, source, attempts=(1,)):
    """Loads one JSONL side; returns (by_page, errors).

    by_page maps page_number -> record for records whose attempt is in
    `attempts` (the pairing scope). Duplicate page keys within the scope
    are pairing-fatal. Records outside the scope are returned separately
    (they belong to the replay/control datasets).
    """
    by_page = {}
    out_of_scope = []
    errors = []
    seen_input_hashes = set()
    try:
        with open(path, encoding="utf-8") as handle:
            lines = [line for line in handle.read().splitlines() if line.strip()]
    except OSError as error:
        errors.append(f"cannot read {path}: {error}")
        return by_page, out_of_scope, errors, seen_input_hashes
    for number, line in enumerate(lines, start=1):
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            errors.append(f"{path}:{number}: invalid JSON: {error}")
            continue
        seen_input_hashes.add(record.get("input_sha256"))
        if record.get("source") != source:
            errors.append(
                f"{path}:{number}: expected source={source}, got {record.get('source')}"
            )
            continue
        if record.get("attempt") not in attempts:
            out_of_scope.append(record)
            continue
        page = record.get("page_number")
        if page is None:
            errors.append(f"{path}:{number}: record without page_number")
            continue
        if page in by_page:
            errors.append(
                f"{path}: duplicate attempt-{record.get('attempt')} record for page {page}"
            )
            continue
        by_page[page] = record
    return by_page, out_of_scope, errors, seen_input_hashes


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--scan", required=True, help="scan-side JSONL")
    parser.add_argument("--direct", required=True, help="direct-side JSONL")
    parser.add_argument("--output", help="verdict JSON path (default stdout)")
    args = parser.parse_args(argv)

    scan, scan_retry, scan_errors, scan_hashes = load_samples(args.scan, "scan", attempts=(1,))
    direct, direct_extra, direct_errors, direct_hashes = load_samples(args.direct, "direct", attempts=(1,))
    errors = scan_errors + direct_errors

    # Input identity: both sides must describe the same document.
    if scan_hashes and direct_hashes and not (scan_hashes & direct_hashes):
        errors.append(
            f"input_sha256 mismatch: scan={sorted(scan_hashes)} direct={sorted(direct_hashes)}"
        )

    missing_direct = sorted(set(scan) - set(direct))
    unmatched_direct = sorted(set(direct) - set(scan))
    if missing_direct:
        errors.append(f"scan pages without a direct attempt-1 record: {missing_direct}")

    pairs = []
    for page in sorted(set(scan) & set(direct)):
        s, d = scan[page], direct[page]
        param_fields = (
            "width",
            "height",
            "max_output_tokens",
            "prompt_sha256",
            "image_tokens_override",
            "retry_sampling",
        )
        param_mismatch = {field: [s.get(field), d.get(field)] for field in param_fields if s.get(field) != d.get(field)}
        pairs.append({
            "page_number": page,
            "scan_attempt": s.get("attempt"),
            "bytes_match": s.get("rgb8_sha256") == d.get("rgb8_sha256"),
            "scan_rgb8_sha256": s.get("rgb8_sha256"),
            "direct_rgb8_sha256": d.get("rgb8_sha256"),
            "scan_len": s.get("rgb8_len"),
            "direct_len": d.get("rgb8_len"),
            "scan_outcome": s.get("outcome"),
            "direct_outcome": d.get("outcome"),
            "param_mismatch": param_mismatch,
        })

    verdict = "INVALID" if errors else (
        "DIFF" if any(not pair["bytes_match"] or pair["param_mismatch"] for pair in pairs)
        else "IDENTICAL"
    )
    report = {
        "verdict": verdict,
        "pairing": {
            "paired_pages": len(pairs),
            "missing_direct": missing_direct,
            "unmatched_direct": unmatched_direct,
            "scan_retry_records": len(scan_retry),
            "direct_extra_records": len(direct_extra),
        },
        "errors": errors,
        "pairs": pairs,
    }
    text = json.dumps(report, ensure_ascii=False, indent=2)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")
    else:
        print(text)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
