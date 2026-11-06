#!/usr/bin/env python3
"""Wrap OmniDocBench page images into single-page PDFs for ocrx scanning.

OmniDocBench (v1.6, dataset revision aa1ee96d) distributes annotated page
images; ocrx accepts PDF input only. This builder wraps every annotated
page image losslessly into a one-page PDF whose MediaBox is sized so that a
300 DPI PDFium render reproduces the source pixel grid exactly (page points
= pixels * 72 / 300):

- JPEG sources are embedded verbatim (no recompression);
- PNG sources are embedded via FlateDecode (lossless);
- anything else (WebP, alpha channels, CMYK JPEG) is flattened to RGB PNG
  and flagged `reencoded` in the mapping.

Pages larger than ocrx's 2880x2880 render budget are still wrapped at
native size; PDFium downscales them proportionally at scan time and the
per-page `page-0001.render-audit` records the ratio. The build report
lists every oversized page.

Outputs (under --out):
- <image stem>.pdf for every annotated page;
- mapping.json: image_path -> {stem, pdf, source, width, height, ...};
- build-report.json: counts, duplicates, missing images, format histogram,
  oversized pages, total bytes.

Usage:
  build-omnidocbench-pdfs.py --gt dataset/OmniDocBench.json \
      --images dataset/images --out pdfs [--limit N] [--stems a,b,c]
"""
import argparse
import io
import json
import sys
from collections import Counter
from pathlib import Path

import img2pdf
from PIL import Image

SCHEMA_VERSION = 1
RENDER_DPI = 300
MAX_RENDER_PIXELS = 2880 * 2880  # ocrx ocrx-pdf MAX_RENDER_PIXELS budget
SOURCE_EXTENSIONS = (".png", ".jpg", ".jpeg", ".webp", ".bmp", ".tif", ".tiff")
IMAGE_LINK_HINTS = ("page images",)


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--gt", type=Path, required=True,
                        help="OmniDocBench.json ground-truth file")
    parser.add_argument("--images", type=Path, required=True,
                        help="dataset images directory")
    parser.add_argument("--out", type=Path, required=True,
                        help="output directory for single-page PDFs")
    parser.add_argument("--limit", type=int, default=None,
                        help="wrap only the first N annotated pages")
    parser.add_argument("--stems", type=str, default=None,
                        help="comma-separated image stems to wrap (smoke runs)")
    parser.add_argument("--force", action="store_true",
                        help="rebuild PDFs that already exist")
    return parser.parse_args(argv)


def resolve_source(images_dir: Path, image_path: str) -> Path | None:
    """Locate the on-disk image for a GT `page_info.image_path` entry.

    The GT names pages `<stem>.jpg` while the published files may be PNG;
    match on stem with an explicit extension fallback list.
    """
    direct = images_dir / image_path
    if direct.is_file():
        return direct
    stem = Path(image_path).stem
    for extension in SOURCE_EXTENSIONS:
        candidate = images_dir / f"{stem}{extension}"
        if candidate.is_file():
            return candidate
    matches = sorted(images_dir.glob(f"{glob_escape(stem)}.*"))
    return matches[0] if len(matches) == 1 else None


def glob_escape(stem: str) -> str:
    return stem.replace("[", "[[]").replace("*", "[*]").replace("?", "[?]")


def normalized_png(raw: bytes) -> bytes:
    """Flatten alpha/CMYK/paletted sources to plain RGB PNG bytes."""
    with Image.open(io.BytesIO(raw)) as image:
        if image.mode == "P":
            image = image.convert("RGBA")
        if image.mode in ("RGBA", "LA", "PA") or "transparency" in image.info:
            background = Image.new("RGB", image.size, (255, 255, 255))
            background.paste(image, mask=image.split()[-1])
            image = background
        elif image.mode != "RGB":
            image = image.convert("RGB")
        buffer = io.BytesIO()
        image.save(buffer, format="PNG", optimize=False)
        return buffer.getvalue()


def needs_reencode(image_format: str, mode: str, info: dict) -> bool:
    """True unless the raw bytes can be embedded losslessly by img2pdf.

    JPEG (RGB/L) embeds verbatim; plain single-channel/RGB PNG embeds via
    FlateDecode. Alpha, palettes, CMYK, and foreign formats are flattened
    to RGB PNG first.
    """
    if mode in ("RGBA", "LA", "PA") or "transparency" in info:
        return True
    if image_format == "JPEG" and mode in ("RGB", "L"):
        return False
    if image_format == "PNG" and mode in ("RGB", "L", "1"):
        return False
    return True


