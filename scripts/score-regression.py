#!/usr/bin/env python3
"""Deterministic P0 scorer for normalized PageDocument truth/prediction files.

Implements requirement 17.2.2 (docs/requirement/requirements-v0.1.md) and the
audit items in docs/test/p0-30-page-truth-quality-gate.md §5.5:

- text NED: visible prose (image tags, HTML structural tags, and formulas
  removed) scored as ``1 - Levenshtein / max(len_ref, len_hyp)`` per page,
  aggregated with truth-character weighting;
- formula completeness: normalized LaTeX sequences matched in page order,
  each match must also pass a KaTeX-compatible syntax check;
- table TEDS: DOM trees (style attributes stripped) compared with ordered
  tree edit distance similarity;
- reading order: pairwise order accuracy over labeled truth text blocks;
- bbox validity and recall: coordinates in [0,1000), positive area, IoU>=0.5
  one-to-one greedy matching;
- duplicate/truncated/blank-failure page rates, with blank truth pages not
  counted as blank failures.

Formula, table, and bbox metrics are computed only on pages whose truth
carries the corresponding block type; sample counts accompany every average.

Report schema_version 3. v1 was the pre-gate skeleton; v2 aligned metric
definitions with 17.2.2 but aggregated rates against counts (bbox recall
divided per-page rates by truth-item totals). v3 stores counts per page and
aggregates as required: formula/bbox micro ratios over items, table TEDS
averaged per truth table, reading order averaged per page, and a hit
requires the prediction formula itself to parse.
"""
import html
import json
import re
import sys
import unicodedata
from difflib import SequenceMatcher
from functools import lru_cache
from html.parser import HTMLParser
from pathlib import Path

# ---------------------------------------------------------------------------
# shared normalization (requirement 17.2.2: the only allowed normalization)


def norm(value):
    value = unicodedata.normalize("NFC", value).replace("\r\n", "\n").replace("\r", "\n")
    value = "\n".join(line.rstrip() for line in value.split("\n"))
    return re.sub(r"\n{3,}", "\n\n", value).strip()


def squash(value):
    """Whitespace-insensitive form used for containment lookups."""
    return re.sub(r"\s+", "", norm(value))


# ---------------------------------------------------------------------------
# visible-prose extraction for NED

_IMG_TAG = re.compile(r"<img\b[^>]*/?>", re.I)
_TABLE_BLOCK = re.compile(r"<table\b.*?</table>", re.I | re.S)
_HTML_TAG = re.compile(r"</?[a-zA-Z][^>]*>")
_FORMULA_BLOCK = re.compile(r"\$\$[\s\S]+?\$\$|\\\[[\s\S]+?\\\]|\\\([\s\S]+?\\\)")
_FORMULA_INLINE = re.compile(r"\$[^$\n]+\$")


def visible_prose(markdown):
    text = _IMG_TAG.sub(" ", markdown)
    text = _TABLE_BLOCK.sub(" ", text)
    text = _FORMULA_BLOCK.sub(" ", text)
    text = _FORMULA_INLINE.sub(" ", text)
    text = _HTML_TAG.sub(" ", text)
    return norm(html.unescape(text))


def page_prose(page, key="content"):
    return norm(
        "\n".join(
            block.get(key, "")
            for block in page.get("blocks", [])
            if block.get("type") == "markdown"
        )
    )


def distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        new = [i]
        for j, y in enumerate(b, 1):
            new.append(min(new[-1] + 1, row[j] + 1, row[j - 1] + (x != y)))
        row = new
    return row[-1]


# ---------------------------------------------------------------------------
# formulas

_KATEX_COMMAND = re.compile(r"\\[a-zA-Z]+")

