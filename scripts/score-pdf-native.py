#!/usr/bin/env python3
"""Score PDFium-native truth/prediction pages in the native facts schema."""
import json
import re
import sys
import unicodedata
from difflib import SequenceMatcher
from pathlib import Path

def norm(value):
    return re.sub(r"\n{3,}", "\n\n", unicodedata.normalize("NFC", value or "").replace("\r", "")).strip()

def distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        current = [i]
        for j, y in enumerate(b, 1):
            current.append(min(current[-1] + 1, row[j] + 1, row[j - 1] + (x != y)))
        row = current
    return row[-1]

def iou(a, b):
    left, top = max(a["left"], b["left"]), max(a["top"], b["top"])
    right, bottom = min(a["right"], b["right"]), min(a["bottom"], b["bottom"])
    intersection = max(0, right - left) * max(0, bottom - top)
    area_a = max(0, a["right"] - a["left"]) * max(0, a["bottom"] - a["top"])
    area_b = max(0, b["right"] - b["left"]) * max(0, b["bottom"] - b["top"])
    return intersection / max(area_a + area_b - intersection, 1)

def page_text(page):
    return norm(page.get("native_text", ""))

def normalize_prediction(page):
    """Accept native facts or the real CLI PageDocument artifact."""
    if "native_text" in page:
        return page
    if "blocks" in page and "page_number" in page:
        blocks = page.get("blocks", [])
        markdown = [block.get("content", "") for block in blocks if block.get("type") == "markdown"]
        return {"native_text": "\n".join(markdown), "blocks": [],
                "candidate_status": "complete" if markdown else "failed"}
    document = page.get("document", {})
    blocks = [block for block in document.get("blocks", []) if block.get("type") == "markdown"]
    provenance = page.get("provenance", [])
    text = "\n".join(block.get("content", "") for block in blocks)
    native_blocks = []
    for index, item in enumerate(provenance, 1):
        native_blocks.append({"text": "", "bbox": item.get("bbox", {}), "source_order": item.get("source_order", index), "source": item.get("source")})
    # The real artifact carries one markdown document block and per-line
    # provenance; score text independently and geometry against provenance.
    return {"native_text": text, "blocks": native_blocks, "candidate_status": page.get("processing", {}).get("actual_method") == "native_text" and "complete" or "failed"}

def score_page(truth, pred):
    expected, actual = page_text(truth), page_text(pred)
    ned = distance(actual, expected) / max(len(expected), len(actual), 1)
    truth_blocks = truth.get("blocks") or []
    pred_blocks = pred.get("blocks") or []
    unmatched = list(range(len(truth_blocks)))
    matched = 0
    bbox_matches = 0
    for candidate in pred_blocks:
        match = next((i for i in unmatched if norm(candidate.get("text")) == norm(truth_blocks[i].get("text"))), None)
        if match is None and not candidate.get("text") and candidate.get("source_order") in orders_for_truth(truth_blocks):
            match = next((i for i in unmatched if truth_blocks[i].get("source_order") == candidate.get("source_order")), None)
        if match is not None:
            matched += 1
            bbox_matches += iou(candidate["bbox"], truth_blocks[match]["bbox"]) >= 0.8
            unmatched.remove(match)
    precision = matched / max(len(pred_blocks), 1)
    recall = matched / max(len(truth_blocks), 1)
    f1 = 2 * precision * recall / max(precision + recall, 1e-12)
    bbox_recall = bbox_matches / max(len(truth_blocks), 1)
    bbox_valid = sum(1 for block in pred_blocks if valid_bbox(block.get("bbox"))) / max(len(pred_blocks), 1)
    orders = [block.get("source_order") for block in pred_blocks]
    order_score = 1.0 if orders == sorted(orders) and len(orders) == len(set(orders)) else 0.0
    return {"text_ned": ned, "reading_order": order_score, "block_precision": precision,
            "block_recall": recall, "block_f1": f1, "bbox_validity_rate": bbox_valid,
            "bbox_recall": bbox_recall, "duplicate": len(orders) != len(set(orders)),
            "blank_or_failed": not actual or pred.get("candidate_status") == "failed"}

def orders_for_truth(blocks):
    return {block.get("source_order") for block in blocks}

def valid_bbox(bbox):
    if not isinstance(bbox, dict):
        return False
    keys = ("left", "top", "right", "bottom")
    return all(isinstance(bbox.get(key), int) and 0 <= bbox[key] <= 1000 for key in keys) and bbox["left"] < bbox["right"] and bbox["top"] < bbox["bottom"]

def score(truth_dir, pred_dir):
    truth_files = sorted(truth_dir.glob("page-*.json"))
    if len(truth_files) != 30:
        raise ValueError("native quality scoring requires exactly 30 truth pages")
    pages = []
    for truth_path in truth_files:
        truth_page = json.loads(truth_path.read_text())
        source_name = pred_dir / f"page-{truth_page.get('source_page_number', truth_page.get('page_number')):04}.json"
        pred_path = truth_path if pred_dir.resolve() == truth_dir.resolve() else (source_name if source_name.is_file() else pred_dir / truth_path.name)
        if not pred_path.is_file():
            raise ValueError(f"missing prediction page: {pred_path}")
        pages.append(score_page(truth_page, normalize_prediction(json.loads(pred_path.read_text()))))
    keys = ("text_ned", "reading_order", "block_precision", "block_recall", "block_f1", "bbox_validity_rate", "bbox_recall")
    metrics = {key: sum(page[key] for page in pages) / len(pages) for key in keys}
    return {"schema_version": 1, "page_count": 30, "metrics": metrics,
            "duplicate_pages": sum(page["duplicate"] for page in pages),
            "blank_or_failed_pages": sum(page["blank_or_failed"] for page in pages), "pages": pages}

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("usage: score-pdf-native.py TRUTH_DIR PREDICTION_DIR", file=sys.stderr)
        raise SystemExit(2)
    try:
        print(json.dumps(score(Path(sys.argv[1]), Path(sys.argv[2])), ensure_ascii=False, sort_keys=True, separators=(",", ":")))
    except ValueError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(3)
