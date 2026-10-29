#!/usr/bin/env python3
"""Generate the W0.4 official BF16 OvisOCR2 reference PageDocument JSONs.

This is the external reference generator for the Windows quality-regression gate.
It runs the pinned ``ATH-MaaS/OvisOCR2`` Safetensors checkpoint (see
``benchmarks/official-bf16/manifest.json``) through ``transformers``, renders
the same page images that ``ocrx`` feeds its runtime, and emits scorer-compatible
``page-XXXX.json`` files in the stable ``PageDocument`` schema.

The ``ocrx`` CLI never loads this checkpoint; this script is a benchmarking aid
only and produces the BF16 reference that the GGUF ``reference`` profile is
compared against (requirement 17.2.3).

The default prompt, parsing rules, and termination mapping are ported 1:1 from
``src/ocrx-ovisocr2/src/lib.rs`` so the emitted JSON is comparable with the
adapter output. Any intentional difference (thinking-tag stripping and the
transformers image preprocessor) is documented inline.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


# Byte-for-byte copy of `PROMPT` in src/ocrx-runtime/src/llama_mtmd.rs.
PROMPT = (
    "\nExtract all readable content from the image in natural human reading order and output the "
    "result as a single Markdown document. For charts or images, represent them using an HTML image "
    'tag: <img src="images/bbox_{left}_{top}_{right}_{bottom}.jpg" />, where left, top, right, '
    "bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format "
    "tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\n"
    "Preserve the original text without translation or paraphrasing."
)

SCHEMA_VERSION = 1
MIN_PIXELS = 448 * 448
MAX_PIXELS = 2880 * 2880
MODEL_PIXEL_BUDGET = 2880 * 2880
MAX_OUTPUT_TOKENS = 4096

_BBOX_SPLIT = re.compile(r"""[_."']""")


def normalize_line_endings(value: str) -> str:
    return value.replace("\r\n", "\n").replace("\r", "\n")


def remove_bbox_image_placeholders(raw: str) -> str:
    """Port of ``remove_bbox_image_placeholders`` in the OvisOCR2 adapter."""
    output: list[str] = []
    rest = raw
    while True:
        start = rest.find("<img")
        if start == -1:
            output.append(rest)
            break
        output.append(rest[:start])
        candidate = rest[start:]
        end = candidate.find(">")
        if end == -1:
            output.append(candidate)
            break
        tag = candidate[: end + 1]
        if "bbox_" not in tag:
            output.append(tag)
        rest = candidate[end + 1 :]
    return "".join(output)


def remove_confident_tail_repetition(markdown: str) -> tuple[str, bool]:
    """Port of ``remove_confident_tail_repetition`` in the OvisOCR2 adapter."""
    trimmed = markdown.rstrip()
    if "\n" not in trimmed:
        return markdown, False
    body, last = trimmed.rsplit("\n", 1)
    last_trimmed = last.strip()
    # Byte length, matching Rust's `&str::len` (M-7 drift 2): CJK lines are
    # few characters but many bytes, so a character count here diverged from
    # the adapter exactly on the pages most likely to repeat.
    if len(last_trimmed.encode("utf-8")) < 8:
        return markdown, False
    if "\n" not in body:
        return markdown, False
    prefix, previous = body.rsplit("\n", 1)
    if previous.strip() != last_trimmed:
        return markdown, False
    return f"{prefix.rstrip()}\n{last_trimmed}\n", True


def find_formula_start(value: str) -> tuple[int, bool] | None:
    """Port of ``find_formula_start`` in the OvisOCR2 adapter."""
    candidates: list[tuple[int, bool]] = []
    for delimiter, display in (("$$", True), ("$", False), ("\\(", False)):
        index = value.find(delimiter)
        if index != -1:
            candidates.append((index, display))
    return min(candidates, key=lambda item: item[0]) if candidates else None


def formula_delimiter(value: str) -> tuple[int, str] | None:
    """Port of ``formula_delimiter`` in the OvisOCR2 adapter."""
    if value.startswith("$$"):
        return 2, "$$"
    if value.startswith("\\("):
        return 2, "\\)"
    if value.startswith("$"):
        return 1, "$"
    return None


