#!/usr/bin/env python3
"""T-PROJ paired-increment projection (next-phase plan §4.0, D-31/BLK-4).

The legacy `omnidocbench-projection.py` projects θ-scenarios from a single
profile's reports; it cannot express a PAIRED baseline-vs-candidate delta on
a frozen population. This tool is the paired mode the plan's adjudication
gates (B-M2.3 口径澄清, B2/B3/C1) consume:

Inputs
  - official all-granularity details ×3 (per-page text edit /
    per-sample CDM / per-table TEDS JSONs) — the frozen denominators
    (1,557 text pages / 2,352 formula samples / 665 table samples) and the
    full-set baseline values;
  - frozen baseline + candidate same-granularity details ×3 (subset-scope
    official evals; keys must join the official key space);
  - frozen population spec (stems for text pages, sample keys for
    tables/formulas);
  - output: per-component paired deltas + projected full-set component
    values + projected Overall and its delta (D-11 全集增量).

Overall arithmetic (frozen with the official scorer): text score =
1 − mean(edit); Overall = ((1 − text_edit_mean) + CDM + TEDS) / 3 × 100.

Projection: replace baseline values with candidate values on the population
keys, keep baseline values elsewhere, divide by the official denominators:

  projected_component = (Σ_all baseline + Σ_population (cand − base)) / N_all

Population keys outside the official details fail loudly (口径漂移);
candidate keys missing on the population fail loudly unless
--allow-missing-candidate (then they contribute zero delta and are counted).

Usage:
  python scripts/omnidocbench-paired-projection.py --population pop.json \
      --official-text ... --official-cdm ... --official-teds ... \
      --baseline-text ... --candidate-text ... [...] [--output out.json]
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCHEMA_VERSION = 1


class ProjectionError(Exception):
    """Frozen-contract violation (population/key drift, missing inputs)."""


def load_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ProjectionError(f"cannot read {path}: {error}") from error


def ted_value(entry) -> float:
    """Accepts the official per-table shape {'TEDS': x, ...} or a bare float."""
    if isinstance(entry, dict):
        return float(entry["TEDS"])
    return float(entry)


def component_projection(
    *,
    component: str,
    official: dict[str, float],
    baseline: dict[str, float],
    candidate: dict[str, float],
    population_keys: list[str],
    allow_missing_candidate: bool,
) -> dict:
    """One component's paired projection (see module docstring)."""
    official_n = len(official)
    if official_n == 0:
        raise ProjectionError(f"{component}: official details are empty")
    for key in baseline:
        if key not in official:
            raise ProjectionError(
                f"{component}: baseline key {key!r} is outside the official "
                "details (denominator drift — freeze the population against "
                "the official all-granularity export)"
            )
    paired_deltas: list[float] = []
    missing_candidate_n = 0
    delta_sum = 0.0
    for key in population_keys:
        if key not in official:
            raise ProjectionError(
                f"{component}: population key {key!r} is not an official "
                "all-granularity key"
            )
        base = baseline[key]
        if key not in candidate:
            missing_candidate_n += 1
            if not allow_missing_candidate:
                raise ProjectionError(
                    f"{component}: candidate details miss population key "
                    f"{key!r} (pass --allow-missing-candidate to treat it "
                    "as no-change)"
                )
            continue
        delta = candidate[key] - base
        paired_deltas.append(delta)
        delta_sum += delta
    baseline_all_sum = sum(official.values())
    projected_all_mean = (baseline_all_sum + delta_sum) / official_n
    official_all_mean = baseline_all_sum / official_n
    paired_mean = (
        sum(paired_deltas) / len(paired_deltas) if paired_deltas else 0.0
    )
    entry = {
        "official_n": official_n,
        "population_n": len(population_keys),
        "paired_n": len(paired_deltas),
        "missing_candidate_n": missing_candidate_n,
        "official_all_mean": official_all_mean,
        "projected_all_mean": projected_all_mean,
        "paired_mean_delta": paired_mean,
        "population_delta_sum": delta_sum,
    }
    if component == "text":
        # The official text component is 1 − mean(edit distance).
        entry["official_component"] = 1.0 - official_all_mean
        entry["projected_component"] = 1.0 - projected_all_mean
        entry["delta_component"] = official_all_mean - projected_all_mean
    else:
        entry["official_component"] = official_all_mean
        entry["projected_component"] = projected_all_mean
        entry["delta_component"] = projected_all_mean - official_all_mean
    return entry