def wrap_image(source: Path, destination: Path) -> dict:
    """Embed one image into a single-page PDF at RENDER_DPI mapping."""
    raw = source.read_bytes()
    with Image.open(io.BytesIO(raw)) as probe:
        width, height = probe.size
        image_format = probe.format or "?"
        reencode = needs_reencode(image_format, probe.mode, probe.info)

    if reencode:
        payload = normalized_png(raw)
        reencoded = True
    else:
        payload = raw
        reencoded = False

    layout = img2pdf.get_fixed_dpi_layout_fun((RENDER_DPI, RENDER_DPI))
    pdf_bytes = img2pdf.convert(payload, layout_fun=layout)
    destination.write_bytes(pdf_bytes)
    return {
        "source": str(source),
        "width": width,
        "height": height,
        "format": image_format,
        "reencoded": reencoded,
        "source_bytes": len(raw),
        "pdf_bytes": len(pdf_bytes),
    }


def main(argv=None) -> int:
    args = parse_args(argv)
    gt_path = args.gt
    images_dir = args.images
    out_dir = args.out
    out_dir.mkdir(parents=True, exist_ok=True)

    with gt_path.open(encoding="utf-8") as handle:
        gt = json.load(handle)

    wanted = []
    seen_stems: dict[str, str] = {}
    duplicates: list[dict] = []
    for page in gt:
        image_path = page.get("page_info", {}).get("image_path")
        if not image_path:
            continue
        stem = Path(image_path).stem
        if stem in seen_stems:
            duplicates.append({"stem": stem, "first": seen_stems[stem], "second": image_path})
            continue
        seen_stems[stem] = image_path
        wanted.append((image_path, stem))

    if args.stems:
        allow = {s.strip() for s in args.stems.split(",") if s.strip()}
        wanted = [(p, s) for p, s in wanted if s in allow]
    if args.limit is not None:
        wanted = wanted[: args.limit]

    mapping_path = out_dir / "mapping.json"
    existing = {}
    if mapping_path.exists() and not args.force:
        try:
            prior = json.loads(mapping_path.read_text(encoding="utf-8"))
            existing = {p["stem"]: p for p in prior.get("pages", [])}
        except (json.JSONDecodeError, KeyError):
            existing = {}

    pages = []
    missing = []
    oversized = []
    formats = Counter()
    total_source_bytes = 0
    total_pdf_bytes = 0
    reencoded_pages = 0
    rebuilt = 0
    reused = 0

    def account(entry: dict) -> None:
        nonlocal total_source_bytes, total_pdf_bytes, reencoded_pages
        formats[entry["format"]] += 1
        total_source_bytes += entry["source_bytes"]
        total_pdf_bytes += entry["pdf_bytes"]
        reencoded_pages += 1 if entry["reencoded"] else 0
        if entry["width"] * entry["height"] > MAX_RENDER_PIXELS:
            oversized.append({"stem": entry["stem"], "width": entry["width"], "height": entry["height"]})

    for image_path, stem in wanted:
        if stem in existing and (existing[stem]["pdf"]) and Path(existing[stem]["pdf"]).is_file():
            entry = existing[stem]
            pages.append(entry)
            reused += 1
        else:
            source = resolve_source(images_dir, image_path)
            if source is None:
                missing.append(image_path)
                continue
            destination = out_dir / f"{stem}.pdf"
            info = wrap_image(source, destination)
            entry = {"image_path": image_path, "stem": stem, "pdf": str(destination), **info}
            pages.append(entry)
            rebuilt += 1
        account(entry)

    mapping = {
        "schema_version": SCHEMA_VERSION,
        "gt_path": str(gt_path),
        "images_dir": str(images_dir),
        "render_dpi": RENDER_DPI,
        "page_count": len(pages),
        "pages": pages,
    }
    mapping_path.write_text(
        json.dumps(mapping, ensure_ascii=False, indent=2), encoding="utf-8")

    report = {
        "schema_version": SCHEMA_VERSION,
        "gt_pages": len(gt),
        "wanted_pages": len(wanted),
        "wrapped": len(pages),
        "rebuilt": rebuilt,
        "reused": reused,
        "missing": missing,
        "duplicates": duplicates,
        "reencoded_pages": reencoded_pages,
        "formats": dict(formats),
        "oversized_pages": oversized,
        "total_source_bytes": total_source_bytes,
        "total_pdf_bytes": total_pdf_bytes,
    }
    (out_dir / "build-report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

    print(json.dumps({k: report[k] for k in (
        "gt_pages", "wanted_pages", "wrapped", "rebuilt", "reused",
        "missing", "duplicates", "reencoded_pages", "formats",
        "oversized_pages", "total_pdf_bytes")},
        ensure_ascii=False, indent=2))
    if missing or duplicates:
        print(f"ERROR: {len(missing)} missing images, {len(duplicates)} duplicate stems",
              file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