def parse_structured_blocks(markdown: str) -> list[dict]:
    """Port of ``parse_structured_blocks`` in the OvisOCR2 adapter."""
    blocks: list[dict] = []
    remaining = markdown
    while remaining:
        table_position = remaining.find("<table")
        formula = find_formula_start(remaining)
        if table_position != -1 and formula and formula[0] < table_position:
            start, is_table = formula[0], False
        elif table_position != -1:
            start, is_table = table_position, True
        elif formula:
            start, is_table = formula[0], False
        else:
            start, is_table = -1, False

        if start == -1:
            if remaining.strip():
                blocks.append({"type": "markdown", "content": remaining})
            break

        if start > 0:
            blocks.append({"type": "markdown", "content": remaining[:start]})

        if not is_table:
            # Full delimiter set from the adapter (M-7 drift 1): inline `$`
            # and `\(...\)` formulas become formula blocks with display=False
            # exactly like the Rust parser, instead of staying in markdown.
            opened = formula_delimiter(remaining[start:])
            if opened:
                open_len, close = opened
                formula_start = remaining[start + open_len :]
                end = formula_start.find(close)
                if end != -1:
                    blocks.append(
                        {
                            "type": "formula",
                            "latex": formula_start[:end].strip(),
                            "display": remaining[start:].startswith("$$"),
                        }
                    )
                    remaining = formula_start[end + len(close) :]
                    continue
            blocks.append({"type": "markdown", "content": remaining})
            break

        table_start = remaining[start:]
        end = table_start.find("</table>")
        if end != -1:
            end += len("</table>")
            blocks.append({"type": "html_table", "html": table_start[:end]})
            remaining = table_start[end:]
            continue
        blocks.append({"type": "markdown", "content": remaining})
        break

    return blocks


def _valid_bbox(left: int, top: int, right: int, bottom: int) -> bool:
    return left < right and top < bottom and right < 1000 and bottom < 1000


def _parse_u16_strict(value: str) -> int | None:
    """Rust `u16::from_str` semantics: digits only, no sign, whitespace, or
    leading `+`, range-checked. Python's `int()` accepted spellings the
    adapter rejects (M-7 drift 3)."""
    if not value.isdigit():
        return None
    parsed = int(value)
    return parsed if parsed <= 0xFFFF else None


def extract_bboxes(raw: str) -> list[tuple[int, int, int, int]]:
    """Port of ``extract_bboxes`` in the OvisOCR2 adapter."""
    result: list[tuple[int, int, int, int]] = []
    rest = raw
    while True:
        start = rest.find("bbox_")
        if start == -1:
            break
        candidate = rest[start + 5 :]
        fields = _BBOX_SPLIT.split(candidate)
        if len(fields) >= 4:
            parsed = [_parse_u16_strict(fields[i]) for i in range(4)]
            if all(value is not None for value in parsed):
                bbox = tuple(value for value in parsed if value is not None)
                if _valid_bbox(*bbox):
                    result.append(bbox)
        rest = candidate[1:] if candidate else ""
    return result


def strip_thinking(raw: str) -> str:
    """Remove ``<think>...</think>`` blocks emitted by some transformers paths.

    The ocrx adapter disables thinking; llama.cpp's MTMD prompt has no thinking
    markers. ``transformers`` may still emit a think block even with an empty
    ``enable_thinking=False`` generation prompt, so it is removed here to keep
    the reference comparable with the GGUF ``reference`` output.
    """
    stripped = re.sub(r"<think>.*?</think>", "", raw, flags=re.DOTALL)
    stripped = stripped.replace("<think>", "").replace("</think>", "")
    return stripped