def project(
    *,
    official_text: dict[str, float],
    official_cdm: dict[str, float],
    official_teds: dict[str, float],
    baseline_text: dict[str, float],
    baseline_cdm: dict[str, float],
    baseline_teds: dict[str, float],
    candidate_text: dict[str, float],
    candidate_cdm: dict[str, float],
    candidate_teds: dict[str, float],
    population: dict,
    allow_missing_candidate: bool = False,
) -> dict:
    """Full three-component paired projection (library entry, unit-tested)."""
    normalize = lambda details: {  # noqa: E731 - TEDS dicts enter both paths
        key: ted_value(value) for key, value in details.items()
    }
    text = component_projection(
        component="text",
        official=normalize(official_text),
        baseline=normalize(baseline_text),
        candidate=normalize(candidate_text),
        population_keys=list(population.get("text_pages", [])),
        allow_missing_candidate=allow_missing_candidate,
    )
    cdm = component_projection(
        component="cdm",
        official=normalize(official_cdm),
        baseline=normalize(baseline_cdm),
        candidate=normalize(candidate_cdm),
        population_keys=list(population.get("formulas", [])),
        allow_missing_candidate=allow_missing_candidate,
    )
    teds = component_projection(
        component="teds",
        official=normalize(official_teds),
        baseline=normalize(baseline_teds),
        candidate=normalize(candidate_teds),
        population_keys=list(population.get("tables", [])),
        allow_missing_candidate=allow_missing_candidate,
    )
    components = {"text": text, "cdm": cdm, "teds": teds}
    official_overall = (
        (text["official_component"] + cdm["official_component"] + teds["official_component"])
        / 3
        * 100
    )
    projected_overall = (
        (
            text["projected_component"]
            + cdm["projected_component"]
            + teds["projected_component"]
        )
        / 3
        * 100
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "tool": "omnidocbench-paired-projection",
        "population_id": population.get("population_id", ""),
        "components": components,
        "overall": {
            "official": official_overall,
            "projected": projected_overall,
            "delta": projected_overall - official_overall,
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in (
        "official-text",
        "official-cdm",
        "official-teds",
        "baseline-text",
        "baseline-cdm",
        "baseline-teds",
        "candidate-text",
        "candidate-cdm",
        "candidate-teds",
        "population",
    ):
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--allow-missing-candidate", action="store_true")
    parser.add_argument("--output")
    args = parser.parse_args()

    def load_ted_component(path: str) -> dict[str, float]:
        raw = load_json(Path(path))
        return {key: ted_value(value) for key, value in raw.items()}

    official_cdm = load_json(Path(args.official_cdm))
    candidate_cdm = load_json(Path(args.candidate_cdm))
    baseline_cdm = load_json(Path(args.baseline_cdm))
    if not isinstance(official_cdm, dict) or not isinstance(candidate_cdm, dict) or not isinstance(baseline_cdm, dict):
        raise ProjectionError("CDM details must be JSON objects")
    result = project(
        official_text=load_json(Path(args.official_text)),
        official_cdm=official_cdm,
        official_teds=load_ted_component(args.official_teds),
        baseline_text=load_json(Path(args.baseline_text)),
        baseline_cdm=baseline_cdm,
        baseline_teds=load_ted_component(args.baseline_teds),
        candidate_text=load_json(Path(args.candidate_text)),
        candidate_cdm=candidate_cdm,
        candidate_teds=load_ted_component(args.candidate_teds),
        population=load_json(Path(args.population)),
        allow_missing_candidate=args.allow_missing_candidate,
    )

    rendered = json.dumps(result, ensure_ascii=False, indent=2)
    if args.output:
        Path(args.output).write_text(rendered + "\n", encoding="utf-8")
    for component, entry in result["components"].items():
        print(
            f"[{component}] n={entry['official_n']} pop={entry['population_n']} "
            f"paired={entry['paired_n']} missing={entry['missing_candidate_n']} "
            f"pairedΔ={entry['paired_mean_delta']:+.6f} "
            f"{entry['official_component']:.6f}→{entry['projected_component']:.6f} "
            f"(Δ {entry['delta_component']:+.6f})"
        )
    overall = result["overall"]
    print(
        f"[Overall] official={overall['official']:.4f} "
        f"projected={overall['projected']:.4f} "
        f"delta={overall['delta']:+.4f}"
    )


if __name__ == "__main__":
    try:
        main()
    except ProjectionError as error:
        print(f"projection error: {error}", file=sys.stderr)
        sys.exit(2)
