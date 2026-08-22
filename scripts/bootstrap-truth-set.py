#!/usr/bin/env python3
"""Bootstrap a draft OCR truth set with LiteParse (never ocrx).

Pipeline: probe → select → screenshot → draft → status
See docs/test/script/bootstrap-truth-set-requirements.md
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REQUIRED_CATEGORIES = [
    "text",
    "formula",
    "table",
    "visual",
    "mixed",
    "blank",
    "truncated",
    "failure",
]
DEFAULT_QUOTA = {c: 2 for c in REQUIRED_CATEGORIES}
# LaTeX / symbol formulas
FORMULA_SYMBOL_RE = re.compile(
    r"\$\$[\s\S]+?\$\$|\$[^$\n]+\$|[∑∫√∞≈≠≤≥±×÷]|[₀-₉⁰-⁹]|\\frac|\\sqrt|\\sum|\\int"
)
# Scanned Chinese math OCR rarely emits $$; catch textbook cues + simple equations.
FORMULA_ZH_RE = re.compile(
    r"方程|公式|平方|立方|根号|配方法|因式分解|一元二次|勾股|圆周角|圆心角|"
    r"三角函数|正弦|余弦|正切|函数关系式|不等式|"
    r"[A-Za-z]\s*=\s*[-−]?\d|[A-Za-z]\^[0-9]|[²³]|∠|△|π|°"
)
MATH_SOURCE_RE = re.compile(r"数学|代数|几何|math|algebra|geometry|calculus", re.I)
REPO_ROOT = Path(__file__).resolve().parents[1]


def text_looks_like_formula(text: str) -> bool:
    if not text:
        return False
    return bool(FORMULA_SYMBOL_RE.search(text) or FORMULA_ZH_RE.search(text))


def sort_categories(cats: set[str] | list[str]) -> list[str]:
    return sorted(
        set(cats),
        key=lambda c: REQUIRED_CATEGORIES.index(c) if c in REQUIRED_CATEGORIES else 99,
    )


def now_iso() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def die(message: str, code: int = 2) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(code)


def resolve_path(path: str | Path, base: Path | None = None) -> Path:
    p = Path(path).expanduser()
    if not p.is_absolute():
        p = (base or Path.cwd()) / p
    return p.resolve()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def source_id(path: Path) -> str:
    digest = hashlib.sha1(str(path).encode()).hexdigest()[:10]
    # Keep ASCII stem when present; Chinese-only names fall back to hash id.
    ascii_stem = re.sub(r"[^A-Za-z0-9._-]+", "", path.stem)[:24]
    return f"{ascii_stem}-{digest}" if ascii_stem else f"doc-{digest}"


def find_lit(explicit: str | None) -> str:
    candidates: list[str] = []
    if explicit:
        candidates.append(explicit)
    env = os.environ.get("LIT")
    if env:
        candidates.append(env)
    candidates.append(str(REPO_ROOT / ".venv-liteparse" / "bin" / "lit"))
    which = shutil.which("lit")
    if which:
        candidates.append(which)
    for cand in candidates:
        path = Path(cand).expanduser()
        if path.is_file() and os.access(path, os.X_OK):
            return str(path.resolve())
    die(
        "lit not found. Install LiteParse, e.g.\n"
        "  python3 -m venv .venv-liteparse && "
        ".venv-liteparse/bin/pip install liteparse\n"
        "or pass --lit /path/to/lit"
    )


def run_lit(lit: str, args: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    cmd = [lit, *args]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if check and result.returncode != 0:
        # is-complex exits non-zero when any page needs OCR; treat that as success if JSON ok.
        if args and args[0] == "is-complex" and result.stdout.strip().startswith("["):
            return result
        die(
            f"command failed ({result.returncode}): {' '.join(cmd)}\n"
            f"{result.stderr or result.stdout}"
        )
    return result


def lit_version(lit: str) -> str:
    result = run_lit(lit, ["--version"])
    return (result.stdout or result.stderr).strip() or "unknown"


def ensure_layout(out: Path) -> None:
    for rel in (
        "inputs",
        "work/probe",
        "work/liteparse",
        "work/screenshots",
        "preview",
        "truth",
        "review",
    ):
        (out / rel).mkdir(parents=True, exist_ok=True)


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def load_sources(path: Path) -> list[dict[str, Any]]:
    text = path.read_text(encoding="utf-8")
    if path.suffix.lower() == ".csv":
        rows = list(csv.DictReader(text.splitlines()))
        sources = []
        for row in rows:
            pdf = (row.get("source_pdf") or "").strip()
            if not pdf:
                continue
            sources.append(
                {
                    "source_pdf": pdf,
                    "max_pages_scan": int(row["max_pages_scan"])
                    if row.get("max_pages_scan")
                    else None,
                    "note": (row.get("note") or "").strip(),
                }
            )
        return sources
    sources = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        sources.append({"source_pdf": line, "max_pages_scan": None, "note": ""})
    return sources


def load_selection_csv(path: Path) -> list[dict[str, Any]]:
    rows = list(csv.DictReader(path.read_text(encoding="utf-8").splitlines()))
    out = []
    for row in rows:
        pdf = (row.get("source_pdf") or "").strip()
        page = (row.get("source_page") or "").strip()
        if not pdf or not page:
            continue
        out.append(
            {
                "source_pdf": pdf,
                "source_page": int(page),
                "note": (row.get("note") or "").strip(),
                "suggested_categories": [
                    c.strip()
                    for c in (row.get("categories") or "").split(",")
                    if c.strip()
                ],
            }
        )
    if not out:
        die(f"selection CSV empty or invalid: {path}")
    return out


def write_selection_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(
            fh, fieldnames=["source_pdf", "source_page", "categories", "note"]
        )
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    "source_pdf": row["source_pdf"],
                    "source_page": row["source_page"],
                    "categories": ",".join(row.get("suggested_categories") or []),
                    "note": row.get("note") or "",
                }
            )


def relative_display(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(REPO_ROOT))
    except ValueError:
        return str(path.resolve())


def suggest_categories(page: dict[str, Any], source_pdf: str = "") -> list[str]:
    reasons = set(page.get("reasons") or [])
    layout = page.get("layout") or {}
    layout_reasons = set(layout.get("reasons") or [])
    text_length = int(page.get("text_length") or 0)
    figure_count = int(layout.get("figure_count") or 0)
    table_hits = (
        "table-likely" in layout_reasons
        or int(layout.get("ruled_table_count") or 0) > 0
        or int(layout.get("text_table_run_count") or 0) > 0
    )
    visual_hits = (
        figure_count > 0
        or "dense-graphics" in layout_reasons
        or bool(page.get("has_substantial_images"))
        or int(page.get("image_block_count") or 0) > 0
    )
    multi = "multi-column" in layout_reasons or int(layout.get("column_count") or 1) >= 2
    cats: list[str] = []
    if table_hits:
        cats.append("table")
    if visual_hits:
        cats.append("visual")
    if multi:
        cats.append("mixed")
    if table_hits and visual_hits:
        if "mixed" not in cats:
            cats.append("mixed")
    blankish = text_length < 20 and not table_hits and figure_count == 0 and not visual_hits
    if blankish or ("no-text" in reasons and not visual_hits and not table_hits):
        cats.append("blank")
    if "garbled" in reasons and not table_hits:
        cats.append("failure")
    # Math textbooks: non-blank content pages are strong formula candidates even
    # before OCR (is-complex has no formula signal).
    if MATH_SOURCE_RE.search(source_pdf or "") and not blankish and "blank" not in cats:
        cats.append("formula")
    if not cats:
        cats.append("text")
    elif "blank" not in cats and "failure" not in cats and "text" not in cats:
        if not table_hits and not visual_hits and "formula" not in cats:
            cats.append("text")
    if cats == ["mixed"] and not table_hits and not visual_hits:
        cats = ["text"]
    return sort_categories(cats)


def cmd_init(args: argparse.Namespace) -> int:
    out = resolve_path(args.out)
    ensure_layout(out)
    set_id = args.set_id or out.name
    manifest_path = out / "manifest.json"
    if not manifest_path.exists():
        write_json(
            manifest_path,
            {
                "schema_version": 1,
                "set_id": set_id,
                "page_count": 0,
                "input_sha256": "",
                "renderer": {"dpi": args.dpi, "pixel_budget": 1048576},
                "prompt_revision": "",
                "adapter_revision": "",
                "runtime_revision": "",
                "model_revision": "",
                "license": {
                    "redistributable": False,
                    "provenance": "bootstrap draft; fill before release gate",
                },
                "required_categories": REQUIRED_CATEGORIES,
                "bootstrap": {
                    "selector": "liteparse",
                    "status": "draft",
                    "created_at": now_iso(),
                },
            },
        )
    print(json.dumps({"ok": True, "out": str(out)}, ensure_ascii=False))
    return 0


def cmd_probe(args: argparse.Namespace) -> int:
    lit = find_lit(args.lit)
    out = resolve_path(args.out)
    ensure_layout(out)
    sources_path = resolve_path(args.sources) if args.sources else out / "sources.txt"
    if not sources_path.is_file():
        die(f"sources file not found: {sources_path}")
    sources = load_sources(sources_path)
    if not sources:
        die("no PDF sources listed")
    shutil.copy2(sources_path, out / sources_path.name)

    report_pages = []
    acquisition = ["# Source acquisition", ""]
    for item in sources:
        pdf = resolve_path(item["source_pdf"], REPO_ROOT)
        if not pdf.is_file():
            die(f"PDF not found: {pdf}")
        sid = source_id(pdf)
        digest = sha256_file(pdf)
        lit_args = ["is-complex", str(pdf), "--compact"]
        if item.get("max_pages_scan"):
            lit_args.extend(["--max-pages", str(item["max_pages_scan"])])
        result = run_lit(lit, lit_args, check=True)
        pages = json.loads(result.stdout)
        payload = {
            "source_pdf": relative_display(pdf),
            "source_abs": str(pdf),
            "source_id": sid,
            "sha256": digest,
            "liteparse_version": lit_version(lit),
            "probed_at": now_iso(),
            "pages": pages,
        }
        write_json(out / "work" / "probe" / f"{sid}.json", payload)
        acquisition.append(f"- `{relative_display(pdf)}`")
        acquisition.append(f"  - sha256: `{digest}`")
        acquisition.append(f"  - pages_probed: {len(pages)}")
        acquisition.append(f"  - redistributable: false")
        acquisition.append("")
        report_pages.append({"source_id": sid, "pages": len(pages)})
        print(f"probed {relative_display(pdf)}: {len(pages)} pages", file=sys.stderr)

    (out / "inputs" / "acquisition.md").write_text(
        "\n".join(acquisition) + "\n", encoding="utf-8"
    )
    print(json.dumps({"ok": True, "sources": report_pages}, ensure_ascii=False))
    return 0


def iter_probe_records(out: Path) -> list[dict[str, Any]]:
    records = []
    for path in sorted((out / "work" / "probe").glob("*.json")):
        payload = read_json(path)
        for page in payload["pages"]:
            records.append(
                {
                    "source_pdf": payload["source_pdf"],
                    "source_abs": payload.get("source_abs")
                    or str(resolve_path(payload["source_pdf"], REPO_ROOT)),
                    "source_id": payload["source_id"],
                    "source_sha256": payload["sha256"],
                    "source_page": int(page["page_number"]),
                    "probe": page,
                    "suggested_categories": suggest_categories(
                        page, source_pdf=payload.get("source_pdf") or ""
                    ),
                }
            )
    return records


def pick_pages(
    records: list[dict[str, Any]],
    target_pages: int,
    min_page_gap: int,
) -> list[dict[str, Any]]:
    # Fallbacks for categories probe rarely emits on scanned PDFs.
    for rec in records:
        cats = set(rec["suggested_categories"])
        probe = rec["probe"]
        layout = probe.get("layout") or {}
        if "truncated" not in cats and (
            float(probe.get("image_coverage") or 0) > 0.4
            or bool(probe.get("full_page_image"))
        ):
            # Dense raster page — good long-output / truncate stress candidate.
            cats.add("truncated")
        if "failure" not in cats and "garbled" in set(probe.get("reasons") or []):
            cats.add("failure")
        if "text" not in cats and not (
            cats & {"blank", "failure", "table", "visual", "formula"}
        ):
            cats.add("text")
        if int(layout.get("figure_count") or 0) == 0 and not (
            cats & {"table", "visual", "blank", "failure"}
        ):
            cats.add("text")
        rec["suggested_categories"] = sort_categories(cats)

    by_cat: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for rec in records:
        for cat in rec["suggested_categories"]:
            by_cat[cat].append(rec)

    selected: list[dict[str, Any]] = []
    selected_keys: set[tuple[str, int]] = set()
    last_page_by_pdf: dict[str, int] = {}

    def can_add(rec: dict[str, Any], relax: bool = False) -> bool:
        key = (rec["source_pdf"], rec["source_page"])
        if key in selected_keys:
            return False
        if relax:
            return True
        prev = last_page_by_pdf.get(rec["source_pdf"])
        if prev is not None and abs(rec["source_page"] - prev) < min_page_gap:
            return False
        return True

    def add(rec: dict[str, Any], note: str) -> None:
        key = (rec["source_pdf"], rec["source_page"])
        if key in selected_keys:
            return
        item = dict(rec)
        item["note"] = note
        selected.append(item)
        selected_keys.add(key)
        last_page_by_pdf[rec["source_pdf"]] = rec["source_page"]

    for cat, need in DEFAULT_QUOTA.items():
        pool = sorted(by_cat.get(cat, []), key=lambda r: (r["source_pdf"], r["source_page"]))
        if not pool:
            continue
        # Spread across the pool instead of always taking the first pages.
        if len(pool) <= need:
            stride_indexes = list(range(len(pool)))
        else:
            stride_indexes = [
                min(len(pool) - 1, round(i * (len(pool) - 1) / max(need - 1, 1)))
                for i in range(need)
            ]
            # unique preserve order
            seen_i: set[int] = set()
            uniq = []
            for i in stride_indexes:
                if i not in seen_i:
                    seen_i.add(i)
                    uniq.append(i)
            stride_indexes = uniq
        ordered = [pool[i] for i in stride_indexes] + [
            rec for i, rec in enumerate(pool) if i not in set(stride_indexes)
        ]
        got = 0
        for relax in (False, True):
            for rec in ordered:
                if got >= need:
                    break
                if can_add(rec, relax=relax):
                    add(rec, f"quota:{cat}")
                    got += 1
            if got >= need:
                break

    leftovers = sorted(records, key=lambda r: (r["source_pdf"], r["source_page"]))
    # Prefer top-up also spread: take every k-th remaining page.
    remaining = [r for r in leftovers if (r["source_pdf"], r["source_page"]) not in selected_keys]
    if remaining and len(selected) < target_pages:
        need = target_pages - len(selected)
        step = max(1, len(remaining) // need)
        spread = remaining[::step] + remaining
        for relax in (False, True):
            for rec in spread:
                if len(selected) >= target_pages:
                    break
                if can_add(rec, relax=relax):
                    add(rec, "top-up" if not relax else "top-up-relaxed")
            if len(selected) >= target_pages:
                break

    selected.sort(key=lambda r: (r["source_pdf"], r["source_page"]))
    return selected[:target_pages]


def coverage_counts(rows: list[dict[str, Any]]) -> dict[str, int]:
    counts = {c: 0 for c in REQUIRED_CATEGORIES}
    for row in rows:
        for cat in row.get("suggested_categories") or []:
            if cat in counts:
                counts[cat] += 1
    return counts


def write_coverage(out: Path, rows: list[dict[str, Any]]) -> dict[str, Any]:
    counts = coverage_counts(rows)
    gaps = {c: max(0, 2 - n) for c, n in counts.items()}
    lines = [
        "# Category coverage (suggested)",
        "",
        "| category | suggested pages | still need (≥2) |",
        "|---|---:|---:|",
    ]
    for cat in REQUIRED_CATEGORIES:
        lines.append(f"| {cat} | {counts[cat]} | {gaps[cat]} |")
    lines.append("")
    if any(gaps.values()):
        lines.append("Gaps remain — edit `selection.csv` or re-run select after adding PDFs.")
    else:
        lines.append("Suggested coverage meets ≥2 per category. Human must still confirm.")
    (out / "review" / "coverage.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return {"counts": counts, "gaps": gaps}


def write_rationale(out: Path, rows: list[dict[str, Any]]) -> None:
    lines = ["# Selection rationale", ""]
    for idx, row in enumerate(rows, start=1):
        probe = row.get("probe") or {}
        reasons = ",".join(probe.get("reasons") or []) or "-"
        layout = ",".join((probe.get("layout") or {}).get("reasons") or []) or "-"
        cats = ",".join(row.get("suggested_categories") or [])
        lines.append(
            f"- page-{idx:04d} ← {row['source_pdf']} p{row['source_page']}: "
            f"cats=[{cats}] probe=[{reasons}] layout=[{layout}] note={row.get('note','')}"
        )
    (out / "review" / "selection-rationale.md").write_text(
        "\n".join(lines) + "\n", encoding="utf-8"
    )


def write_page_map(out: Path, rows: list[dict[str, Any]], lit_ver: str) -> None:
    pages = []
    for idx, row in enumerate(rows, start=1):
        pages.append(
            {
                "page_number": idx,
                "source_pdf": row["source_pdf"],
                "source_page": row["source_page"],
                "source_sha256": row.get("source_sha256")
                or sha256_file(resolve_path(row["source_pdf"], REPO_ROOT)),
                "suggested_categories": row.get("suggested_categories") or [],
                "probe_reasons": (row.get("probe") or {}).get("reasons") or [],
                "layout_reasons": ((row.get("probe") or {}).get("layout") or {}).get("reasons")
                or [],
                "note": row.get("note") or "",
            }
        )
    write_json(
        out / "page-map.json",
        {
            "schema_version": 1,
            "selector": "liteparse",
            "liteparse_version": lit_ver,
            "pages": pages,
        },
    )


def cmd_select(args: argparse.Namespace) -> int:
    lit = find_lit(args.lit)
    out = resolve_path(args.out)
    ensure_layout(out)
    lit_ver = lit_version(lit)

    if args.from_csv:
        rows = load_selection_csv(resolve_path(args.from_csv))
        # Enrich with probe if available.
        probe_index = {
            (r["source_pdf"], r["source_page"]): r for r in iter_probe_records(out)
        }
        enriched = []
        for row in rows:
            key = (row["source_pdf"], row["source_page"])
            # also try absolute/relative variants
            abs_pdf = relative_display(resolve_path(row["source_pdf"], REPO_ROOT))
            rec = probe_index.get(key) or probe_index.get((abs_pdf, row["source_page"]))
            item = dict(row)
            if rec:
                item["probe"] = rec["probe"]
                item["source_sha256"] = rec["source_sha256"]
                if not item.get("suggested_categories"):
                    item["suggested_categories"] = rec["suggested_categories"]
            elif not item.get("suggested_categories"):
                item["suggested_categories"] = ["text"]
            enriched.append(item)
        rows = enriched
    else:
        records = iter_probe_records(out)
        if not records:
            die("no probe data; run probe first")
        rows = pick_pages(records, args.target_pages, args.min_page_gap)

    write_selection_csv(out / "selection.csv", rows)
    write_page_map(out, rows, lit_ver)
    write_rationale(out, rows)
    coverage = write_coverage(out, rows)

    manifest = read_json(out / "manifest.json") if (out / "manifest.json").exists() else {}
    manifest.update(
        {
            "schema_version": 1,
            "set_id": args.set_id or out.name,
            "page_count": len(rows),
            "renderer": {"dpi": args.dpi, "pixel_budget": 1048576},
            "required_categories": REQUIRED_CATEGORIES,
            "license": manifest.get("license")
            or {"redistributable": False, "provenance": "bootstrap draft"},
            "bootstrap": {
                "selector": "liteparse",
                "liteparse_version": lit_ver,
                "status": "draft",
                "updated_at": now_iso(),
            },
        }
    )
    write_json(out / "manifest.json", manifest)

    print(
        json.dumps(
            {
                "ok": True,
                "selected": len(rows),
                "coverage": coverage,
            },
            ensure_ascii=False,
        )
    )
    return 0


def pages_spec(pages: list[int]) -> str:
    return ",".join(str(p) for p in pages)


def load_current_selection(out: Path) -> list[dict[str, Any]]:
    sel = out / "selection.csv"
    if not sel.is_file():
        die("selection.csv missing; run select first")
    rows = load_selection_csv(sel)
    page_map = out / "page-map.json"
    if page_map.is_file():
        mapped = {p["page_number"]: p for p in read_json(page_map)["pages"]}
        for idx, row in enumerate(rows, start=1):
            meta = mapped.get(idx)
            if meta:
                row["source_sha256"] = meta.get("source_sha256")
                if not row.get("suggested_categories"):
                    row["suggested_categories"] = meta.get("suggested_categories") or []
                row["probe_reasons"] = meta.get("probe_reasons") or []
    return rows


def cmd_screenshot(args: argparse.Namespace) -> int:
    lit = find_lit(args.lit)
    out = resolve_path(args.out)
    ensure_layout(out)
    rows = load_current_selection(out)

    by_pdf: dict[str, list[tuple[int, int]]] = defaultdict(list)
    for idx, row in enumerate(rows, start=1):
        by_pdf[row["source_pdf"]].append((idx, row["source_page"]))

    for pdf_rel, items in by_pdf.items():
        pdf = resolve_path(pdf_rel, REPO_ROOT)
        sid = source_id(pdf)
        shot_dir = out / "work" / "screenshots" / sid
        if shot_dir.exists():
            shutil.rmtree(shot_dir)
        shot_dir.mkdir(parents=True, exist_ok=True)
        source_pages = sorted({p for _, p in items})
        run_lit(
            lit,
            [
                "screenshot",
                str(pdf),
                "--target-pages",
                pages_spec(source_pages),
                "--dpi",
                str(args.dpi),
                "-o",
                str(shot_dir),
                "-q",
            ],
        )
        for reg_page, src_page in items:
            src = shot_dir / f"page_{src_page}.png"
            if not src.is_file():
                # some builds zero-pad
                alt = shot_dir / f"page_{src_page:04d}.png"
                src = alt if alt.is_file() else src
            if not src.is_file():
                die(f"screenshot missing for {pdf_rel} page {src_page}: expected {src}")
            dest = out / "preview" / f"page-{reg_page:04d}.png"
            shutil.copy2(src, dest)
            print(f"preview page-{reg_page:04d}.png ← {pdf_rel} p{src_page}", file=sys.stderr)

    print(json.dumps({"ok": True, "previews": len(rows)}, ensure_ascii=False))
    return 0


def md_table_to_html(table_lines: list[str]) -> str:
    rows = []
    for line in table_lines:
        line = line.strip().strip("|")
        if re.match(r"^[\s|:-]+$", "|" + line + "|"):
            continue
        cells = [c.strip() for c in line.split("|")]
        rows.append(cells)
    if not rows:
        return ""
    html = ["<table>"]
    for i, cells in enumerate(rows):
        tag = "th" if i == 0 else "td"
        html.append("<tr>" + "".join(f"<{tag}>{c}</{tag}>" for c in cells) + "</tr>")
    html.append("</table>")
    return "".join(html)


def markdown_to_blocks(md: str) -> tuple[list[dict[str, Any]], list[str]]:
    needs = ["reading_order", "categories"]
    blocks: list[dict[str, Any]] = []
    lines = md.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    buf: list[str] = []
    i = 0

    def flush_text() -> None:
        nonlocal buf
        text = "\n".join(buf).strip("\n")
        if text.strip():
            blocks.append({"type": "markdown", "content": text + ("\n" if not text.endswith("\n") else "")})
        buf = []

    while i < len(lines):
        line = lines[i]
        # fenced formula $$
        if line.strip().startswith("$$"):
            flush_text()
            if line.strip() == "$$" or not line.strip().endswith("$$") or line.strip().count("$$") == 1:
                chunk = [line]
                i += 1
                while i < len(lines):
                    chunk.append(lines[i])
                    if "$$" in lines[i] and i > 0:
                        break
                    i += 1
                body = "\n".join(chunk)
                latex = body.replace("$$", "").strip()
                blocks.append({"type": "formula", "latex": latex, "display": True})
                needs.append("formula_latex")
                i += 1
                continue
            latex = line.strip()[2:-2].strip()
            blocks.append({"type": "formula", "latex": latex, "display": True})
            needs.append("formula_latex")
            i += 1
            continue
        # pipe table
        if "|" in line and i + 1 < len(lines) and re.search(r"\|?\s*:?-{3,}", lines[i + 1]):
            flush_text()
            table_lines = [line]
            i += 1
            while i < len(lines) and "|" in lines[i]:
                table_lines.append(lines[i])
                i += 1
            html = md_table_to_html(table_lines)
            if html:
                blocks.append({"type": "html_table", "html": html})
                needs.append("table_html")
            continue
        # image placeholder
        img = re.search(r"!\[[^\]]*\]\(([^)]+)\)|<img\s+[^>]*src=\"([^\"]+)\"", line)
        if img:
            flush_text()
            blocks.append(
                {
                    "type": "visual_region",
                    "bbox": {"left": 0, "top": 0, "right": 1, "bottom": 1},
                    "source_order": sum(1 for b in blocks if b.get("type") == "visual_region"),
                }
            )
            needs.append("bbox")
            i += 1
            continue
        buf.append(line)
        i += 1
    flush_text()

    # inline / Chinese formula heuristic on markdown blocks
    joined = "\n".join(b.get("content", "") for b in blocks if b.get("type") == "markdown")
    if text_looks_like_formula(joined):
        needs.append("formula_latex")

    # dedupe needs preserving order
    seen = set()
    needs_u = []
    for n in needs:
        if n not in seen:
            seen.add(n)
            needs_u.append(n)
    return blocks, needs_u


def enrich_categories_from_blocks(cats: list[str], blocks: list[dict[str, Any]], md: str) -> list[str]:
    cats = list(cats)
    types = {b.get("type") for b in blocks}
    if "html_table" in types and "table" not in cats:
        cats.append("table")
    if "visual_region" in types and "visual" not in cats:
        cats.append("visual")
    blob = md or ""
    blob += "\n" + "\n".join(
        (b.get("content") or b.get("latex") or "") for b in blocks
    )
    if "formula" in types or text_looks_like_formula(blob):
        if "formula" not in cats:
            cats.append("formula")
    if len([c for c in ("table", "visual", "formula") if c in cats]) >= 2 and "mixed" not in cats:
        cats.append("mixed")
    if not any(b.get("type") == "markdown" and (b.get("content") or "").strip() for b in blocks):
        if "blank" not in cats and "failure" not in cats:
            cats.append("blank")
    return sort_categories(cats)


def cmd_draft(args: argparse.Namespace) -> int:
    lit = find_lit(args.lit)
    out = resolve_path(args.out)
    ensure_layout(out)
    rows = load_current_selection(out)
    lit_ver = lit_version(lit)
    ocr_lang = args.ocr_language

    by_pdf: dict[str, list[tuple[int, dict[str, Any]]]] = defaultdict(list)
    for idx, row in enumerate(rows, start=1):
        by_pdf[row["source_pdf"]].append((idx, row))

    for pdf_rel, items in by_pdf.items():
        pdf = resolve_path(pdf_rel, REPO_ROOT)
        sid = source_id(pdf)
        parse_dir = out / "work" / "liteparse" / sid
        parse_dir.mkdir(parents=True, exist_ok=True)
        # Parse page-by-page so one failure does not kill the batch.
        for reg_page, row in items:
            src_page = row["source_page"]
            md_path = parse_dir / f"page-{src_page:04d}.md"
            print(
                f"draft parse {pdf_rel} p{src_page} → truth/page-{reg_page:04d}.json",
                file=sys.stderr,
            )
            result = run_lit(
                lit,
                [
                    "parse",
                    str(pdf),
                    "--target-pages",
                    str(src_page),
                    "--format",
                    "markdown",
                    "--ocr-language",
                    ocr_lang,
                    "--image-mode",
                    "placeholder",
                    "-o",
                    str(md_path),
                    "-q",
                ],
                check=False,
            )
            if result.returncode != 0 and not md_path.is_file():
                md = ""
                termination = "invalid"
                warnings: list[str] = []
                cats = sorted(set((row.get("suggested_categories") or []) + ["failure"]))
                blocks: list[dict[str, Any]] = []
                needs = ["reading_order", "categories", "formula_latex", "table_html", "bbox"]
            else:
                md = md_path.read_text(encoding="utf-8") if md_path.is_file() else (result.stdout or "")
                md_path.write_text(md, encoding="utf-8")
                blocks, needs = markdown_to_blocks(md)
                termination = "eos"
                warnings = []
                cats = enrich_categories_from_blocks(row.get("suggested_categories") or [], blocks, md)
                if not md.strip():
                    cats = sorted(set(cats + ["blank"]))

            doc = {
                "schema_version": 1,
                "page_number": reg_page,
                "categories": cats,
                "blocks": blocks,
                "termination": termination,
                "warnings": warnings,
                "review_status": "draft",
                "bootstrap": {
                    "source": "liteparse",
                    "liteparse_version": lit_ver,
                    "generated_at": now_iso(),
                    "source_pdf": pdf_rel,
                    "source_page": src_page,
                    "needs_human": needs,
                },
            }
            write_json(out / "truth" / f"page-{reg_page:04d}.json", doc)

    write_checklist(out, rows)
    write_coverage(
        out,
        [
            {
                "suggested_categories": read_json(out / "truth" / f"page-{i:04d}.json").get(
                    "categories", []
                )
            }
            for i in range(1, len(rows) + 1)
        ],
    )
    print(json.dumps({"ok": True, "draft_pages": len(rows)}, ensure_ascii=False))
    return 0


def write_checklist(out: Path, rows: list[dict[str, Any]]) -> None:
    statuses = {"draft": 0, "in_review": 0, "approved": 0, "rejected": 0}
    lines = [
        f"# 校准清单 — {out.name}",
        "",
    ]
    table = [
        "| 回归页 | 预览 | 真值 JSON | 建议类别 | 状态 | 你要做的事 |",
        "|---:|---|---|---|---|---|",
    ]
    for idx, row in enumerate(rows, start=1):
        truth_path = out / "truth" / f"page-{idx:04d}.json"
        status = "missing"
        cats = ",".join(row.get("suggested_categories") or [])
        todo = "生成草稿后校对"
        if truth_path.is_file():
            doc = read_json(truth_path)
            status = doc.get("review_status", "draft")
            cats = ",".join(doc.get("categories") or cats.split(","))
            needs = doc.get("bootstrap", {}).get("needs_human") or []
            todo = "对图校对: " + (", ".join(needs) if needs else "全文")
        if status in statuses:
            statuses[status] += 1
        table.append(
            f"| {idx} | preview/page-{idx:04d}.png | truth/page-{idx:04d}.json | {cats} | {status} | {todo} |"
        )
    lines.append(
        f"进度：approved {statuses['approved']} / {len(rows)} · "
        f"draft {statuses['draft']} · in_review {statuses['in_review']} · "
        f"rejected {statuses['rejected']}"
    )
    lines.append("")
    lines.extend(table)
    (out / "review" / "checklist.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def cmd_status(args: argparse.Namespace) -> int:
    out = resolve_path(args.out)
    if not out.is_dir():
        die(f"out dir missing: {out}")
    rows = load_current_selection(out) if (out / "selection.csv").is_file() else []
    write_checklist(out, rows)
    truth_cats = []
    for idx in range(1, len(rows) + 1):
        path = out / "truth" / f"page-{idx:04d}.json"
        if path.is_file():
            truth_cats.append({"suggested_categories": read_json(path).get("categories", [])})
        else:
            truth_cats.append({"suggested_categories": rows[idx - 1].get("suggested_categories") or []})
    coverage = write_coverage(out, truth_cats) if rows else {"counts": {}, "gaps": {}}
    approved = 0
    draft = 0
    for path in sorted((out / "truth").glob("page-*.json")):
        st = read_json(path).get("review_status")
        if st == "approved":
            approved += 1
        elif st == "draft":
            draft += 1
    previews = len(list((out / "preview").glob("page-*.png")))
    payload = {
        "selected": len(rows),
        "previews": previews,
        "truth_files": len(list((out / "truth").glob("page-*.json"))),
        "approved": approved,
        "draft": draft,
        "coverage": coverage,
    }
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0


def cmd_export_gate(args: argparse.Namespace) -> int:
    out = resolve_path(args.out)
    gate = resolve_path(args.gate_out)
    rows = load_current_selection(out)
    docs = []
    for idx in range(1, len(rows) + 1):
        path = out / "truth" / f"page-{idx:04d}.json"
        if not path.is_file():
            die(f"missing truth page: {path}")
        doc = read_json(path)
        if doc.get("review_status") != "approved":
            die(f"page {idx} is not approved (status={doc.get('review_status')})")
        docs.append(doc)
    if len(docs) < 30:
        print(
            f"warning: only {len(docs)} approved pages (<30)",
            file=sys.stderr,
        )
    gate.mkdir(parents=True, exist_ok=True)
    (gate / "truth").mkdir(parents=True, exist_ok=True)
    for doc in docs:
        export = {
            k: v
            for k, v in doc.items()
            if k not in {"bootstrap", "review_status"}
        }
        write_json(gate / "truth" / f"page-{doc['page_number']:04d}.json", export)
    manifest = read_json(out / "manifest.json")
    manifest["page_count"] = len(docs)
    write_json(gate / "manifest.json", manifest)
    shutil.copy2(out / "page-map.json", gate / "page-map.json")
    print(json.dumps({"ok": True, "exported": len(docs), "gate_out": str(gate)}, ensure_ascii=False))
    return 0


def write_bootstrap_report(out: Path, lit: str, stages: list[str], extra: dict[str, Any]) -> None:
    write_json(
        out / "bootstrap-report.json",
        {
            "schema_version": 1,
            "created_at": now_iso(),
            "liteparse_version": lit_version(lit),
            "lit_path": lit,
            "stages": stages,
            **extra,
        },
    )


def cmd_bootstrap(args: argparse.Namespace) -> int:
    lit = find_lit(args.lit)
    out = resolve_path(args.out)
    if not args.sources:
        die("bootstrap requires --sources")
    args.set_id = args.set_id or out.name
    cmd_init(args)
    cmd_probe(args)
    cmd_select(args)
    cmd_screenshot(args)
    cmd_draft(args)
    cmd_status(args)
    write_bootstrap_report(
        out,
        lit,
        ["init", "probe", "select", "screenshot", "draft", "status"],
        {
            "sources": str(resolve_path(args.sources)),
            "target_pages": args.target_pages,
            "dpi": args.dpi,
        },
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="Bootstrap truth-set drafts via LiteParse")
    p.add_argument("--lit", help="path to lit binary")
    p.add_argument("--out", required=True, help="output regression set directory")
    p.add_argument("--set-id", help="set id (default: out dir name)")
    p.add_argument("--dpi", type=int, default=300)
    p.add_argument("--sources", help="PDF list .txt or .csv")
    p.add_argument("--target-pages", type=int, default=30)
    p.add_argument("--min-page-gap", type=int, default=2)
    p.add_argument("--from-csv", help="for select: use this selection.csv")
    p.add_argument("--ocr-language", default="chi_sim+eng")
    p.add_argument("--gate-out", help="for export-gate")
    p.add_argument(
        "command",
        nargs="?",
        default="bootstrap",
        choices=[
            "bootstrap",
            "init",
            "probe",
            "select",
            "screenshot",
            "draft",
            "status",
            "export-gate",
        ],
    )
    return p


def main(argv: list[str] | None = None) -> int:
    raw = list(argv) if argv is not None else sys.argv[1:]
    known = {
        "bootstrap",
        "init",
        "probe",
        "select",
        "screenshot",
        "draft",
        "status",
        "export-gate",
    }
    # Support `tool <command> [flags]` and `tool [flags] [command]`.
    if raw and raw[0] in known:
        raw = raw[1:] + [raw[0]]
    parser = build_parser()
    args = parser.parse_args(raw)
    commands = {
        "bootstrap": cmd_bootstrap,
        "init": cmd_init,
        "probe": cmd_probe,
        "select": cmd_select,
        "screenshot": cmd_screenshot,
        "draft": cmd_draft,
        "status": cmd_status,
        "export-gate": cmd_export_gate,
    }
    if args.command == "export-gate" and not args.gate_out:
        die("export-gate requires --gate-out")
    if args.command in {"probe", "bootstrap"} and not args.sources and not (
        resolve_path(args.out) / "sources.txt"
    ).is_file():
        if args.command == "probe":
            die("probe requires --sources")
    return commands[args.command](args)


if __name__ == "__main__":
    raise SystemExit(main())
