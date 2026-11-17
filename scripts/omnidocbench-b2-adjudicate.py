#!/usr/bin/env python3
"""R3 B2 five-gate adjudication (m4-table-reopen plan 3.4, D-22/D-46).

Gates (frozen text):
  ① pool-table trigger coverage on SAFE-ELIGIBLE pages >= 80%
     (denominator = the worst/mid/bad-low pool tables on pairing-PASS
     pages; absolute pool coverage is reported as projection input);
  ② normal-page final replacement rate <= 2% (denominator = table
     samples on normal pages under the CONTROL config);
  ③ good-band (en+zh+other) per-table paired dTEDS mean >= -0.02 AND
     regressions (d < -0.10) <= max(3, 2% of the paired good tables);
  ④ worst/bad-band (worst+bad-low) mean dTEDS >= +0.3 on the paired
     safe-eligible subset;
  ⑤ T-PROJ full-set projection recorded + control normal-page
     checkpoint zero-drift against the pre-M4 baseline scan.

Pairing key normalization: corpus keys are `<image_path>_[<ordinal>]`
(the GT image extension varies); eval per-table keys use the ORIGINAL GT
extension — both normalize to `<stem>_[<ordinal>]`.

  python scripts/omnidocbench-b2-adjudicate.py \
      --corpus benchmarks/omnidocbench/b0b-corpus/b0b-corpus.json \
      --pairing <analysis-pairing-regions-mode.json> \
      --control-scans <scans/control> --target-scans <scans/target> \
      --baseline-scans <scans/m23-m2rung-balanced> \
      --control-teds <control_per_table_TEDS.json> \
      --target-teds <target_per_table_TEDS.json> \
      --output <analysis-b2-adjudication.json>
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

GATES = ("coverage", "replacement", "good_band", "worst_bad", "projection_drift")
POOL_BANDS = ("worst", "mid", "bad-low")
GOOD_BANDS = ("good-en", "good-zh", "good-other")
WORST_BAD_BANDS = ("worst", "bad-low")
TRIGGER_COVERAGE_MIN = 0.80
REPLACEMENT_RATE_MAX = 0.02
GOOD_BAND_MEAN_MIN = -0.02
GOOD_BAND_REGRESSION_TEDS = -0.10
GOOD_BAND_REGRESSION_COUNT_MAX = 3
WORST_BAD_DELTA_MIN = 0.30


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def normalize_key(corpus_key: str) -> str:
    """`<stem>.<ext>_[<ordinal>]` -> `<stem>_[<ordinal>]`."""
    head, sep, ordinal = corpus_key.rpartition("_[")
    stem = Path(head).stem if sep else Path(corpus_key).stem
    return f"{stem}_[{ordinal}" if sep else stem


def member_stem(member: dict) -> str:
    return Path(member["image_path"]).stem


def paired_pages(pairing: dict) -> set[str]:
    return {
        Path(entry["image_path"]).stem
        for entry in pairing.get("per_page", [])
        if entry.get("verdict") == "pair"
    }


def doc_dir(scans_dir: Path, stem: str) -> Path | None:
    """The document output dir: `<stem>-<input-hash suffix>` (the
    orchestrated mode appends the input hash); falls back to `<stem>`."""
    direct = scans_dir / stem
    if (direct / "result.json").is_file():
        return direct
    for candidate in sorted(scans_dir.glob(f"{stem}-*")):
        if (candidate / "result.json").is_file():
            return candidate
    return None


def doc_reread_counters(scans_dir: Path, stem: str) -> dict:
    """Per-document reread counters from result.json (doc = one page)."""
    doc = doc_dir(scans_dir, stem)
    result_path = doc / "result.json" if doc else None
    if result_path is None or not result_path.is_file():
        return {"found": False, "tables_reread": 0, "replaced": 0}
    result = load_json(result_path)
    reread = result.get("reread", {})
    pages = result.get("pages", {})
    return {
        "found": True,
        "tables_reread": int(pages.get("tables_reread_total", 0) or 0),
        "replaced": int(pages.get("replaced_total", 0) or 0),
        "enabled": bool(reread.get("enabled")),
        "prompt_version": reread.get("prompt_version", ""),
        "rules_generation": reread.get("rules_generation", 0),
    }


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def zero_drift(control_scans: Path, baseline_scans: Path, stems: list[str]) -> dict:
    """Gate ⑤ second leg: control-run page json/md byte-identical to the
    pre-M4 baseline scan (the off path never drifts)."""
    checked, drifted, missing = 0, [], 0
    for stem in stems:
        control_doc = doc_dir(control_scans, stem)
        baseline_doc = doc_dir(baseline_scans, stem)
        if control_doc is None or baseline_doc is None:
            missing += 1
            continue
        for name in ("page-0001.json", "page-0001.md"):
            control_file = control_doc / name
            baseline_file = baseline_doc / name
            if not control_file.is_file() or not baseline_file.is_file():
                missing += 1
                continue
            checked += 1
            if file_sha256(control_file) != file_sha256(baseline_file):
                drifted.append(f"{stem}/{name}")
    return {"checked": checked, "missing": missing, "drifted": drifted,
            "pass": checked > 0 and not drifted}


def adjudicate(
    corpus: dict,
    pairing: dict,
    control_teds: dict,
    target_teds: dict,
    control_scans: Path,
    target_scans: Path,
    baseline_scans: Path,
) -> dict:
    safe_pages = paired_pages(pairing)
    members_by_band = corpus["tables"]

    pool_members = [
        member
        for band in POOL_BANDS
        for member in members_by_band[band]
        if member_stem(member) in safe_pages
    ]
    triggered = [
        member
        for member in pool_members
        if doc_reread_counters(target_scans, member_stem(member))["tables_reread"] > 0
    ]
    absolute_pool = [
        member
        for band in POOL_BANDS
        for member in members_by_band[band]
    ]
    coverage = {
        "gate": 1,
        "rule": "pool tables on safe-eligible pages triggered >= 80%",
        "pool_tables_safe_eligible": len(pool_members),
        "triggered": len(triggered),
        "coverage": (len(triggered) / len(pool_members)) if pool_members else 0.0,
        "absolute_pool_tables": len(absolute_pool),
        "absolute_coverage": (len(triggered) / len(absolute_pool)) if absolute_pool else 0.0,
    }
    coverage["pass"] = coverage["coverage"] >= TRIGGER_COVERAGE_MIN

    normal_members = [
        member
        for band in members_by_band
        for member in members_by_band[band]
        if not member.get("page_bad")
    ]
    replaced = sum(
        doc_reread_counters(target_scans, member_stem(member))["replaced"]
        for member in normal_members
    )
    replacement = {
        "gate": 2,
        "rule": "normal-page replacements <= 2% of normal-page table samples",
        "normal_page_table_samples": len(normal_members),
        "replaced": replaced,
        "rate": (replaced / len(normal_members)) if normal_members else 0.0,
    }
    replacement["pass"] = replacement["rate"] <= REPLACEMENT_RATE_MAX

    def paired_delta(bands: tuple[str, ...]) -> list[dict]:
        pairs = []
        for band in bands:
            for member in members_by_band[band]:
                key = normalize_key(member["key"])
                control_value = control_teds.get(key, {}).get("TEDS")
                target_value = target_teds.get(key, {}).get("TEDS")
                if control_value is None or target_value is None:
                    continue
                pairs.append({
                    "key": key,
                    "band": band,
                    "control": control_value,
                    "target": target_value,
                    "delta": target_value - control_value,
                })
        return pairs

    good_pairs = paired_delta(GOOD_BANDS)
    regressions = [p for p in good_pairs if p["delta"] < GOOD_BAND_REGRESSION_TEDS]
    good_mean = sum(p["delta"] for p in good_pairs) / len(good_pairs) if good_pairs else None
    good_band = {
        "gate": 3,
        "rule": "good-band paired mean dTEDS >= -0.02 AND regressions (d < -0.10) <= max(3, 2%)",
        "paired_tables": len(good_pairs),
        "mean_delta": good_mean,
        "regressions": len(regressions),
        "regression_cap": max(GOOD_BAND_REGRESSION_COUNT_MAX,
                              int(0.02 * len(good_pairs)) if good_pairs else 0),
        "regression_keys": [p["key"] for p in regressions],
    }
    good_band["pass"] = (
        good_mean is not None
        and good_mean >= GOOD_BAND_MEAN_MIN
        and len(regressions) <= good_band["regression_cap"]
    )

    worst_bad_pairs = [
        p for p in paired_delta(WORST_BAD_BANDS)
        if Path(p["key"].rpartition("_[")[0]).stem in safe_pages
    ]
    worst_bad_mean = (
        sum(p["delta"] for p in worst_bad_pairs) / len(worst_bad_pairs)
        if worst_bad_pairs else None
    )
    worst_bad = {
        "gate": 4,
        "rule": "worst/bad-band mean dTEDS >= +0.3 (paired safe-eligible subset)",
        "paired_tables": len(worst_bad_pairs),
        "mean_delta": worst_bad_mean,
        "deltas": {p["key"]: round(p["delta"], 4) for p in worst_bad_pairs},
    }
    worst_bad["pass"] = (
        worst_bad_mean is not None and worst_bad_mean >= WORST_BAD_DELTA_MIN
    )

    stems = sorted({member_stem(member) for members in members_by_band.values() for member in members})
    drift = zero_drift(control_scans, baseline_scans, stems)
    projection_leg = {"recorded": False}
    projection_drift = {
        "gate": 5,
        "rule": "T-PROJ full-set projection recorded AND control normal-page checkpoint zero-drift",
        "zero_drift": drift,
        "projection": projection_leg,
    }
    projection_drift["pass"] = drift["pass"] and projection_leg["recorded"]

    gates = {
        "coverage": coverage,
        "replacement": replacement,
        "good_band": good_band,
        "worst_bad": worst_bad,
        "projection_drift": projection_drift,
    }
    all_pass = all(gates[name]["pass"] for name in GATES)
    return {
        "schema_version": 1,
        "artifact": "omnidocbench-b2-adjudication",
        "rule": "m4-table-reopen plan 3.4 five gates (D-22/D-46)",
        "safe_eligible_pages": len(safe_pages),
        "gates": gates,
        "verdict": "GO" if all_pass else "NO-GO",
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--pairing", type=Path, required=True)
    parser.add_argument("--control-scans", type=Path, required=True)
    parser.add_argument("--target-scans", type=Path, required=True)
    parser.add_argument("--baseline-scans", type=Path, required=True)
    parser.add_argument("--control-teds", type=Path, required=True)
    parser.add_argument("--target-teds", type=Path, required=True)
    parser.add_argument("--projection-recorded", action="store_true",
                        help="set when the T-PROJ artifact has been produced")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)

    verdict = adjudicate(
        load_json(args.corpus),
        load_json(args.pairing),
        load_json(args.control_teds),
        load_json(args.target_teds),
        args.control_scans,
        args.target_scans,
        args.baseline_scans,
    )
    if args.projection_recorded:
        verdict["gates"]["projection_drift"]["projection"]["recorded"] = True
        verdict["gates"]["projection_drift"]["pass"] = (
            verdict["gates"]["projection_drift"]["zero_drift"]["pass"]
        )
    verdict["verdict"] = "GO" if all(
        verdict["gates"][name]["pass"] for name in GATES
    ) else "NO-GO"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(verdict, ensure_ascii=False, indent=2), encoding="utf-8")
    summary = {
        name: {
            "pass": verdict["gates"][name]["pass"],
            **{k: v for k, v in verdict["gates"][name].items()
               if k in ("coverage", "rate", "mean_delta", "paired_tables", "regressions")},
        }
        for name in GATES
    }
    print(json.dumps({"verdict": verdict["verdict"], "gates": summary}, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
