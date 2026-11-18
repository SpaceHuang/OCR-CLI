#!/usr/bin/env python3
"""R5 giveback localization (m4-table-reopen plan; D-43 analysis-only).

Where does the landing Overall lose mass against the 89.5 gate? For each
component (text / CDM / TEDS) compute the deficit decomposition over the
frozen official populations, bucket the loss by threshold bands, and list
the top deficit carriers. ANALYSIS ONLY: this artifact informs expectations
and future work; it changes no behavior and no baseline (D-43).

  python scripts/omnidocbench-giveback-localization.py \
      --text <per_page_edit.json> --cdm <per_sample_CDM.json> \
      --teds <per_table_TEDS.json> \
      [--table-bands <b0b-corpus.json>] \
      --gate 89.5 --output <analysis-giveback.json>
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

GATE_DEFAULT = 89.5


def load_json(path: Path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def component_analysis(values: dict[str, float], score_of, bands: list[tuple[str, float]]) -> dict:
    entries = [{"key": key, "score": score_of(value)} for key, value in values.items()]
    n = len(entries)
    mean = sum(e["score"] for e in entries) / n if n else 0.0
    deficit_total = sum(1.0 - e["score"] for e in entries)
    ordered = sorted(entries, key=lambda e: e["score"])
    buckets = {}
    remaining = entries
    lower = 0.0
    for name, upper in bands:
        in_band = [e for e in remaining if e["score"] < upper]
        bucket_deficit = sum(1.0 - e["score"] for e in in_band)
        buckets[name] = {
            "threshold_lt": upper,
            "count": len(in_band),
            "share_of_population": len(in_band) / n if n else 0.0,
            "deficit": bucket_deficit,
            "share_of_deficit": bucket_deficit / deficit_total if deficit_total else 0.0,
        }
        remaining = [e for e in remaining if e["score"] >= upper]
        lower = upper
    tail = remaining
    buckets["tail"] = {
        "threshold_lt": None,
        "count": len(tail),
        "share_of_population": len(tail) / n if n else 0.0,
        "deficit": sum(1.0 - e["score"] for e in tail),
        "share_of_deficit": (sum(1.0 - e["score"] for e in tail) / deficit_total) if deficit_total else 0.0,
    }
    return {
        "n": n,
        "mean_score": mean,
        "deficit_total": deficit_total,
        "buckets": buckets,
        "top20_deficit": [
            {"key": e["key"], "score": round(e["score"], 4)}
            for e in ordered[:20]
        ],
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--text", type=Path, required=True)
    parser.add_argument("--cdm", type=Path, required=True)
    parser.add_argument("--teds", type=Path, required=True)
    parser.add_argument("--table-bands", type=Path, default=None,
                        help="b0b-corpus.json: attach the frozen band to each table")
    parser.add_argument("--gate", type=float, default=GATE_DEFAULT)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)

    text_raw = load_json(args.text)
    cdm_raw = load_json(args.cdm)
    teds_raw = load_json(args.teds)

    band_of = {}
    if args.table_bands:
        corpus = load_json(args.table_bands)
        for band, members in corpus["tables"].items():
            for member in members:
                band_of[member["key"]] = band

    text = component_analysis(
        {k: v for k, v in text_raw.items()},
        score_of=lambda v: 1.0 - float(v),
        bands=[("lt_0.5", 0.5), ("lt_0.85", 0.85)],
    )
    cdm = component_analysis(
        {k: (v if isinstance(v, (int, float)) else float(v.get("CDM", 0.0)))
         for k, v in cdm_raw.items()},
        score_of=lambda v: float(v),
        bands=[("lt_0.5", 0.5), ("lt_0.85", 0.85)],
    )
    teds_entries = {
        k: (v["TEDS"] if isinstance(v, dict) else float(v))
        for k, v in teds_raw.items()
    }
    teds = component_analysis(
        teds_entries,
        score_of=lambda v: float(v),
        bands=[("lt_0.5", 0.5), ("lt_0.85", 0.85)],
    )
    if band_of:
        band_deficit = {}
        for key, value in teds_entries.items():
            band = band_of.get(key, "outside-b0b")
            band_deficit.setdefault(band, {"count": 0, "deficit": 0.0})
            band_deficit[band]["count"] += 1
            band_deficit[band]["deficit"] += 1.0 - float(value)
        total = sum(b["deficit"] for b in band_deficit.values()) or 1.0
        for stats in band_deficit.values():
            stats["share_of_deficit"] = stats["deficit"] / total
        teds["deficit_by_b0b_band"] = band_deficit

    # The artifact stores per-page EDIT DISTANCES; the official text
    # component = 1 - mean(edit).
    text_component = 1.0 - (sum(float(e) for e in text_raw.values()) / len(text_raw))
    cdm_component = sum(float(v if isinstance(v, (int, float)) else v.get("CDM", 0.0))
                        for v in cdm_raw.values()) / len(cdm_raw)
    teds_component = sum(teds_entries.values()) / len(teds_entries)
    overall = (text_component + cdm_component + teds_component) / 3 * 100

    result = {
        "schema_version": 1,
        "artifact": "omnidocbench-giveback-localization",
        "note": "D-43 analysis-only; no behavior or baseline change",
        "gate": args.gate,
        "components": {
            "text": {"component_score": text_component, "analysis": text},
            "cdm": {"component_score": cdm_component, "analysis": cdm},
            "teds": {"component_score": teds_component, "analysis": teds},
        },
        "overall": {
            "landing": overall,
            "gate": args.gate,
            "gap_to_gate": overall - args.gate,
            "per_component_deficit_vs_perfect": {
                "text": 1.0 - text_component,
                "cdm": 1.0 - cdm_component,
                "teds": 1.0 - teds_component,
            },
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    summary = {
        "landing_overall": round(overall, 2),
        "gap_to_gate": round(result["overall"]["gap_to_gate"], 2),
        "text": round(text_component, 4),
        "cdm": round(cdm_component, 4),
        "teds": round(teds_component, 4),
        "teds_buckets": {k: v["count"] for k, v in teds["buckets"].items()},
        "cdm_buckets": {k: v["count"] for k, v in cdm["buckets"].items()},
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