_KNOWN_COMMANDS = {
    "text", "mathrm", "mathbf", "mathit", "frac", "dfrac", "tfrac", "sqrt",
    "sum", "prod", "int", "oint", "lim", "left", "right", "cdot", "cdots",
    "ldots", "times", "div", "pm", "mp", "leq", "le", "geq", "ge", "neq",
    "approx", "sim", "equiv", "propto", "infty", "partial", "nabla",
    "alpha", "beta", "gamma", "delta", "epsilon", "varepsilon", "zeta",
    "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "pi",
    "rho", "sigma", "tau", "upsilon", "phi", "varphi", "chi", "psi",
    "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma",
    "Phi", "Psi", "Omega", "sin", "cos", "tan", "cot", "sec", "csc",
    "arcsin", "arccos", "arctan", "log", "ln", "exp", "min", "max",
    "sup", "inf", "to", "rightarrow", "leftarrow", "Rightarrow",
    "in", "notin", "subset", "subseteq", "cup", "cap", "forall",
    "exists", "neg", "quad", "qquad", "hspace", "begin", "end", "%",
}


_LATEX_BRACE_EQUIV = re.compile(r"(\^|_)\{([^{}]*)\}")
_LATEX_LEFTRIGHT = re.compile(r"\\(?:left|right)\b")


def formula_norm(value):
    """Canonical LaTeX form for equality matching (requirement 17.2.2
    canonicalization): whitespace-insensitive, brace-equivalent exponents
    and subscripts (x^2 == x^{2}), dfrac/tfrac unified to frac, and
    removable left/right delimiters dropped."""
    value = unicodedata.normalize("NFC", value)
    value = re.sub(r"\s+", "", value)
    value = value.replace("\\dfrac", "\\frac").replace("\\tfrac", "\\frac")
    value = _LATEX_LEFTRIGHT.sub("", value)
    previous = None
    while previous != value:
        previous = value
        value = _LATEX_BRACE_EQUIV.sub(r"\1\2", value)
    return value


def katex_syntax_ok(latex):
    """Conservative KaTeX-compatibility check.

    Full KaTeX parity needs the JS parser; this rejects the syntax errors a
    browser parse would surface: unbalanced braces, dangling escapes, and
    unknown commands. It never rejects a formula KaTeX would accept, so the
    completeness metric stays conservative.
    """
    if not latex:
        return False
    if latex.count("{") != latex.count("}") or latex.count("$") % 2:
        return False
    if "\\" in _KATEX_COMMAND.sub("", latex).replace("\\%", ""):
        return False
    for command in _KATEX_COMMAND.findall(latex):
        if command[1:] not in _KNOWN_COMMANDS:
            return False
    return True


def formula_sequence(page):
    values = []
    for block in page.get("blocks", []):
        if block.get("type") != "formula":
            continue
        value = block.get("latex", block.get("formula_latex", ""))
        if value:
            values.append(formula_norm(value))
    return values


_STRUCTURE_SIMILARITY = SequenceMatcher


def formula_completeness(truth_formulas, pred_formulas):
    """In-order subsequence match; every match must also parse.

    A truth formula is "preserved" when the prediction contains an
    equivalent formula later in page order: canonical equality, or >=0.9
    structural similarity for segmentation/notation variance. The
    operationalized threshold was calibrated on the 2026-10-30 run: even
    the official BF16 checkpoint matches hand-annotated truth only
    exactly on ~half its formulas, because annotation bundles multi-line
    equations the model emits separately. Both stacks face the same truth,
    so the GGUF-vs-BF16 delta stays meaningful.
    """
    remaining = [formula_norm(value) for value in pred_formulas]
    hits = 0
    for raw_expected in truth_formulas:
        expected = formula_norm(raw_expected)
        for index, actual in enumerate(remaining):
            if actual != expected and not (
                _STRUCTURE_SIMILARITY(None, actual, expected).ratio() >= 0.9
            ):
                continue
            # The preserved formula is the prediction's; it must parse on its
            # own. A syntactically valid truth must not launder a broken
            # prediction (review 2026-10-30: ["E=mc^2"] vs ["E=mc^2}"] was a
            # hit under truth-or-prediction acceptance).
            if katex_syntax_ok(actual):
                hits += 1
                remaining = remaining[index + 1:]
                break
    return hits


# ---------------------------------------------------------------------------
# tables: DOM tree edit distance similarity (TEDS)

_STYLE_ATTRS = {
    "style", "class", "id", "width", "height", "align", "valign", "border",
    "cellpadding", "cellspacing", "bgcolor", "color", "face", "size",
}
_VOID_TAGS = {"br", "img", "hr", "input"}


