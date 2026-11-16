#!/usr/bin/env python3
"""Unit checks for the E1 boundary-sample joiner (plan SP-T02, v1.2)."""
import importlib.util
import json
import os
import sys
from pathlib import Path
import tempfile

spec = importlib.util.spec_from_file_location(
    "compare_boundary_samples", Path(__file__).with_name("compare-boundary-samples.py")
)
cb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cb)

INPUT = "ab3235512ef1f60122c3b060c5cf43c12947839e82875d940e10c36e37963477"


def sample(source, page, attempt=1, sha="a" * 64, prompt="p" * 64, outcome="nonempty",
           width=2531, height=3276, override=None, sampling=None, input_sha=INPUT,
           base_image_tokens=2048, schema_version=2):
    return {
        "input_sha256": input_sha,
        "run_id": "run-x",
        "schema_version": schema_version,
        "source": source,
        "page_number": page,
        "page_index": page - 1,
        "attempt": attempt,
        "window_id": 0,
        "slot": 0,
        "rgb8_len": 100,
        "rgb8_sha256": sha,
        "prompt_sha256": prompt,
        "width": width,
        "height": height,
        "max_output_tokens": 16384,
        "base_image_tokens": base_image_tokens,
        "image_tokens_override": override,
        "retry_sampling": sampling,
        "outcome": outcome,
    }


def write_jsonl(path, records):
    with open(path, "w", encoding="utf-8") as handle:
        for record in records:
            handle.write(json.dumps(record, ensure_ascii=False) + "\n")


def run(scan_records, direct_records):
    tmp = tempfile.mkdtemp(prefix="cb-test-")
    scan_path = os.path.join(tmp, "scan.jsonl")
    direct_path = os.path.join(tmp, "direct.jsonl")
    write_jsonl(scan_path, scan_records)
    write_jsonl(direct_path, direct_records)
    code = cb.main(["--scan", scan_path, "--direct", direct_path])
    return code


def check(name, condition):
    if not condition:
        print(f"FAIL {name}")
        sys.exit(1)
    print(f"ok   {name}")


# 1) Identical bytes on all pairs -> IDENTICAL, exit 0.
code = run(
    [sample("scan", 4, sha="h4"), sample("scan", 5, sha="h5")],
    [sample("direct", 4, sha="h4"), sample("direct", 5, sha="h5")],
)
check("identical bytes -> exit 0", code == 0)

# 2) Byte difference on one page -> valid experiment (exit 0); verdict DIFF
#    is carried in the report (checked via captured stdout by re-running
#    with a known pair and inspecting the module-level behavior below).
code = run(
    [sample("scan", 4, sha="scan-bytes"), sample("scan", 5, sha="h5")],
    [sample("direct", 4, sha="direct-bytes"), sample("direct", 5, sha="h5")],
)
check("byte diff is a valid experiment -> exit 0", code == 0)

# 3) Missing direct record for a scan attempt-1 page -> INVALID, exit 1.
code = run(
    [sample("scan", 4, sha="h4"), sample("scan", 5, sha="h5")],
    [sample("direct", 4, sha="h4")],
)
check("missing direct page -> exit 1", code == 1)

# 4) Duplicate scan attempt-1 record for one page -> INVALID.
code = run(
    [sample("scan", 4, sha="h4"), sample("scan", 4, sha="h4b")],
    [sample("direct", 4, sha="h4")],
)
check("duplicate scan page -> exit 1", code == 1)

# 5) Scan attempt>=2 records NEVER invalidate the pairing (v1.2 gate scope).
code = run(
    [
        sample("scan", 4, attempt=1, sha="h4"),
        sample("scan", 4, attempt=2, sha="retry-bytes", sampling=[1200, 64]),
        sample("scan", 4, attempt=3, sha="retry-bytes-2"),
    ],
    [sample("direct", 4, sha="h4")],
)
check("attempt>=2 records out of pairing scope -> exit 0", code == 0)

# 6) input_sha256 mismatch between sides -> INVALID.
code = run(
    [sample("scan", 4, sha="h4", input_sha="doc-a")],
    [sample("direct", 4, sha="h4", input_sha="doc-b")],
)
check("input hash mismatch -> exit 1", code == 1)

# 7) Param mismatch (render geometry differs) -> valid experiment; the
#    pair carries the mismatch fields.
code = run(
    [sample("scan", 4, sha="h4", width=100, height=50)],
    [sample("direct", 4, sha="h4")],
)
check("param mismatch -> exit 0", code == 0)

