#!/usr/bin/env python3
"""B0a (next-phase plan §4.2, BLK-5/D-32): freeze the detection-annotation
subset from the OmniDocBench GT — GT 单源确定性页规则, no B0b dependency.

Page set (frozen rule, one-click reproducible): every dataset page carrying
>= 1 GT `table` det or >= 1 GT `equation_isolated`/`equation_semantic` det.
The frozen artifact is the stem list + the GT region polys (per-class
ordinals for the RBAC class-ordinal pairing) + this script itself.

Outputs (deterministic, byte-stable):
  <output-dir>/region-truth.json        the frozen pages + regions + counts
  <output-dir>/region-truth-stems.txt   one selected image_path per line

  python scripts/build-omnidocbench-region-truth.py \
      --dataset benchmarks/omnidocbench/dataset/OmniDocBench.json \
      --mapping benchmarks/omnidocbench/pdfs/mapping.json \
      --output-dir benchmarks/omnidocbench/region-truth [--verify]

`--verify` re-derives the artifact from the GT and byte-compares it with
the frozen copy (真值一致性检查); any drift is a loud failure.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import sys
from pathlib import Path

SCHEMA_VERSION = 1

TABLE_CLASSES = ("table",)
EQUATION_CLASSES = ("equation_isolated", "equation_semantic")
REGION_CLASSES = TABLE_CLASSES + EQUATION_CLASSES
# Poly coordinates may touch the page edge; a small epsilon absorbs GT
# rounding. Out-of-bounds polys are COUNTED in the artifact, not fatal.
OOB_EPSILON = 1.0


class RegionTruthError(Exception):
    """Frozen-contract violation (malformed GT poly, mapping drift)."""


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def canonical_bytes(artifact: dict) -> bytes:
    """The byte-stable serialization (sorted keys, no incidental whitespace)."""
    return json.dumps(
        artifact, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def check_poly(poly, class_name: str, image_path: str) -> bool:
    """Validates one GT poly; returns whether it lies in page bounds."""
    if len(poly) != 8 or not all(
        isinstance(value, (int, float)) and math.isfinite(value) for value in poly
    ):
        raise RegionTruthError(
            f"{image_path}: {class_name} det has a malformed poly "
            f"(need 8 finite numbers, got {poly!r})"
        )
    xs = poly[0::2]
    ys = poly[1::2]
    return (
        min(xs) >= -OOB_EPSILON
        and min(ys) >= -OOB_EPSILON
        and max(xs) >= 0.0
        and max(ys) >= 0.0
        and min(xs) <= max(xs)
        and min(ys) <= max(ys)
    )


def build_region_truth(*, dataset: list, mapping: dict | None) -> dict:
    """Derives the frozen region-truth artifact from the GT (pure, tested)."""
    pages: list[dict] = []
    counts = {
        "pages": 0,
        "pages_with_table": 0,
        "pages_with_equation": 0,
        "table_regions": 0,
        "equation_regions": 0,
        "out_of_bounds_regions": 0,
    }
    stem_of = {}
    if mapping is not None:
        stem_of = {entry["image_path"]: entry["stem"] for entry in mapping["pages"]}
    for page in dataset:
        info = page["page_info"]
        image_path = info["image_path"]
        # Candidates keep their GT det index; regions are ordered by the GT
        # reading order (`order`, det index as tiebreak) BEFORE the per-class
        # ordinals are assigned — the ordinal IS the document-order pairing
        # key the RBAC contract uses.
        candidates = []
        has_table = False
        has_equation = False
        for det_index, det in enumerate(page.get("layout_dets", [])):
            class_name = det["category_type"]
            if class_name not in REGION_CLASSES:
                continue
            in_bounds = check_poly(det["poly"], class_name, image_path)
            if not in_bounds:
                counts["out_of_bounds_regions"] += 1
            candidates.append((det.get("order", 0), det_index, class_name, det))
            counts["table_regions" if class_name in TABLE_CLASSES else "equation_regions"] += 1
            has_table = has_table or class_name in TABLE_CLASSES
            has_equation = has_equation or class_name in EQUATION_CLASSES
        if not candidates:
            continue  # the frozen rule selects region-bearing pages only
        # GT `order` can be null (unordered det): nulls sort last, det index
        # breaks ties — deterministic on any GT payload.
        candidates.sort(
            key=lambda entry: (
                entry[0] is None,
                entry[0] if entry[0] is not None else 0,
                entry[1],
            )
        )
        regions = []
        class_seen: dict[str, int] = {}
        for order, _det_index, class_name, det in candidates:
            ordinal = class_seen.get(class_name, 0)
            class_seen[class_name] = ordinal + 1
            regions.append(
                {
                    "class": class_name,
                    "index_in_class": ordinal,
                    "order": order,
                    "poly": list(det["poly"]),
                    "anno_id": det.get("anno_id", ""),
                }
            )
        if mapping is not None and image_path not in stem_of:
            raise RegionTruthError(
                f"{image_path}: selected by the GT rule but missing from "
                "mapping.json (denominator drift — regenerate the mapping)"
            )
        counts["pages"] += 1
        counts["pages_with_table"] += int(has_table)
        counts["pages_with_equation"] += int(has_equation)
        pages.append(
            {
                "image_path": image_path,
                "stem": stem_of.get(image_path, ""),
                "width": info["width"],
                "height": info["height"],
                "regions": regions,
            }
        )
    pages.sort(key=lambda page: page["image_path"])
    return {
        "schema_version": SCHEMA_VERSION,
        "artifact": "omnidocbench-region-truth",
        "rule": {
            "table_classes": list(TABLE_CLASSES),
            "equation_classes": list(EQUATION_CLASSES),
            "selection": ">=1 det of any listed class (GT single source, D-32)",
            "ordinal": "index_in_class = document-order ordinal within its class on the page (RBAC class-ordinal pairing key)",
        },
        "counts": counts,
        "pages": pages,
    }


def write_outputs(artifact: dict, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "region-truth.json").write_bytes(canonical_bytes(artifact))
    stems = "\n".join(page["image_path"] for page in artifact["pages"]) + "\n"
    (out_dir / "region-truth-stems.txt").write_text(stems, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--mapping")
    parser.add_argument("--output-dir", required=True)
    parser.add_argument(
        "--verify",
        action="store_true",
        help="re-derive and byte-compare against the frozen copy, write nothing",
    )
    args = parser.parse_args()
    dataset_path = Path(args.dataset)
    mapping = load_json(Path(args.mapping)) if args.mapping else None
    artifact = build_region_truth(
        dataset=load_json(dataset_path), mapping=mapping
    )
    artifact["dataset_sha256"] = sha256_file(dataset_path)
    out_dir = Path(args.output_dir)
    if args.verify:
        frozen = (out_dir / "region-truth.json").read_bytes()
        if frozen != canonical_bytes(artifact):
            print(
                "region-truth verify FAILED: the frozen artifact drifted from "
                "the GT derivation (dataset change or manual edit)",
                file=sys.stderr,
            )
            raise SystemExit(2)
        stems = (out_dir / "region-truth-stems.txt").read_text(encoding="utf-8")
        expected_stems = (
            "\n".join(page["image_path"] for page in artifact["pages"]) + "\n"
        )
        if stems != expected_stems:
            print(
                "region-truth verify FAILED: stems file drifted from the "
                "frozen page list",
                file=sys.stderr,
            )
            raise SystemExit(2)
        print(
            f"region-truth verify OK: {artifact['counts']['pages']} pages, "
            f"{artifact['counts']['table_regions']} table regions, "
            f"{artifact['counts']['equation_regions']} equation regions"
        )
        return
    write_outputs(artifact, out_dir)
    print(
        f"region-truth frozen: {artifact['counts']['pages']} pages "
        f"(table {artifact['counts']['pages_with_table']}, equation "
        f"{artifact['counts']['pages_with_equation']}), "
        f"{artifact['counts']['table_regions']} table + "
        f"{artifact['counts']['equation_regions']} equation regions, "
        f"{artifact['counts']['out_of_bounds_regions']} out-of-bounds polys "
        f"recorded -> {out_dir / 'region-truth.json'}"
    )


if __name__ == "__main__":
    try:
        main()
    except RegionTruthError as error:
        print(f"region-truth error: {error}", file=sys.stderr)
        raise SystemExit(2) from error
