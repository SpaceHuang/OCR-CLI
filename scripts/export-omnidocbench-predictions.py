#!/usr/bin/env python3
"""Export ocrx scan outputs to OmniDocBench prediction markdown files.

OmniDocBench's end2end evaluator expects one `<image stem>.md` per
annotated page (the GT `page_info.image_path` with the extension swapped
to `.md`). Each page of the benchmark is scanned here as its own
single-page PDF, so one ocrx document directory maps to exactly one
prediction file:

- `page-0001.md` is taken verbatim except for embedded bbox-crop image
  links (`![...](images/...)`), which are stripped — the evaluator scores
  text, tables, and formulas, and stray image markdown would pollute
  text matching. `$...$`/`$$...$$` LaTeX and `<table>` HTML stay intact;
- the per-page checkpoint supplies `truncated`/`degenerated` flags;
- coverage against the build mapping must be complete: missing pages are
  emitted as empty files AND listed in the export report (the evaluator
  documents no graceful handling of absent files);
- pages whose PDF did not contain exactly one page are flagged.

Usage:
  export-omnidocbench-predictions.py --scans scans/<profile> \
      --mapping pdfs/mapping.json --out predictions/<profile>
"""
import argparse
import json
import re
import sys
from pathlib import Path

SCHEMA_VERSION = 1
IMAGE_LINK = re.compile(r"!\[[^\]]*\]\([^)]*\)[ \t]*")
MARKDOWN_LINE_TRAIL = re.compile(r"[ \t]+$", re.MULTILINE)
# OvisOCR2 occasionally leaks its empty reasoning block (`<think>\n\n</think>`)
# into the merged markdown. The evaluator would score it as page text, so it
# is stripped deterministically and counted (known ocrx defect, see report).
THINK_BLOCK = re.compile(r"<think\b[^>]*>.*?</think>\s*", re.DOTALL)
STRAY_THINK = re.compile(r"</?think\b[^>]*>\s*")


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--scans", type=Path, required=True,
                        help="ocrx multi-doc scan output root (scan-summary.json)")
    parser.add_argument("--mapping", type=Path, required=True,
                        help="mapping.json produced by build-omnidocbench-pdfs.py")
    parser.add_argument("--out", type=Path, required=True,
                        help="prediction output directory (<stem>.md files)")
    parser.add_argument("--keep-image-links", action="store_true",
                        help="keep bbox-crop image markdown links (default: strip)")
    parser.add_argument("--stems-filter", type=Path, default=None,
                        help="optional file with one stem per line; scan stems outside "
                             "the filter are ignored instead of reported as extra "
                             "(subset exports, e.g. the degeneration corpus)")
    return parser.parse_args(argv)


def read_checkpoint_flags(checkpoint_path: Path) -> dict:
    flags = {"complete": None, "truncated": None, "degenerated": None}
    try:
        text = checkpoint_path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return flags
    for line in text.splitlines():
        key, _, value = line.partition("=")
        if key in flags:
            flags[key] = value.strip() == "true"
    return flags


def clean_markdown(markdown: str, strip_images: bool) -> tuple[str, int]:
    think_count = len(THINK_BLOCK.findall(markdown))
    markdown = THINK_BLOCK.sub("", markdown)
    markdown = STRAY_THINK.sub("", markdown)
    if strip_images:
        markdown = IMAGE_LINK.sub("", markdown)
    return MARKDOWN_LINE_TRAIL.sub("", markdown).strip("\n") + "\n", think_count


def discover_documents(scans_root: Path) -> list[dict]:
    """Return [{input, output_dir, status, exit_code}] from scan artifacts."""
    summary_path = scans_root / "scan-summary.json"
    documents = []
    if summary_path.is_file():
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        for doc in summary.get("documents", []):
            documents.append({
                "input": doc.get("input", ""),
                "output_dir": doc.get("output_dir", ""),
                "status": doc.get("status"),
                "exit_code": doc.get("exit_code"),
            })
        return documents
    for result_path in sorted(scans_root.glob("*/result.json")):
        result = json.loads(result_path.read_text(encoding="utf-8"))
        documents.append({
            "input": result.get("input", ""),
            "output_dir": str(result_path.parent),
            "status": result.get("status"),
            "exit_code": None,
        })
    return documents