def adapt_response(page_number: int, raw: str, finish_reason: str) -> dict:
    """Port of ``parse_page_with_termination`` in the OvisOCR2 adapter."""
    raw = normalize_line_endings(raw)
    raw = strip_thinking(raw)
    markdown = remove_bbox_image_placeholders(raw)
    markdown, repetition_removed = remove_confident_tail_repetition(markdown)
    blocks = parse_structured_blocks(markdown)

    seen: list[tuple[int, int, int, int]] = []
    for bbox in extract_bboxes(raw):
        if bbox in seen:
            continue
        order = len(seen)
        seen.append(bbox)
        left, top, right, bottom = bbox
        blocks.append(
            {
                "type": "visual_region",
                "bbox": {"left": left, "top": top, "right": right, "bottom": bottom},
                "source_order": order,
            }
        )

    # Mirror the adapter's termination mapping (M-7 drift 4): `stop` and
    # `stop_token` map to stop_token, everything unrecognized falls back to
    # eos — not only an exact `max_tokens`/`eos` split.
    termination = (
        "max_tokens"
        if finish_reason == "max_tokens"
        else "stop_token"
        if finish_reason in ("stop", "stop_token")
        else "eos"
    )
    warnings: list[str] = []
    if termination == "max_tokens":
        warnings.append("truncated")
    if repetition_removed:
        warnings.append("repetition_removed")

    return {
        "schema_version": SCHEMA_VERSION,
        "page_number": page_number,
        "blocks": blocks,
        "termination": termination,
        "warnings": warnings,
    }


def render_pdf_page(
    pdf_path: Path,
    page_index: int,
    dpi: int,
    budget: int = MODEL_PIXEL_BUDGET,
):
    """Render a PDF page to the same RGB8 input shape ``ocrx`` feeds its runtime."""
    import pymupdf as fitz  # PyMuPDF; imported lazily so image-only runs don't require it.

    document = fitz.open(str(pdf_path))
    page = document[page_index]
    matrix = fitz.Matrix(dpi / 72.0, dpi / 72.0)
    pixmap = page.get_pixmap(matrix=matrix)
    from PIL import Image
    import io

    image = Image.open(io.BytesIO(pixmap.tobytes("png"))).convert("RGB")
    width, height = image.size
    pixels = width * height
    if pixels > budget and budget > 0:
        scale = (budget / pixels) ** 0.5
        target_width = max(1, int(width * scale))
        target_height = max(1, int(height * scale))
        image = image.resize((target_width, target_height), Image.Resampling.BILINEAR)
    document.close()
    return image


def load_model(model_dir: str, device: str, dtype_name: str):
    import torch
    from transformers import AutoModelForMultimodalLM, AutoProcessor

    processor = AutoProcessor.from_pretrained(model_dir, local_files_only=True)
    if device == "auto":
        device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.float32 if dtype_name == "float32" else torch.bfloat16
    model = AutoModelForMultimodalLM.from_pretrained(
        model_dir,
        dtype=dtype,
        local_files_only=True,
    )
    if device == "cuda":
        model = model.to("cuda")
    model.eval()
    return processor, model, device