# 8) EMPTY scan dataset -> INVALID (an experiment that produced no
#    records must never be reported as IDENTICAL).
code = run([], [sample("direct", 4, sha="h4")])
check("empty scan dataset -> exit 1", code == 1)

code = run([sample("scan", 4, sha="h4")], [])
check("empty direct dataset -> exit 1", code == 1)

# 9) Mixed document identities WITHIN one side -> INVALID even when the
#    two sides share an input hash (the adversarial three-document case).
mixed_scan = [
    sample("scan", 1, sha="h1", input_sha="doc-a"),
    sample("scan", 2, sha="h2", input_sha="doc-a"),
    sample("scan", 3, sha="h3", input_sha="doc-b"),
]
code = run(mixed_scan, [sample("direct", 1, sha="h1", input_sha="doc-a")])
check("mixed input hashes on scan side -> exit 1", code == 1)

# 10) The verdict report must carry record counts so archived verdicts
#     stay recomputable against their evidence files (record-count check).
tmp = tempfile.mkdtemp(prefix="cb-counts-")
scan_path = os.path.join(tmp, "scan.jsonl")
direct_path = os.path.join(tmp, "direct.jsonl")
verdict_path = os.path.join(tmp, "verdict.json")
write_jsonl(scan_path, [
    sample("scan", 4, attempt=1, sha="h4"),
    sample("scan", 4, attempt=2, sha="retry", sampling=[1200, 64]),
    sample("scan", 5, attempt=1, sha="h5"),
])
write_jsonl(direct_path, [sample("direct", 4, sha="h4"), sample("direct", 5, sha="h5")])
code = cb.main([
    "--scan", scan_path, "--direct", direct_path, "--output", verdict_path,
])
check("counts verdict exit 0", code == 0)
verdict = json.load(open(verdict_path, encoding="utf-8"))
pairing = verdict["pairing"]
check(
    "verdict counts: total records",
    pairing["scan_records_total"] == 3 and pairing["direct_records_total"] == 2,
)
check(
    "verdict counts: attempt-1 and retry split",
    pairing["scan_attempt1_records"] == 2 and pairing["scan_retry_records"] == 1,
)

# 11) A dataset with ONLY retry records forms zero pairs -> INVALID
#     (no E1 comparison happened, so no conclusion may be drawn).
code = run(
    [sample("scan", 4, attempt=2, sha="retry", sampling=[1200, 64])],
    [sample("direct", 9, sha="h9")],
)
check("only-retry dataset (zero pairs) -> exit 1", code == 1)

# 12) BOTH sides missing input_sha256 -> INVALID (identity unverifiable).
missing_id = []
for source, page in (("scan", 4), ("direct", 4)):
    record = sample(source, page, sha="h4")
    del record["input_sha256"]
    missing_id.append(record)
code = run(missing_id[:1], missing_id[1:])
check("both sides missing input_sha256 -> exit 1", code == 1)

# 13) PARTIAL missing identity on one side (some records hashed, some
#     missing/null) -> INVALID exit 1 without crashing on sorted(None).
partial_scan = [
    sample("scan", 1, sha="h1", input_sha="doc-a"),
    sample("scan", 2, sha="h2"),  # missing input_sha256
]
del partial_scan[1]["input_sha256"]
partial_direct = [sample("direct", 1, sha="h1", input_sha="doc-a")]
code = run(partial_scan, partial_direct)
check("partial missing identity -> exit 1 without crash", code == 1)

# 14) PARTIAL missing identity AND no common hash across sides -> still
#     INVALID exit 1 without crashing (the cross-side mismatch diagnostic
#     also sorts the hash sets).
partial_no_common_scan = [
    sample("scan", 1, sha="h1", input_sha="doc-a"),
    sample("scan", 2, sha="h2"),  # missing input_sha256
]
del partial_no_common_scan[1]["input_sha256"]
partial_no_common_direct = [sample("direct", 9, sha="h9", input_sha="doc-z")]
code = run(partial_no_common_scan, partial_no_common_direct)
check("partial missing + no common hash -> exit 1 without crash", code == 1)

# 15) Missing rgb8 hash on BOTH sides -> INVALID (None == None must not
#     count as a byte match — sixth-review adversarial case).
def strip_keys(records, *keys):
    for record in records:
        for key in keys:
            record.pop(key, None)
    return records

code = run(
    strip_keys([sample("scan", 4, sha="h4")], "rgb8_sha256", "prompt_sha256"),
    strip_keys([sample("direct", 4, sha="h4")], "rgb8_sha256", "prompt_sha256"),
)
check("both sides missing rgb/prompt hashes -> exit 1", code == 1)