def resolve_doc_dir(raw: str, scans_root: Path) -> Path:
    """Resolve a document output dir from scan artifacts.

    result.json / scan-summary.json record `output_dir` relative to the
    ocrx process working directory (usually the benchmark root), while the
    exporter may run from anywhere; scan-summary entries were observed
    absolute in some runs. Try each interpretation, preferring one that
    exists on disk; the directory basename (`<stem>-<hash8>`) is unique.
    """
    path = Path(raw)
    candidates = []
    if path.is_absolute():
        candidates.append(path)
    else:
        candidates.append(Path.cwd() / path)
        candidates.append(scans_root / path.name)
    for candidate in candidates:
        if candidate.is_dir():
            return candidate
    return candidates[0]


def main(argv=None) -> int:
    args = parse_args(argv)
    scans_root = args.scans
    out_dir = args.out
    out_dir.mkdir(parents=True, exist_ok=True)
    strip_images = not args.keep_image_links

    mapping = json.loads(args.mapping.read_text(encoding="utf-8"))
    mapping_stems = {page["stem"] for page in mapping["pages"]}

    documents = discover_documents(scans_root)
    by_stem: dict[str, dict] = {}
    ambiguous: list[str] = []
    for doc in documents:
        stem = Path(doc["input"]).stem
        if stem in by_stem and by_stem[stem]["output_dir"] != doc["output_dir"]:
            ambiguous.append(stem)
        by_stem[stem] = doc

    pages = []
    missing = []
    anomalies = []
    think_total = 0
    for stem in sorted(mapping_stems):
        doc = by_stem.get(stem)
        if doc is None:
            missing.append(stem)
            (out_dir / f"{stem}.md").write_text("", encoding="utf-8")
            pages.append({"stem": stem, "emitted": False, "empty": True, "status": "missing_scan"})
            continue
        doc_dir = resolve_doc_dir(doc["output_dir"], scans_root)
        page_md = doc_dir / "page-0001.md"
        result_path = doc_dir / "result.json"
        result = json.loads(result_path.read_text(encoding="utf-8")) if result_path.is_file() else {}
        page_stats = result.get("pages", {})
        entry = {
            "stem": stem,
            "emitted": False,
            "empty": False,
            "status": doc.get("status"),
            "exit_code": doc.get("exit_code"),
            "pages_total": page_stats.get("total"),
            "pages_completed": page_stats.get("completed"),
            "pages_failed": page_stats.get("failed"),
        }
        entry.update(read_checkpoint_flags(doc_dir / "page-0001.checkpoint"))
        if page_md.is_file():
            markdown, think_count = clean_markdown(page_md.read_text(encoding="utf-8"), strip_images)
            (out_dir / f"{stem}.md").write_text(markdown, encoding="utf-8")
            entry["emitted"] = True
            entry["empty"] = not markdown.strip()
            entry["markdown_bytes"] = len(markdown)
            entry["think_blocks_stripped"] = think_count
            think_total += think_count
        else:
            missing.append(stem)
            (out_dir / f"{stem}.md").write_text("", encoding="utf-8")
            entry["empty"] = True
        if entry["pages_total"] not in (None, 1):
            anomalies.append({"stem": stem, "pages_total": entry["pages_total"]})
        if entry["truncated"] or entry["degenerated"]:
            anomalies.append({
                "stem": stem,
                "truncated": entry["truncated"],
                "degenerated": entry["degenerated"],
            })
        pages.append(entry)

    extra = set(by_stem) - mapping_stems
    if args.stems_filter is not None:
        filter_stems = {line.strip() for line in
                        args.stems_filter.read_text(encoding="utf-8").splitlines()
                        if line.strip()}
        extra -= filter_stems
    extra = sorted(extra)

    report = {
        "schema_version": SCHEMA_VERSION,
        "scans_root": str(scans_root),
        "mapping_path": str(args.mapping),
        "mapping_pages": len(mapping_stems),
        "scan_documents": len(documents),
        "emitted": sum(1 for p in pages if p["emitted"]),
        "empty_predictions": sum(1 for p in pages if p["empty"]),
        "missing": missing,
        "extra_predictions": extra,
        "ambiguous_stems": ambiguous,
        "think_blocks_stripped_total": think_total,
        "anomalies": anomalies,
        "pages": pages,
    }
    report_path = out_dir / "export-report.json"
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

    print(json.dumps({k: report[k] for k in (
        "mapping_pages", "scan_documents", "emitted", "empty_predictions",
        "missing", "extra_predictions", "think_blocks_stripped_total")},
        ensure_ascii=False, indent=2))
    if missing or extra or ambiguous:
        print(f"ERROR: {len(missing)} missing, {len(extra)} extra, "
              f"{len(ambiguous)} ambiguous predictions", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
