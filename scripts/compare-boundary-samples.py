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
import re
import sys


# The frozen BoundarySample schema (SP-T01): every key must be present in
# every record; NULLABLE_KEYS are the only fields where an explicit null
# is a legitimate value (the optional rung fields, absent on attempt 1).
SCHEMA_KEYS = (
    "input_sha256",
    "run_id",
    "source",
    "page_number",
    "page_index",
    "attempt",
    "window_id",
    "slot",
    "rgb8_len",
    "rgb8_sha256",
    "prompt_sha256",
    "width",
    "height",
    "max_output_tokens",
    "base_image_tokens",
    "image_tokens_override",
    "retry_sampling",
    "outcome",
    "schema_version",
)
NULLABLE_KEYS = ("image_tokens_override", "retry_sampling")

# Type and value-domain contract (ninth review): integer keys must be JSON
# integers (bool excluded) within their floors; enum keys have closed
# value sets; the identity/byte hashes must be exactly 64 characters
# (truncated hashes cannot be verified against evidence).
INT_KEYS = (
    "page_number",
    "page_index",
    "attempt",
    "window_id",
    "slot",
    "rgb8_len",
    "width",
    "height",
    "max_output_tokens",
    "base_image_tokens",
    "schema_version",
)
INT_MINIMUMS = {
    "page_number": 1,
    "page_index": 0,
    "attempt": 1,
    "window_id": 0,
    "slot": 0,
    "rgb8_len": 0,
    "width": 1,
    "height": 1,
    "max_output_tokens": 1,
    "base_image_tokens": 1,
    "schema_version": 1,
}
ENUM_DOMAINS = {
    "source": ("scan", "direct"),
    "outcome": ("empty", "nonempty"),
}
HEX64_KEYS = ("input_sha256", "rgb8_sha256", "prompt_sha256")


def load_samples(path, source, attempts=(1,)):
    """Loads one JSONL side; returns (by_page, out_of_scope, errors,
    seen_input_hashes, total_records, schema_versions).

    by_page maps page_number -> record for records whose attempt is in
    `attempts` (the pairing scope). Duplicate page keys within the scope
    are pairing-fatal. Records outside the scope are returned separately
    (they belong to the replay/control datasets).

    All records must carry the full mandatory field set (byte hashes,
    base grid, geometry, output budget) — records predating a field are
    migrated to the current schema at the evidence layer, never exempted
    here (the seventh-review residuals closed the schema-version bypass).
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
        schema_versions.add(record.get("schema_version"))
        # Full schema contract (eighth review): every schema key must be
        # PRESENT — a deleted key is a schema violation even when both
        # sides delete it (None == None would fake a match). Explicit null
        # is legitimate ONLY on the optional rung fields (no override /
        # no sampling on attempt 1); every other key must be non-null and,
        # for the identity/geometry/hash strings, non-empty. Records
        # predating a field are migrated to the current schema at the
        # evidence layer, never exempted here.
        missing_keys = [key for key in SCHEMA_KEYS if key not in record]
        if missing_keys:
            errors.append(f"{path}:{number}: missing {', '.join(missing_keys)}")
            continue
        if record.get("source") != source:
            errors.append(
                f"{path}:{number}: expected source={source}, got {record.get('source')}"
            )
            continue
        # Attempt scope FIRST (v1.2): ladder retries are archived for the
        # replay/control datasets and never enter — or invalidate — the
        # attempt-1 pairing field validation.
        if record.get("attempt") not in attempts:
            out_of_scope.append(record)
            continue
        invalid_null = [
            key
            for key in SCHEMA_KEYS
            if key not in NULLABLE_KEYS and record[key] is None
        ]
        if invalid_null:
            errors.append(f"{path}:{number}: null in non-nullable {', '.join(invalid_null)}")
            continue
        # run_id is audit metadata (the scan-side recorder used to leave it
        # empty in archived samples) — tolerated; the content/identity
        # hashes must never be empty.
        empty_values = [
            key
            for key in ("input_sha256", "rgb8_sha256", "prompt_sha256")
            if record[key] == ""
        ]
        if empty_values:
            errors.append(f"{path}:{number}: empty {', '.join(empty_values)}")
            continue
        # Type and value-domain contract (ninth review): integer fields
        # must be JSON integers (bool is not an acceptable integer), enums
        # are closed sets, hashes are 64-char lowercase hex. A record that
        # violates the contract cannot be paired or compared.
        type_errors = []
        int_keys_ok = set()
        for key in INT_KEYS:
            value = record[key]
            # bool is not an acceptable integer (json true/false).
            if isinstance(value, bool) or not isinstance(value, int):
                type_errors.append(f"{key} must be an integer")
                continue
            if value < INT_MINIMUMS[key]:
                type_errors.append(f"{key} below {INT_MINIMUMS[key]}")
                continue
            int_keys_ok.add(key)
        if "page_number" in int_keys_ok and "page_index" in int_keys_ok:
            if record["page_index"] != record["page_number"] - 1:
                type_errors.append("page_index must be page_number - 1")
        for key, allowed in ENUM_DOMAINS.items():
            if record[key] not in allowed:
                type_errors.append(f"{key} must be one of {allowed}")
        override = record["image_tokens_override"]
        if override is not None and (
            isinstance(override, bool) or not isinstance(override, int) or override < 1
        ):
            type_errors.append("image_tokens_override must be a positive integer or null")
        sampling = record["retry_sampling"]
        if sampling is not None and not (
            isinstance(sampling, list)
            and len(sampling) == 2
            and all(
                isinstance(value, int) and not isinstance(value, bool) and value >= 1
                for value in sampling
            )
        ):
            type_errors.append(
                "retry_sampling must be [repeat_penalty_milli, penalty_window] or null"
            )
        for key in HEX64_KEYS:
            value = record[key]
            # The raw value must BE a string: str() on a 64-digit JSON
            # integer would launder it through the hex check (eleventh
            # review).
            if not isinstance(value, str) or not re.fullmatch(
                r"[0-9a-f]{64}", value
            ):
                type_errors.append(f"{key} must be a 64-char lowercase hex string")
        # Tenth review: the schema is frozen at v2 — a complete v1-shaped
        # record is still a legacy record (migrate it; no exemption).
        if record.get("schema_version") != 2:
            type_errors.append("schema_version must be 2")
        if type_errors:
            errors.append(f"{path}:{number}: " + "; ".join(type_errors))
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
            "base_image_tokens",
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