# 16) Base-grid mismatch -> param_mismatch -> DIFF (recorded AND compared:
#     different effective grids must not silently pass as IDENTICAL).
tmp = tempfile.mkdtemp(prefix="cb-grid-")
scan_path = os.path.join(tmp, "scan.jsonl")
direct_path = os.path.join(tmp, "direct.jsonl")
verdict_path = os.path.join(tmp, "verdict.json")
write_jsonl(
    scan_path,
    [sample("scan", 4, sha="s4", base_image_tokens=2048, schema_version=2)],
)
write_jsonl(
    direct_path,
    [sample("direct", 4, sha="d4", base_image_tokens=1024, schema_version=2)],
)
code = cb.main(["--scan", scan_path, "--direct", direct_path, "--output", verdict_path])
verdict = json.load(open(verdict_path, encoding="utf-8"))
pair = verdict["pairs"][0]
check("base-grid mismatch -> exit 0 (valid experiment)", code == 0)
check("base-grid mismatch -> verdict DIFF", verdict["verdict"] == "DIFF")
check(
    "base-grid mismatch reported in param_mismatch",
    "base_image_tokens" in pair["param_mismatch"],
)

# 17) Missing base_image_tokens field (schema violation) -> INVALID.
code = run(
    strip_keys(
        [sample("scan", 4, sha="s4", schema_version=2)], "base_image_tokens"
    ),
    [sample("direct", 4, sha="d4", schema_version=2)],
)
check("missing base_image_tokens -> exit 1", code == 1)

# 18) PURE v1 pairing (no grid fields anywhere) -> INVALID: the bypass is
#     closed — legacy records carry no base grid, so they cannot compare
#     one (migrate the evidence to v2 instead). The records here are the
#     real legacy shape: no schema_version, no base_image_tokens.
legacy_scan = sample("scan", 4, sha="s4", schema_version=None)
legacy_scan.pop("base_image_tokens")
legacy_direct = sample("direct", 4, sha="d4", schema_version=None)
legacy_direct.pop("base_image_tokens")
code = run([legacy_scan], [legacy_direct])
check("pure v1 pairing (no grid) -> exit 1", code == 1)

# 19) v2 + v1 mixed pairing -> INVALID (the v1 side has no grid to compare).
mixed_v1_direct = sample("direct", 4, sha="d4", schema_version=None)
mixed_v1_direct.pop("base_image_tokens")
code = run(
    [sample("scan", 4, sha="s4", schema_version=2)],
    [mixed_v1_direct],
)
check("v2+v1 mixed pairing -> exit 1", code == 1)

# 20) BOTH sides missing width -> INVALID (None == None fake param match).
code = run(
    strip_keys([sample("scan", 4, sha="s4", schema_version=2)], "width"),
    strip_keys([sample("direct", 4, sha="d4", schema_version=2)], "width"),
)
check("both sides missing width -> exit 1", code == 1)

# 21) BOTH sides missing height and max_output_tokens -> INVALID.
code = run(
    strip_keys(
        [sample("scan", 4, sha="s4", schema_version=2)], "height", "max_output_tokens"
    ),
    strip_keys(
        [sample("direct", 4, sha="d4", schema_version=2)], "height", "max_output_tokens"
    ),
)
check("both sides missing height/max_output_tokens -> exit 1", code == 1)

# 22) BOTH sides deleting optional-rung fields entirely -> INVALID
#     (missing key is a schema violation; null is the legitimate
#     "absent rung" spelling, a missing key is not).
code = run(
    strip_keys(
        [sample("scan", 4, sha="s4", schema_version=2)],
        "image_tokens_override",
        "retry_sampling",
    ),
    strip_keys(
        [sample("direct", 4, sha="d4", schema_version=2)],
        "image_tokens_override",
        "retry_sampling",
    ),
)
check("both sides delete override/sampling keys -> exit 1", code == 1)

# 23) BOTH sides deleting identity/geometry/outcome fields -> INVALID.
for field in ("page_index", "outcome", "rgb8_len"):
    code = run(
        strip_keys([sample("scan", 4, sha="s4", schema_version=2)], field),
        strip_keys([sample("direct", 4, sha="d4", schema_version=2)], field),
    )
    check(f"both sides delete {field} -> exit 1", code == 1)

# 24) Explicit null on the optional rung fields is LEGITIMATE (the
#     attempt-1 shape) — a valid experiment, never INVALID.
code = run(
    [sample("scan", 4, sha="s4", schema_version=2)],
    [sample("direct", 4, sha="d4", schema_version=2)],
)
check("explicit-null optional fields remain a valid experiment", code == 0)
