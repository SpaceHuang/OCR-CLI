#!/usr/bin/env python3
"""B0b (next-phase plan §4.2): the full experiment-corpus freeze.

Page set reuses the B0a frozen region-truth list — no page selection of its
own (D-32). Members are the OFFICIAL sample keys (`<image_path>_[<idx>]`)
from the landing all-granularity details — the toolkit's own key space is
the frozen denominator (665 tables / 2,352 formulas), so no det-level key
reverse-engineering is involved; page attributes (language, source, ledger
badness) join by image-path prefix.

Frozen content:

  tables    ALL 665 official table samples banded by the landing per-table
            TEDS on GOOD pages (worst < 0.5 / mid 0.5..<0.85 / good-en
            >= 0.85 english / good-zh >= 0.85 simplified_chinese /
            good-other the rest) and by the bad-page ledger (bad-low /
            bad-dropped = missing baseline);
  formulas  fracture subset = good pages with landing CDM < 0.5 + ALL
            bad-page formulas (the B3 门① population), each member carrying
            its landing per-sample CDM baseline and origin tag;
            protected set = seeded stratified en/zh sample of good-page
            CDM >= 0.5 formulas, >= --protected-target members, fracture
            excluded (the B3 门③ population);
            formula page list = pages carrying >= 1 official CDM key (the
            E2 313-page corpus reference);
  bad_pages the frozen ledger (stems, landing export report).

Every member page must appear in the B0a region-truth stems and in the GT
dataset (denominator integrity, both loud). Canonical byte-stable output;
`--verify` re-derives and byte-compares.

  python scripts/omnidocbench-freeze-b0b-corpus.py \
      --dataset benchmarks/omnidocbench/dataset/OmniDocBench.json \
      --landing-teds reports/omnidocbench-q13-landing-all_quick_match_table_per_table_TEDS.json \
      --landing-cdm reports/omnidocbench-q13-landing-all_quick_match_display_formula_per_sample_CDM.json \
      --export-report reports/q13-landing-full-balanced-export-report.json \
      --region-truth benchmarks/omnidocbench/region-truth/region-truth.json \
      --output-dir benchmarks/omnidocbench/b0b-corpus [--verify]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import random
import sys
from pathlib import Path

SCHEMA_VERSION = 1
SEED_DEFAULT = 20261114
PROTECTED_TARGET_DEFAULT = 40
TABLE_WORST_BELOW = 0.5
TABLE_GOOD_AT = 0.85
FRACTURE_CDM_BELOW = 0.5
LANG_EN = "english"
LANG_ZH = "simplified_chinese"


class B0bError(Exception):
    """Frozen-contract violation (B0a drift, key/page mismatch)."""


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def canonical_bytes(artifact: dict) -> bytes:
    return json.dumps(
        artifact, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def split_key(key: str) -> tuple[str, str]:
    """'<image-path>_[idx]' -> (image-path, stem)."""
    image_path = key.rsplit("_[", 1)[0]
    return image_path, image_path.rsplit(".", 1)[0]


def build_b0b_corpus(
    *,
    dataset: list,
    landing_teds: dict,
    landing_cdm: dict,
    export_report: dict,
    region_truth: dict,
    seed: int = SEED_DEFAULT,
    protected_target: int = PROTECTED_TARGET_DEFAULT,
) -> dict:
    """Derives the frozen B0b artifact (pure, unit-tested)."""
    b0a_pages = {page["image_path"] for page in region_truth["pages"]}
    bad_stems = {
        page["stem"]
        for page in export_report["pages"]
        if page.get("degenerated") or page.get("truncated")
    }
    gt_by_image = {item["page_info"]["image_path"]: item for item in dataset}

    def page_attributes(image_path: str) -> tuple[bool, str, str]:
        item = gt_by_image.get(image_path)
        if item is None:
            raise B0bError(
                f"{image_path}: official sample key names a page outside the "
                "GT dataset (key/GT drift)"
            )
        if image_path not in b0a_pages:
            raise B0bError(
                f"{image_path}: sample page outside the B0a frozen list "
                "(B0a/B0b drift — regenerate region-truth)"
            )
        attribute = item["page_info"]["page_attribute"]
        return (
            item["page_info"]["image_path"].rsplit(".", 1)[0] in bad_stems,
            attribute.get("language", ""),
            attribute.get("data_source", ""),
        )

    tables: dict[str, list] = {band: [] for band in (
        "worst", "mid", "good-en", "good-zh", "good-other", "bad-low", "bad-dropped",
    )}
    formulas_fracture: list[dict] = []
    pool_en: list[dict] = []
    pool_zh: list[dict] = []
    pool_other: list[dict] = []
    formula_pages: set[str] = set()
    counts = {
        "pages": 0,
        "table_samples": 0,
        "formula_samples": 0,
        "fracture_samples": 0,
        "protected_samples": 0,
        "formula_pages": 0,
        "bad_pages": 0,
    }
    seen_pages: set[str] = set()

    for key, entry in landing_teds.items():
        image_path, _stem = split_key(key)
        page_bad, lang, source = page_attributes(image_path)
        seen_pages.add(image_path)
        value = entry["TEDS"] if isinstance(entry, dict) else entry
        member = {
            "key": key,
            "image_path": image_path,
            "baseline": value,
            "lang": lang,
            "source": source,
            "page_bad": page_bad,
        }
        if page_bad:
            band = "bad-low" if value is not None else "bad-dropped"
        elif value is None:
            band = "bad-dropped"
        elif value < TABLE_WORST_BELOW:
            band = "worst"
        elif value < TABLE_GOOD_AT:
            band = "mid"
        elif lang == LANG_EN:
            band = "good-en"
        elif lang == LANG_ZH:
            band = "good-zh"
        else:
            band = "good-other"
        tables[band].append(member)
        counts["table_samples"] += 1

    for key, value in landing_cdm.items():
        image_path, _stem = split_key(key)
        page_bad, lang, source = page_attributes(image_path)
        seen_pages.add(image_path)
        formula_pages.add(image_path)
        member = {
            "key": key,
            "image_path": image_path,
            "baseline": value,
            "lang": lang,
            "source": source,
            "page_bad": page_bad,
        }
        counts["formula_samples"] += 1
        if page_bad:
            member["origin"] = "bad-page"
            formulas_fracture.append(member)
            counts["fracture_samples"] += 1
        elif value is not None and value < FRACTURE_CDM_BELOW:
            member["origin"] = "good-low-cdm"
            formulas_fracture.append(member)
            counts["fracture_samples"] += 1
        else:
            # protected-set pool: good page, CDM >= threshold (a missing
            # landing sample cannot prove health, so it never protects)
            (pool_en if lang == LANG_EN else pool_zh if lang == LANG_ZH else pool_other).append(member)

    # Protected set: seeded, stratified en/zh half/half (others fill gaps),
    # fracture members already excluded by construction.
    rng = random.Random(seed)
    rng.shuffle(pool_en)
    rng.shuffle(pool_zh)
    rng.shuffle(pool_other)
    half_en = protected_target // 2
    half_zh = protected_target - half_en
    protected = pool_en[:half_en] + pool_zh[:half_zh]
    if len(protected) < protected_target:
        rest = pool_en[half_en:] + pool_zh[half_zh:] + pool_other
        protected.extend(rest[: protected_target - len(protected)])
    if len(protected) < protected_target:
        raise B0bError(
            f"protected pool too small: want {protected_target}, have {len(protected)}"
        )
    counts["protected_samples"] = len(protected)
    counts["pages"] = len(seen_pages)
    counts["formula_pages"] = len(formula_pages)

    for band_members in tables.values():
        band_members.sort(key=lambda member: member["key"])
    formulas_fracture.sort(key=lambda member: (member["origin"], member["key"]))
    protected.sort(key=lambda member: member["key"])
    bad_pages = sorted(bad_stems)
    counts["bad_pages"] = len(bad_pages)

    return {
        "schema_version": SCHEMA_VERSION,
        "artifact": "omnidocbench-b0b-corpus",
        "seed": seed,
        "protected_target": protected_target,
        "rule": {
            "members": "official sample keys from the landing all-granularity details (the toolkit key space IS the denominator)",
            "table_bands": "good pages: worst <0.5 / mid 0.5..<0.85 / good-en >=0.85 english / good-zh >=0.85 simplified_chinese / good-other; bad ledger pages: bad-low / bad-dropped (missing baseline)",
            "fracture": "good pages landing CDM < 0.5 + ALL bad-page formulas (B3 gate-1 population)",
            "protected": "seeded stratified en/zh sample, good pages CDM >= 0.5, fracture excluded (B3 gate-3 population)",
            "page_set": "reuses the B0a frozen region-truth list (D-32)",
        },
        "counts": counts,
        "bad_pages": bad_pages,
        "tables": tables,
        "formulas": {
            "fracture": formulas_fracture,
            "protected": protected,
            "pages": sorted(formula_pages),
        },
    }


def write_outputs(artifact: dict, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "b0b-corpus.json").write_bytes(canonical_bytes(artifact))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--landing-teds", required=True)
    parser.add_argument("--landing-cdm", required=True)
    parser.add_argument("--export-report", required=True)
    parser.add_argument("--region-truth", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--seed", type=int, default=SEED_DEFAULT)
    parser.add_argument("--protected-target", type=int, default=PROTECTED_TARGET_DEFAULT)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    artifact = build_b0b_corpus(
        dataset=load_json(Path(args.dataset)),
        landing_teds=load_json(Path(args.landing_teds)),
        landing_cdm=load_json(Path(args.landing_cdm)),
        export_report=load_json(Path(args.export_report)),
        region_truth=load_json(Path(args.region_truth)),
        seed=args.seed,
        protected_target=args.protected_target,
    )
    counts = artifact["counts"]
    if args.verify:
        frozen = (Path(args.output_dir) / "b0b-corpus.json").read_bytes()
        if frozen != canonical_bytes(artifact):
            print(
                "b0b verify FAILED: the frozen corpus drifted from the "
                "re-derivation (inputs changed or manual edit)",
                file=sys.stderr,
            )
            raise SystemExit(2)
        print(
            f"b0b verify OK: {counts['pages']} pages, "
            f"{counts['table_samples']} table samples, "
            f"{counts['fracture_samples']} fracture + "
            f"{counts['protected_samples']} protected formulas"
        )
        return
    write_outputs(artifact, Path(args.output_dir))
    print(
        f"b0b frozen: {counts['pages']} pages, "
        f"{counts['table_samples']} table samples "
        f"({', '.join(f'{band}={len(members)}' for band, members in artifact['tables'].items() if members)}), "
        f"{counts['fracture_samples']} fracture + "
        f"{counts['protected_samples']} protected formulas, "
        f"{counts['formula_pages']} formula pages, "
        f"{counts['bad_pages']} bad pages -> "
        f"{Path(args.output_dir) / 'b0b-corpus.json'}"
    )


if __name__ == "__main__":
    try:
        main()
    except B0bError as error:
        print(f"b0b error: {error}", file=sys.stderr)
        raise SystemExit(2) from error