def run_inference(
    processor,
    model,
    image,
    max_new_tokens: int,
    min_pixels: int,
    max_pixels: int,
) -> tuple[str, str]:
    import torch

    messages = [
        {
            "role": "user",
            "content": [{"type": "image"}, {"type": "text", "text": PROMPT}],
        }
    ]
    text = processor.apply_chat_template(
        messages,
        tokenize=False,
        add_generation_prompt=True,
        enable_thinking=False,
    )
    inputs = processor(
        text=[text],
        images=[image],
        return_tensors="pt",
        min_pixels=min_pixels,
        max_pixels=max_pixels,
    )
    if next(model.parameters()).device.type == "cuda":
        inputs = {key: value.to("cuda") for key, value in inputs.items()}
    # The pinned checkpoint ships no generation_config.json, so generate()
    # does not stop at the chat `<|im_end|>` turn delimiter and every page
    # runs to the token budget (W0.4 2026-10-30 run: 30/30 truncated, 83%
    # periodic tails). Pass the stop ids explicitly; sampling stays greedy.
    tokenizer = getattr(processor, "tokenizer", processor)
    stop_ids = {
        tokenizer.convert_tokens_to_ids("<|im_end|>"),
        tokenizer.eos_token_id,
    }
    stop_ids.discard(None)
    with torch.inference_mode():
        outputs = model.generate(
            **inputs,
            max_new_tokens=max_new_tokens,
            do_sample=False,
            eos_token_id=sorted(stop_ids),
        )
    input_length = inputs["input_ids"].shape[1]
    new_tokens = outputs[0][input_length:]
    raw = processor.decode(new_tokens, skip_special_tokens=True)
    finish_reason = "max_tokens" if new_tokens.shape[0] >= max_new_tokens else "eos"
    return raw, finish_reason


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", required=True)
    parser.add_argument(
        "--input",
        required=True,
        help="PDF file or a directory of page-XXXX.png images.",
    )
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--dpi", type=int, default=300)
    parser.add_argument("--pixel-budget", type=int, default=MODEL_PIXEL_BUDGET)
    parser.add_argument("--pages", help="1-based page selection, e.g. 1-3 or 1,2,4")
    parser.add_argument("--max-new-tokens", type=int, default=MAX_OUTPUT_TOKENS)
    parser.add_argument("--min-pixels", type=int, default=MIN_PIXELS)
    parser.add_argument("--max-pixels", type=int, default=MAX_PIXELS)
    parser.add_argument("--device", choices=("auto", "cpu", "cuda"), default="auto")
    parser.add_argument(
        "--dtype",
        choices=("bfloat16", "float32"),
        default="bfloat16",
        help="Inference dtype. Turing GPUs (sm_75) lack native BF16, so float32 "
        "is ~6x faster there and is the recommended practical choice for the "
        "one-time reference baseline (BF16 weights are upcast).",
    )
    args = parser.parse_args()

    model_dir = Path(args.model_dir)
    if not (model_dir / "model.safetensors").is_file():
        print(f"missing model.safetensors under {model_dir}", file=sys.stderr)
        return 2

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    processor, model, device = load_model(str(model_dir), args.device, args.dtype)
    print(
        f"reference generator: device={device} dtype={args.dtype}",
        file=sys.stderr,
    )

    input_path = Path(args.input)
    if input_path.is_dir():
        page_images = {
            int(match.group(1)): path
            for path in input_path.glob("page-*.png")
            if (match := re.search(r"page-(\d+)\.png$", path.name))
        }
        if not page_images:
            print(f"no page-XXXX.png files under {input_path}", file=sys.stderr)
            return 2
        page_numbers = sorted(page_images)
    else:
        page_numbers = list(range(1, _pdf_page_count(input_path) + 1))
        page_images = {}

    selected = _resolve_pages(page_numbers, args.pages)
    for page_number in selected:
        if page_number in page_images:
            from PIL import Image

            image = Image.open(page_images[page_number]).convert("RGB")
        else:
            image = render_pdf_page(input_path, page_number - 1, args.dpi, args.pixel_budget)
        raw, finish_reason = run_inference(
            processor,
            model,
            image,
            args.max_new_tokens,
            args.min_pixels,
            args.max_pixels,
        )
        document = adapt_response(page_number, raw, finish_reason)
        out_path = output_dir / f"page-{page_number:04}.json"
        out_path.write_text(
            json.dumps(document, ensure_ascii=False, separators=(",", ":")),
            encoding="utf-8",
        )
        print(f"wrote {out_path}", file=sys.stderr)
    return 0


def _pdf_page_count(pdf_path: Path) -> int:
    import pymupdf as fitz

    document = fitz.open(str(pdf_path))
    try:
        return document.page_count
    finally:
        document.close()


def _resolve_pages(page_numbers: list[int], selection: str | None) -> list[int]:
    if not selection:
        return page_numbers
    selected: set[int] = set()
    for part in selection.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            start, _, end = part.partition("-")
            selected.update(range(int(start), int(end) + 1))
        else:
            selected.add(int(part))
    return [page for page in page_numbers if page in selected]


if __name__ == "__main__":
    raise SystemExit(main())