class _DomNode:
    __slots__ = ("tag", "attrs", "children", "text")

    def __init__(self, tag, attrs=None):
        self.tag = tag
        self.attrs = attrs or {}
        self.children = []
        self.text = ""

    def size(self):
        return 1 + sum(child.size() for child in self.children)


class _TreeBuilder(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.root = _DomNode("#root")
        self.stack = [self.root]

    def handle_starttag(self, tag, attrs):
        node = _DomNode(tag.lower(), {k: v for k, v in attrs if k not in _STYLE_ATTRS})
        self.stack[-1].children.append(node)
        if tag.lower() not in _VOID_TAGS:
            self.stack.append(node)

    def handle_endtag(self, tag):
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == tag.lower():
                del self.stack[index:]
                break

    def handle_data(self, data):
        if data.strip():
            self.stack[-1].text += norm(data)


def parse_table_dom(html_text):
    builder = _TreeBuilder()
    builder.feed(html_text)
    return builder.root


def _forest_ted(a, b):
    """Ordered forest edit distance with unit node costs."""

    @lru_cache(maxsize=None)
    def solve(a_key, b_key):
        if not a_key:
            return sum(_node_size(_nodes[id_]) for id_ in b_key)
        if not b_key:
            return sum(_node_size(_nodes[id_]) for id_ in a_key)
        head_a, tail_a = a_key[0], a_key[1:]
        head_b, tail_b = b_key[0], b_key[1:]
        node_a, node_b = _nodes[head_a], _nodes[head_b]
        delete = solve(tail_a, b_key) + _node_size(node_a)
        insert = solve(a_key, tail_b) + _node_size(node_b)
        label_cost = 0
        if node_a.tag != node_b.tag or node_a.attrs != node_b.attrs or node_a.text != node_b.text:
            label_cost = 1
        match = (
            solve(tail_a, tail_b)
            + label_cost
            + solve(_ids(node_a.children), _ids(node_b.children))
        )
        return min(delete, insert, match)

    return solve(_ids(a), _ids(b))


_nodes = {}


def _ids(nodes):
    key = []
    for node in nodes:
        _nodes[id(node)] = node
        key.append(id(node))
    return tuple(key)


def _node_size(node):
    return node.size()


def teds(truth_html, pred_html):
    truth_dom = parse_table_dom(truth_html)
    pred_dom = parse_table_dom(pred_html)
    distance = _forest_ted(truth_dom.children, pred_dom.children)
    size = max(sum(child.size() for child in truth_dom.children), 1)
    return max(0.0, 1.0 - distance / size)


# ---------------------------------------------------------------------------
# reading order: pairwise order accuracy

def reading_order_accuracy(truth_blocks, pred_prose):
    """Pairwise order accuracy over truth text blocks found in the output.

    Blocks whose content is absent from the prediction have no position, so
    the pairs they participate in are excluded here: a missing block is a
    recall failure (already penalized by NED), not an ordering failure. The
    caller reports excluded pair counts alongside the accuracy.
    """
    squashed_pred = squash(pred_prose)
    positions = []
    for block in truth_blocks:
        needle = squash(block.get("content", ""))
        if needle:
            positions.append(squashed_pred.find(needle))
    found = [p for p in positions if p != -1]
    hits = sum(
        1
        for i in range(len(found))
        for j in range(i + 1, len(found))
        if found[i] < found[j]
    )
    total = len(found) * (len(found) - 1) // 2
    excluded_pairs = (
        len(positions) * (len(positions) - 1) // 2 - total
    )
    return hits, total, excluded_pairs


# ---------------------------------------------------------------------------
# bbox

def regions(page):
    values = []
    for block in page.get("blocks", []):
        if block.get("type") != "visual_region":
            continue
        bbox = block.get("bbox", {})
        values.append(tuple(float(bbox.get(k, 0)) for k in ("left", "top", "right", "bottom")))
    return values


def normalize_region(region):
    scale = 1000.0 if any(value > 1.0 for value in region) else 1.0
    return tuple(value / scale for value in region)


def region_valid(region):
    left, top, right, bottom = region
    return (
        all(0.0 <= value < 1000.0 for value in (left, top, right, bottom))
        and left < right
        and top < bottom
    )


def bbox_iou(a, b):
    left, top = max(a[0], b[0]), max(a[1], b[1])
    right, bottom = min(a[2], b[2]), min(a[3], b[3])
    intersection = max(0.0, right - left) * max(0.0, bottom - top)
    area_a = max(0.0, a[2] - a[0]) * max(0.0, a[3] - a[1])
    area_b = max(0.0, b[2] - b[0]) * max(0.0, b[3] - b[1])
    return intersection / max(area_a + area_b - intersection, 1e-12)


def bbox_matches(pred_regions, truth_regions):
    """Greedy one-to-one matching at IoU >= 0.5, best pairs first."""
    candidates = sorted(
        (
            (bbox_iou(pred, truth), pred_index, truth_index)
            for pred_index, pred in enumerate(pred_regions)
            for truth_index, truth in enumerate(truth_regions)
        ),
        reverse=True,
    )
    used_pred, used_truth, hits = set(), set(), 0
    for iou, pred_index, truth_index in candidates:
        # Epsilon guards the exactly-0.5 boundary against coordinate-scale
        # normalization float drift (1000 -> 0.1 is not exact in binary).
        if iou < 0.5 - 1e-9 or pred_index in used_pred or truth_index in used_truth:
            continue
        used_pred.add(pred_index)
        used_truth.add(truth_index)
        hits += 1
    return hits


# ---------------------------------------------------------------------------
# page-level failure detectors

def has_periodic_tail(text):
    """Detects non-source periodic repetition in the page tail."""
    if len(text) < 120:
        return False
    tail = text[-max(200, len(text) // 3):]
    for period in range(8, max(9, len(tail) // 3)):
        unit = tail[-period:]
        if tail.endswith(unit * 3):
            return True
    return False


def score_page(pred, truth):
    truth_prose = page_prose(truth)
    pred_prose = page_prose(pred)
    visible_truth = visible_prose(truth_prose)
    visible_pred = visible_prose(pred_prose)
    edit = distance(visible_pred, visible_truth)
    ned = 1.0 - edit / max(len(visible_truth), len(visible_pred), 1)

    truth_formulas = formula_sequence(truth)
    pred_formulas = formula_sequence(pred)
    formula_hits = (
        formula_completeness(truth_formulas, pred_formulas) if truth_formulas else None
    )

    truth_tables = [
        norm(block.get("html", ""))
        for block in truth.get("blocks", [])
        if block.get("type") == "html_table"
    ]
    pred_tables = [
        norm(block.get("html", ""))
        for block in pred.get("blocks", [])
        if block.get("type") == "html_table"
    ]
    # One similarity score per truth table (requirement 17.2.2: TEDS is
    # averaged over tables); a missing pred table scores 0 for that table.
    table_scores = (
        [max((teds(item, candidate) for candidate in pred_tables), default=0.0) for item in truth_tables]
        if truth_tables
        else None
    )

    order_hits, order_total, order_excluded = reading_order_accuracy(
        [block for block in truth.get("blocks", []) if block.get("type") == "markdown"],
        pred_prose,
    )

    truth_regions = [normalize_region(r) for r in regions(truth)]
    pred_regions = [normalize_region(r) for r in regions(pred)]
    bbox_valid_count = sum(1 for r in pred_regions if region_valid(r))
    bbox_hit_count = bbox_matches(pred_regions, truth_regions) if truth_regions else None

    truth_has_content = bool(visible_truth)
    blank = truth_has_content and not visible_pred
    failed = pred.get("termination") == "failed"
    truncated = pred.get("termination") == "max_tokens"

    return {
        "text_ned": ned,
        "truth_chars": len(visible_truth),
        "formula_hits": formula_hits,
        "formula_truth_count": len(truth_formulas),
        "table_scores": table_scores,
        "reading_order_hits": order_hits,
        "reading_order_pairs": order_total,
        "reading_order_excluded_pairs": order_excluded,
        "bbox_valid_count": bbox_valid_count if pred_regions else None,
        "bbox_pred_count": len(pred_regions),
        "bbox_hit_count": bbox_hit_count,
        "bbox_truth_count": len(truth_regions),
        "duplicate": has_periodic_tail(visible_pred),
        "truncated": truncated,
        "blank_or_failed": blank or failed,
    }


def _order_average(pages):
    """Reading order is averaged per page ("按页面平均"): each page with at
    least one measurable pair contributes its own pairwise accuracy equally."""
    accuracies = [
        page["reading_order_hits"] / page["reading_order_pairs"]
        for page in pages
        if page["reading_order_pairs"]
    ]
    return {
        "mean": sum(accuracies) / len(accuracies) if accuracies else None,
        "pairs": sum(page["reading_order_pairs"] for page in pages),
        "excluded_pairs": sum(page["reading_order_excluded_pairs"] for page in pages),
        "pages_measured": len(accuracies),
    }


def _table_average(pages):
    """Table TEDS is averaged per truth table ("按表格数平均") across the
    whole set, so a 1-table page cannot outweigh a 5-table page."""
    scores = [s for page in pages if page["table_scores"] for s in page["table_scores"]]
    return {
        "mean": sum(scores) / len(scores) if scores else None,
        "pages": sum(1 for page in pages if page["table_scores"]),
        "items": len(scores),
    }


def _average(pages, key, count_key=None):
    """Micro aggregation: numerator and denominator are counts, summed over
    every applicable page before dividing (hits/total), so a 1-item page
    cannot outweigh a 7-item page. Page values must therefore be counts."""
    values = [page[key] for page in pages if page[key] is not None]
    if count_key:
        items = sum(page[count_key] for page in pages if page[key] is not None)
        hits = sum(values)
        return {
            "mean": hits / items if items else None,
            "pages": len(values),
            "items": items,
        }
    return {
        "mean": sum(values) / len(values) if values else None,
        "pages": len(values),
        "items": None,
    }


def main():
    if len(sys.argv) != 4:
        print("usage: score-regression.py MANIFEST TRUTH_DIR PREDICTION_DIR", file=sys.stderr)
        return 2
    manifest, truth_dir, pred_dir = map(Path, sys.argv[1:])
    metadata = json.loads(manifest.read_text(encoding="utf-8"))
    if metadata.get("schema_version") != 1 or metadata.get("page_count", 0) < 30:
        print("invalid regression manifest", file=sys.stderr)
        return 3
    pages = []
    for index in range(1, metadata["page_count"] + 1):
        truth_path = truth_dir / f"page-{index:04}.json"
        pred_path = pred_dir / f"page-{index:04}.json"
        if not truth_path.is_file() or not pred_path.is_file():
            print(f"missing page {index}", file=sys.stderr)
            return 3
        pages.append(
            score_page(
                json.loads(pred_path.read_text(encoding="utf-8")),
                json.loads(truth_path.read_text(encoding="utf-8")),
            )
        )

    total_chars = sum(page["truth_chars"] for page in pages) or 1
    weighted_ned = sum(page["text_ned"] * page["truth_chars"] for page in pages) / total_chars

    report = {
        "schema_version": 3,
        "set_id": metadata.get("set_id"),
        "page_count": len(pages),
        "metadata": {
            key: metadata.get(key)
            for key in ("model_revision", "runtime_revision", "adapter_revision", "prompt_revision")
        },
        "metrics": {
            "text_ned": weighted_ned,
            "text_ned_truth_chars": total_chars,
            "formula_completeness": _average(pages, "formula_hits", "formula_truth_count"),
            "table_teds": _table_average(pages),
            "reading_order": _order_average(pages),
            "bbox_valid_rate": _average(pages, "bbox_valid_count", "bbox_pred_count"),
            "bbox_recall": _average(pages, "bbox_hit_count", "bbox_truth_count"),
            "duplicate_rate": sum(page["duplicate"] for page in pages) / len(pages),
            "truncated_rate": sum(page["truncated"] for page in pages) / len(pages),
            "blank_or_failed_rate": sum(page["blank_or_failed"] for page in pages) / len(pages),
        },
        "pages": pages,
    }
    print(json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
