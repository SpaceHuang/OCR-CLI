#!/usr/bin/env python3
"""Unit checks for the v2 score-regression metric definitions (§17.2.2)."""
import importlib.util
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "score_regression", Path(__file__).with_name("score-regression.py")
)
sr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sr)


def check(name, condition):
    if not condition:
        print(f"FAIL {name}")
        sys.exit(1)
    print(f"ok   {name}")


# NED: identical prose = 1.0; garbage vs truth = low; weighted toward truth.
check("ned identical", sr.score_page(
    {"blocks": [{"type": "markdown", "content": "义务教育教科书"}], "termination": "eos"},
    {"blocks": [{"type": "markdown", "content": "义务教育教科书"}]},
)["text_ned"] == 1.0)
check("ned empty pred", sr.score_page(
    {"blocks": [{"type": "markdown", "content": ""}], "termination": "eos"},
    {"blocks": [{"type": "markdown", "content": "义务教育教科书"}]},
)["text_ned"] == 0.0)

# Formulas/tables/img tags are stripped from prose before NED.
page_with_formula = {"blocks": [
    {"type": "markdown", "content": "正文\n\n$$E=mc^2$$\n\n<img src=\"images/x.jpg\" />"},
    {"type": "formula", "latex": "E=mc^2"},
], "termination": "eos"}
check("ned strips formulas and img", sr.score_page(
    page_with_formula,
    {"blocks": [{"type": "markdown", "content": "正文"}]},
)["text_ned"] == 1.0)

# Formula completeness: truth uses `formula_latex`, pred uses `latex`;
# in-order matching; broken syntax does not count.
truth = {"blocks": [{"type": "formula", "formula_latex": "E=mc^2"}]}
pred_good = {"blocks": [{"type": "formula", "latex": "E = mc^2"}]}
pred_bad_syntax = {"blocks": [{"type": "formula", "latex": "E=mc^2}}{"}]}
page = sr.score_page(pred_good, truth)
check("formula match via dual keys", page["formula_hits"] == 1)
check("formula broken syntax rejected", sr.score_page(pred_bad_syntax, truth)["formula_hits"] == 0)

# Table TEDS: identical tables = 1.0; structural edit lowers it; style attrs ignored.
t_html = "<table><tr><td>A</td><td>B</td></tr></table>"
check("teds identical", abs(sr.teds(t_html, t_html) - 1.0) < 1e-9)
check("teds style-insensitive", abs(sr.teds(t_html, '<table border="1"><tr><td>A</td><td>B</td></tr></table>') - 1.0) < 1e-9)
assert sr.teds(t_html, "<table><tr><td>A</td><td>C</td></tr></table>") < 1.0
check("teds content edit < 1", sr.teds(t_html, "<table><tr><td>A</td><td>C</td></tr></table>") < 1.0)
check("teds missing table = 0", sr.teds(t_html, "") == 0.0)

# Reading order: correct sequence = all pairs correct; swapped = zero hits.
blocks3 = [{"type": "markdown", "content": f"第{i}段"} for i in (1, 2, 3)]
truth3 = {"blocks": blocks3}
pred_ordered = {"blocks": [{"type": "markdown", "content": "第1段\n\n第2段\n\n第3段"}]}
pred_swapped = {"blocks": [{"type": "markdown", "content": "第2段\n\n第1段\n\n第3段"}]}
p = sr.score_page(pred_ordered, truth3)
check("reading order correct", (p["reading_order_hits"], p["reading_order_pairs"]) == (3, 3))
p = sr.score_page(pred_swapped, truth3)
check("reading order swapped", p["reading_order_hits"] < 3)

# bbox: 0-1 and 0-1000 coordinate scales both accepted; IoU>=0.5 one-to-one.
truth_bbox = {"blocks": [{"type": "visual_region", "bbox": {"left": 100, "top": 100, "right": 500, "bottom": 500}}]}
pred_same_0_1 = {"blocks": [{"type": "visual_region", "bbox": {"left": 0.1, "top": 0.1, "right": 0.5, "bottom": 0.5}}]}
pred_half = {"blocks": [{"type": "visual_region", "bbox": {"left": 100, "top": 100, "right": 300, "bottom": 500}}]}
p = sr.score_page(pred_same_0_1, truth_bbox)
check("bbox scale-normalized recall", p["bbox_recall"] == 1.0)
check("bbox valid rate", p["bbox_valid_rate"] == 1.0)
p = sr.score_page(pred_half, truth_bbox)
check("bbox iou 0.5 boundary matches", p["bbox_recall"] == 1.0)

# Blank truth page with blank pred is NOT a blank failure; blank pred on
# content truth IS.
check("blank truth not failure", sr.score_page(
    {"blocks": [{"type": "markdown", "content": ""}], "termination": "eos"},
    {"blocks": [], "termination": "eos"},
)["blank_or_failed"] is False)
check("blank pred on content truth is failure", sr.score_page(
    {"blocks": [{"type": "markdown", "content": ""}], "termination": "eos"},
    {"blocks": [{"type": "markdown", "content": "内容"}]},
)["blank_or_failed"] is True)

# Duplicate tail detection.
check("periodic tail flagged", sr.has_periodic_tail("开头内容。" + "重复内容块内容块" * 40))
check("normal tail not flagged", not sr.has_periodic_tail(
    "第一章 总论。经济学研究资源配置。第二章 微观部分。供给与需求决定价格。"
    "第三章 生产者行为。成本曲线与边际分析。第四章 市场结构。完全竞争与垄断比较。"))

print("all score-regression checks passed")
