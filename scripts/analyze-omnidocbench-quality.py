#!/usr/bin/env python3
"""Stage-0 characterization analyses for the quality plan (Q2.1/Q5.1/Q5.2/Q6.1).

Subcommands (each writes one JSON report under
``benchmarks/omnidocbench/reports/analysis/`` and prints a summary):

  q51  28 pure-truncated pages: tail periodicity three-way split
       (long-period loop evading the p<=64 detector / true long content /
       budget edge).
  q52  near-empty pages (tiny or think-only markdown): true empty vs
       think-only vs adapter over-strip.
  q21  92 newspaper degenerated corpus pages: loop-content taxonomy from the
       raw sidecar (table HTML / formula / bbox tag sequence / line
       fragments / no strong loop) joined with ground-truth layout signals
       (column estimate, cross-column images) and the render-audit
       downsample join.
  q61  en table TEDS gap: per-page aggregation of the official per-table
       detail, worst-50 pages, three-way error split (missing table /
       representation suspicion / transcription suspicion). The formula half
       of Q6.1 is closed by E2 (token-2048 eliminated the en formula gap).

Usage:
  python scripts/analyze-omnidocbench-quality.py q51 --benchmark-root benchmarks/omnidocbench
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path


def decode(value: str) -> str:
    if value and len(value) % 2 == 0:
        try:
            return bytes.fromhex(value).decode("utf-8", "strict")
        except ValueError:
            return value
    return value


def read_checkpoint(path: Path) -> dict:
    fields: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, _, value = line.partition("=")
        fields[key] = decode(value)
    return fields


def load_ledger(root: Path) -> list[dict]:
    """All balanced checkpoint pages with stem/flags (corpus build light)."""
    pages = []
    for scan_dir in sorted((root / "scans" / "balanced").iterdir()):
        checkpoint = scan_dir / "page-0001.checkpoint"
        if not scan_dir.is_dir() or not checkpoint.is_file():
            continue
        fields = read_checkpoint(checkpoint)
        pages.append({
            "stem": scan_dir.name.rsplit("-", 1)[0],
            "scan_dir": scan_dir.name,
            "degenerated": fields.get("degenerated") == "true",
            "truncated": fields.get("truncated") == "true",
            "attempts": int(fields.get("attempts", "1")),
        })
    return pages


def find_period(text: str, max_period: int = 600, min_repeats: int = 3) -> int | None:
    """Smallest exact period p of the text tail repeated min_repeats times."""
    n = len(text)
    for period in range(2, max_period + 1):
        need = period * min_repeats
        if need > n:
            return None
        unit = text[-period:]
        if unit * min_repeats == text[-need:]:
            return period
    return None


def out_dir(root: Path) -> Path:
    path = root / "reports" / "analysis"
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_report(root: Path, name: str, doc: dict) -> None:
    path = out_dir(root) / f"{name}.json"
    path.write_text(json.dumps(doc, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"written: {path}")


# ---------------------------------------------------------------- q51
def cmd_q51(root: Path) -> int:
    pages = load_ledger(root)
    pure = [p for p in pages if p["truncated"] and not p["degenerated"]]
    rows = []
    for page in pure:
        md_path = root / "scans" / "balanced" / page["scan_dir"] / "page-0001.md"
        md = md_path.read_text(encoding="utf-8", errors="replace")
        truncated_marker = "（输出被截断）" in md
        body = md.replace("（输出被截断）", "").rstrip()
        tail_window = body[-6000:]
        # Loops that increment numbers ("2857.\n\n2858.\n\n...") are not
        # byte-exact periodic; mask digits before the exact-period check.
        masked = re.sub(r"[0-9０-９]", "#", tail_window)
        exact_period = find_period(tail_window, max_period=600, min_repeats=3)
        strong_period = find_period(masked, max_period=300, min_repeats=20)
        weak_period = find_period(masked, max_period=600, min_repeats=3)
        last_line = body.splitlines()[-1] if body.splitlines() else ""
        clean_end = bool(re.search(r"[。.!?:；;）)\]]$", last_line.strip()))
        if exact_period is not None:
            cls = "loop_exact_period"
        elif strong_period is not None:
            cls = "loop_numeric_increment"
        elif weak_period is not None:
            cls = "loop_suspect_weak"
        elif clean_end:
            cls = "budget_edge"
        else:
            cls = "true_long_content"
        rows.append({
            "stem": page["stem"],
            "md_chars": len(body),
            "exact_period_chars": exact_period,
            "masked_strong_period_chars": strong_period,
            "masked_weak_period_chars": weak_period,
            "truncated_marker": truncated_marker,
            "class": cls,
        })
    summary = Counter(r["class"] for r in rows)
    loop_total = sum(1 for r in rows if r["class"].startswith("loop"))
    doc = {
        "schema_version": 1,
        "task": "Q5.1 pure-truncated three-way characterization",
        "n_pages": len(rows),
        "summary": dict(summary),
        "loop_share": f"{loop_total}/{len(rows)}",
        "verdict_input": {
            "q1_5_condition": "loop_* share is the Q1.5 trigger (truncated pages "
                              "entering the escape-ladder tail tier); observed share "
                              "decides whether Q1.5 is conditionally initiated",
            "d6_residual": "true_long_content + budget_edge stay accepted residual",
        },
        "pages": sorted(rows, key=lambda r: r["class"]),
    }
    write_report(root, "analysis-q51-pure-truncation", doc)
    print("Q5.1 summary:", dict(summary))
    return 0


# ---------------------------------------------------------------- q52
def cmd_q52(root: Path) -> int:
    pages = load_ledger(root)
    rows = []
    for page in pages:
        scan_dir = root / "scans" / "balanced" / page["scan_dir"]
        md_path = scan_dir / "page-0001.md"
        if not md_path.is_file():
            continue
        md = md_path.read_text(encoding="utf-8", errors="replace")
        if len(md.strip()) >= 64:
            continue
        raw_path = scan_dir / "page-0001.raw"
        raw = raw_path.read_text(encoding="utf-8", errors="replace") if raw_path.is_file() else ""
        page_json_path = scan_dir / "page-0001.json"
        blocks = 0
        if page_json_path.is_file():
            blocks = len(json.loads(page_json_path.read_text(encoding="utf-8")).get("blocks", []))
        stripped = len(re.findall(r"<think\b[^>]*>.*?</think>", raw, re.DOTALL))
        if not raw.strip():
            cls = "true_empty_model_output"
        elif stripped > 0 and not md.strip():
            cls = "think_only_output"
        elif not md.strip():
            cls = "adapter_drop_suspect"
        else:
            cls = "tiny_output"
        rows.append({
            "stem": page["stem"],
            "md_chars": len(md.strip()),
            "raw_chars": len(raw.strip()),
            "page_json_blocks": blocks,
            "think_blocks_in_raw": stripped,
            "class": cls,
        })
    summary = Counter(r["class"] for r in rows)
    doc = {
        "schema_version": 1,
        "task": "Q5.2 near-pure-image empty-output characterization (balanced)",
        "n_pages": len(rows),
        "summary": dict(summary),
        "pages": rows,
    }
    write_report(root, "analysis-q52-empty-output", doc)
    print("Q5.2 summary:", dict(summary))
    return 0


# ---------------------------------------------------------------- q21
def cmd_q21(root: Path) -> int:
    asset = (
        Path(__file__).resolve().parents[1]
        / "assets/regression/omnidocbench-degeneration-v0.1/pages.json"
    )
    corpus = json.loads(asset.read_text(encoding="utf-8"))["pages"]
    newspaper = [p for p in corpus if p.get("source_category") == "newspaper"]
    dataset = json.loads((root / "dataset" / "OmniDocBench.json").read_text(encoding="utf-8"))
    img2dets = {
        e["page_info"]["image_path"]: e["layout_dets"]
        for e in dataset
    }
    rows = []
    for page in newspaper:
        scan_dir = root / "scans" / "balanced" / page["scan_dir"]
        raw = (scan_dir / "page-0001.raw").read_text(encoding="utf-8", errors="replace")
        period = find_period(raw[-6000:], max_period=600, min_repeats=2)
        unit = raw[-period:] if period else raw[-400:]
        markers = {
            "table_html": ("<td" in unit or "<tr" in unit or "<table" in unit),
            "formula": ("$$" in unit or (r"\begin{" in unit and r"\end{" in unit)),
            "bbox_tag_seq": ("<img" in unit and "bbox_" in unit),
        }
        if not any(markers.values()):
            loop_type = "line_fragments"
        elif markers["table_html"]:
            loop_type = "table_html"
        elif markers["formula"]:
            loop_type = "formula"
        else:
            loop_type = "bbox_tag_seq"
        if period is None:
            loop_type = "no_strong_loop"

        dets = img2dets.get(page.get("image_path"), [])
        original = page["render"].get("original") or {"width": 1, "height": 1}
        width = original["width"]
        height = original["height"]
        scale = 300 / 72.0
        render_over_budget = (
            int(width * scale) * int(height * scale) > 2880 * 2880
        )
        text_xs = sorted(
            det["poly"][0] / width
            for det in dets
            if det["category_type"] == "text_block" and not det.get("ignore")
        )
        columns = 1
        for prev, cur in zip(text_xs, text_xs[1:]):
            if cur - prev > 0.12:
                columns += 1
        images = [
            det for det in dets
            if det["category_type"] in ("figure", "image")
        ]
        cross_column_images = sum(
            1 for det in images
            if (det["poly"][2] - det["poly"][0]) / width > 0.6
        )
        rows.append({
            "stem": page["stem"],
            "language": page.get("language"),
            "loop_type": loop_type,
            "tail_period_chars": period,
            "gt_columns_estimate": columns,
            "gt_text_blocks": sum(1 for d in dets if d["category_type"] == "text_block"),
            "gt_images": len(images),
            "gt_cross_column_images": cross_column_images,
            "render_over_budget": render_over_budget,
        })
    loop_summary = Counter(r["loop_type"] for r in rows)
    over_budget = sum(1 for r in rows if r["render_over_budget"])
    multi_column = sum(1 for r in rows if r["gt_columns_estimate"] >= 2)
    cross_img = sum(1 for r in rows if r["gt_cross_column_images"] > 0)
    doc = {
        "schema_version": 1,
        "task": "Q2.1 newspaper degeneration taxonomy",
        "n_pages": len(rows),
        "loop_type_summary": dict(loop_summary),
        "layout_signals": {
            "multi_column_pages": multi_column,
            "pages_with_cross_column_images": cross_img,
            "render_over_budget_pages": over_budget,
        },
        "strategy_response": {
            "token_2048": "expected strong on table_html/formula loops (E1/E2)",
            "column_projection": "expected on multi-column line_fragments pages; "
                                 "cross-column image pages are known failures",
            "sampling_penalty": "table_html loops need the low-penalty tier",
        },
        "pages": rows,
    }
    write_report(root, "analysis-q21-newspaper-taxonomy", doc)
    print("Q2.1 loop types:", dict(loop_summary))
    print(f"Q2.1 multi-column {multi_column}/{len(rows)}, "
          f"cross-column images {cross_img}, render over-budget {over_budget}")
    return 0


# ---------------------------------------------------------------- q61
def cmd_q61(root: Path) -> int:
    teds_path = (
        root / "reports"
        / "omnidocbench-balanced-en_quick_match_table_per_table_TEDS.json"
    )
    teds = json.loads(teds_path.read_text(encoding="utf-8"))
    per_page: dict[str, list[float]] = {}
    for key, value in teds.items():
        if isinstance(value, dict):
            value = value["TEDS"]
        # Table-detail keys are prefixed with the GT image file name.
        per_page.setdefault(key.split("_[")[0], []).append(value)
    page_scores = {
        stem: sum(values) / len(values) for stem, values in per_page.items()
    }
    worst = sorted(page_scores.items(), key=lambda kv: kv[1])[:50]
    dataset = json.loads((root / "dataset" / "OmniDocBench.json").read_text(encoding="utf-8"))
    img2dets = {e["page_info"]["image_path"]: e["layout_dets"] for e in dataset}
    mapping = json.loads((root / "pdfs" / "mapping.json").read_text(encoding="utf-8"))
    img2stem = {p["image_path"]: p["stem"] for p in mapping["pages"]}

    rows = []
    for image_name, score in worst:
        stem = img2stem.get(image_name)
        if stem is None:
            continue
        pred_path = root / "predictions" / "balanced" / f"{stem}.md"
        pred = pred_path.read_text(encoding="utf-8", errors="replace") if pred_path.is_file() else ""
        pred_tables = len(re.findall(r"<table", pred))
        dets = img2dets.get(image_name, [])
        gt_tables = [det for det in dets if det["category_type"] == "table"]
        if not gt_tables:
            continue

        def tag_counts(html: str) -> Counter:
            return Counter(re.findall(r"</?(table|tr|td|th)\b", html))

        if pred_tables < len(gt_tables):
            cls = "missing_or_dropped_table"
        else:
            # Same table count: compare structure tags against GT. Close tag
            # multisets with a low TEDS point at a representation mismatch
            # (legal HTML, large tree distance); wild divergence points at
            # transcription errors.
            representation = 0
            transcription = 0
            pred_htmls = re.findall(r"<table.*?</table>", pred, re.DOTALL)
            for det in gt_tables:
                gt_tags = tag_counts(det.get("html", ""))
                best = None
                for candidate in pred_htmls:
                    candidate_tags = tag_counts(candidate)
                    delta = sum(
                        abs(gt_tags[tag] - candidate_tags[tag])
                        for tag in set(gt_tags) | set(candidate_tags)
                    )
                    if best is None or delta < best:
                        best = delta
                if best is not None and best <= max(6, 0.3 * sum(gt_tags.values())):
                    representation += 1
                else:
                    transcription += 1
            cls = "representation_suspicion" if representation >= transcription else "transcription_suspicion"
        rows.append({
            "stem": stem,
            "page_teds_mean": round(score, 4),
            "gt_tables": len(gt_tables),
            "pred_tables": pred_tables,
            "class": cls,
        })
    summary = Counter(r["class"] for r in rows)
    doc = {
        "schema_version": 1,
        "task": "Q6.1 en table TEDS gap taxonomy (worst 50 pages)",
        "note": "formula half closed by E2: token-2048 removed the en formula gap",
        "n_worst_pages": len(rows),
        "summary": dict(summary),
        "pages": rows,
    }
    write_report(root, "analysis-q61-en-table-gap", doc)
    print("Q6.1 summary:", dict(summary))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("task", choices=["q51", "q52", "q21", "q61"])
    parser.add_argument("--benchmark-root", type=Path, default=Path("benchmarks/omnidocbench"))
    args = parser.parse_args()
    handlers = {"q51": cmd_q51, "q52": cmd_q52, "q21": cmd_q21, "q61": cmd_q61}
    return handlers[args.task](args.benchmark_root)


if __name__ == "__main__":
    sys.exit(main())
