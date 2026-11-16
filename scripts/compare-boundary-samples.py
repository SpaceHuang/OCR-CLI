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
    """Loads one JSONL side; returns (by_page, out_of_scope, errors,
    seen_input_hashes, total_records, schema_versions).

    by_page maps page_number -> record for records whose attempt is in
    `attempts` (the pairing scope). Duplicate page keys within the scope
    are pairing-fatal. Records outside the scope are returned separately
    (they belong to the replay/control datasets).

    Schema handling: records without `schema_version` are schema v1
    (legacy — `base_image_tokens` not required, not compared); records
    with schema_version >= 2 must carry `base_image_tokens`, which the
    pair comparison treats as a parameter.
    """
    by_page = {}
    out_of_scope = []
    errors = []
    seen_input_hashes = set()
    schema_versions = set()
    total_records = 0
    try:
        with open(path, encoding="utf-8") as handle:
            lines = [line for line in handle.read().splitlines() if line.strip()]
    except OSError as error:
        errors.append(f"cannot read {path}: {error}")
        return by_page, out_of_scope, errors, seen_input_hashes, total_records, set()
    total_records = len(lines)
    for number, line in enumerate(lines, start=1):
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            errors.append(f"{path}:{number}: invalid JSON: {error}")
            continue
        seen_input_hashes.add(record.get("input_sha256"))
        schema_version = int(record.get("schema_version") or 1)
        schema_versions.add(schema_version)
        # Mandatory fields: without the byte hashes the pair comparison is
        # meaningless (None == None would fake a match). Schema v2 adds the
        # effective base grid (round-6 review): required there, treated as
        # uncompared for legacy v1 records.
        mandatory = ["rgb8_sha256", "prompt_sha256"]
        if schema_version >= 2:
            mandatory.append("base_image_tokens")
        missing = [key for key in mandatory if record.get(key) in (None, "")]
        if missing:
            errors.append(f"{path}:{number}: missing {', '.join(missing)}")
            continue
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
    return by_page, out_of_scope, errors, seen_input_hashes, total_records, schema_versions


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--scan", required=True, help="scan-side JSONL")
    parser.add_argument("--direct", required=True, help="direct-side JSONL")
    parser.add_argument("--output", help="verdict JSON path (default stdout)")
    args = parser.parse_args(argv)

    scan, scan_retry, scan_errors, scan_hashes, scan_total, scan_schemas = load_samples(
        args.scan, "scan", attempts=(1,)
    )
    direct, direct_extra, direct_errors, direct_hashes, direct_total, direct_schemas = load_samples(
        args.direct, "direct", attempts=(1,)
    )
    errors = scan_errors + direct_errors

    # An experiment that produced no records must never be reported as a
    # conclusion (plan SP-T02: empty datasets invalidate the experiment).
    if scan_total == 0 or direct_total == 0:
        errors.append(
            f"empty dataset: scan records={scan_total}, direct records={direct_total} — "
            "no experiment output can be validated"
        )

    # Input identity: each side must describe EXACTLY ONE document (mixed
    # identities on one side invalidate even when the two sides share a
    # hash), and the two sides must agree on it. A record without an
    # input_sha256 makes the document identity unverifiable — fatal.
    if None in scan_hashes or "" in scan_hashes:
        errors.append("scan side has records without input_sha256")
    if None in direct_hashes or "" in direct_hashes:
        errors.append("direct side has records without input_sha256")
    if len(scan_hashes) > 1:
        errors.append(
            f"scan side mixes input_sha256 values: "
            + str(sorted(scan_hashes, key=lambda value: (value is None, str(value))))
        )
    if len(direct_hashes) > 1:
        errors.append(
            f"direct side mixes input_sha256 values: "
            + str(sorted(direct_hashes, key=lambda value: (value is None, str(value))))
        )
    if scan_hashes and direct_hashes and not (scan_hashes & direct_hashes):
        none_safe = lambda value: (value is None, str(value))
        errors.append(
            f"input_sha256 mismatch: "
            f"scan={sorted(scan_hashes, key=none_safe)} "
            f"direct={sorted(direct_hashes, key=none_safe)}"
        )

    missing_direct = sorted(set(scan) - set(direct))
    unmatched_direct = sorted(set(direct) - set(scan))
    if missing_direct:
        errors.append(f"scan pages without a direct attempt-1 record: {missing_direct}")

    pairs = []
    for page in sorted(set(scan) & set(direct)):
        s, d = scan[page], direct[page]
        # The base grid is compared only when both records are schema v2
        # (legacy v1 archived evidence predates the field — its grid
        # limitation is documented in the root-cause plan).
        param_fields = (
            "width",
            "height",
            "max_output_tokens",
            "prompt_sha256",
            "image_tokens_override",
            "retry_sampling",
        )
        s_v2 = int(s.get("schema_version") or 1) >= 2
        d_v2 = int(d.get("schema_version") or 1) >= 2
        if s_v2 and d_v2:
            param_fields = ("base_image_tokens",) + param_fields
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

    # No pairs = no comparison happened: an empty pairing can never back
    # a conclusion, whatever the record counts say.
    if not pairs and not errors:
        errors.append(
            "no attempt-1 pairs formed — the E1 comparison requires at least "
            "one same-page scan attempt-1 / direct record pair"
        )
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
            # Record counts: archived verdicts stay recomputable against
            # their evidence files (plan review BLOCK-3).
            "scan_records_total": scan_total,
            "scan_attempt1_records": len(scan),
            "scan_retry_records": len(scan_retry),
            "direct_records_total": direct_total,
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
