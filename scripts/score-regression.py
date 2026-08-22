#!/usr/bin/env python3
"""Deterministic P0 scorer for normalized PageDocument truth/prediction files."""
import json
import re
import sys
import unicodedata
from difflib import SequenceMatcher
from pathlib import Path

def norm(value):
    value = unicodedata.normalize("NFC", value).replace("\r\n", "\n").replace("\r", "\n")
    return re.sub(r"\n{3,}", "\n\n", value).strip()

def formula_norm(value):
    """Ignore formatting whitespace when comparing equivalent LaTeX."""
    return re.sub(r"\s+", "", unicodedata.normalize("NFC", value))

def fuzzy_contains(expected, actual):
    if not expected or not actual:
        return False
    if expected in actual or actual in expected:
        return True
    return SequenceMatcher(None, expected, actual).ratio() >= 0.8

def text(page):
    return norm("\n".join(b.get("content", "") for b in page.get("blocks", []) if b.get("type") == "markdown"))

def distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        new = [i]
        for j, y in enumerate(b, 1):
            new.append(min(new[-1] + 1, row[j] + 1, row[j - 1] + (x != y)))
        row = new
    return row[-1]

def regions(page, scale_coordinates=False):
    values = []
    for block in page.get("blocks", []):
        if block.get("type") != "visual_region":
            continue
        bbox = block.get("bbox", {})
        scale = 1000 if scale_coordinates and any(float(bbox.get(k, 0)) > 1 for k in ("left", "top", "right", "bottom")) else 1
        values.append(tuple(float(bbox.get(k, 0)) / scale for k in ("left", "top", "right", "bottom")))
    return values

def bbox_iou(a, b):
    left, top = max(a[0], b[0]), max(a[1], b[1])
    right, bottom = min(a[2], b[2]), min(a[3], b[3])
    intersection = max(0.0, right - left) * max(0.0, bottom - top)
    area_a = max(0.0, a[2] - a[0]) * max(0.0, a[3] - a[1])
    area_b = max(0.0, b[2] - b[0]) * max(0.0, b[3] - b[1])
    return intersection / max(area_a + area_b - intersection, 1e-12)

def score_page(pred, truth):
    expected, actual = text(truth), text(pred)
    ned = distance(actual, expected) / max(len(expected), len(actual), 1)
    formulas = [formula_norm(b.get("latex", "")) for b in truth.get("blocks", []) if b.get("type") == "formula"]
    actual_formulas = [formula_norm(b.get("latex", "")) for b in pred.get("blocks", []) if b.get("type") == "formula"]
    formula_rate = sum(any(fuzzy_contains(item, actual) for actual in actual_formulas) for item in formulas) / max(len(formulas), 1)
    tables = [norm(b.get("html", "")) for b in truth.get("blocks", []) if b.get("type") == "html_table"]
    actual_tables = [norm(b.get("html", "")) for b in pred.get("blocks", []) if b.get("type") == "html_table"]
    table_rate = sum(any(fuzzy_contains(item, actual) for actual in actual_tables) for item in tables) / max(len(tables), 1)
    expected_regions = regions(truth)
    actual_regions = regions(pred, scale_coordinates=True)
    unmatched = list(expected_regions)
    true_positive = 0
    for actual in actual_regions:
        match = next((index for index, expected in enumerate(unmatched) if bbox_iou(actual, expected) >= 0.8), None)
        if match is not None:
            true_positive += 1
            unmatched.pop(match)
    precision = true_positive / max(len(actual_regions), 1)
    recall = true_positive / max(len(expected_regions), 1)
    duplicate_rate = 1 - len({tuple(round(value, 3) for value in region) for region in actual_regions}) / max(len(actual_regions), 1)
    truncated = pred.get("termination") == "max_tokens"
    blank = not actual
    return {"text_ned": ned, "formula_completeness": formula_rate, "table_teds": table_rate, "reading_order": 1.0 if actual == expected else max(0.0, 1.0 - ned), "bbox_precision": precision, "bbox_recall": recall, "duplicate_rate": duplicate_rate, "truncated": truncated, "blank_or_failed": blank or pred.get("termination") == "failed"}

def main():
    if len(sys.argv) != 4:
        print("usage: score-regression.py MANIFEST TRUTH_DIR PREDICTION_DIR", file=sys.stderr); return 2
    manifest, truth_dir, pred_dir = map(Path, sys.argv[1:])
    metadata = json.loads(manifest.read_text())
    if metadata.get("schema_version") != 1 or metadata.get("page_count", 0) < 30:
        print("invalid regression manifest", file=sys.stderr); return 3
    pages = []
    for index in range(1, metadata["page_count"] + 1):
        truth = truth_dir / f"page-{index:04}.json"; pred = pred_dir / f"page-{index:04}.json"
        if not truth.is_file() or not pred.is_file():
            print(f"missing page {index}", file=sys.stderr); return 3
        pages.append(score_page(json.loads(pred.read_text()), json.loads(truth.read_text())))
    keys = ["text_ned", "formula_completeness", "table_teds", "reading_order", "bbox_precision", "bbox_recall", "duplicate_rate"]
    averages = {key: sum(page[key] for page in pages) / len(pages) for key in keys}
    report = {"schema_version": 1, "set_id": metadata.get("set_id"), "page_count": len(pages), "metadata": {k: metadata.get(k) for k in ("model_revision", "runtime_revision", "adapter_revision", "prompt_revision")}, "metrics": averages, "truncated_pages": sum(page["truncated"] for page in pages), "blank_or_failed_pages": sum(page["blank_or_failed"] for page in pages), "pages": pages}
    print(json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
    return 0

if __name__ == "__main__": raise SystemExit(main())
