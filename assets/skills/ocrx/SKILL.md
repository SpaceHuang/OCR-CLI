---
name: ocrx
description: Run local OvisOCR2 PDF OCR safely with OCR-CLI.
version: 0.1.0
source: ocrx
---

# OCR-CLI

1. If this Skill directory contains `bin/ocrx`, invoke that binary with an absolute path and set `OCRX_PDFIUM_LIBRARY` to this Skill's absolute `lib/libpdfium.dylib` when present; otherwise use `ocrx` on PATH.
2. Run `ocrx doctor --report json` first. Do not download models from the Skill.
3. Use absolute paths for the input PDF and output directory.
4. Run non-interactively with `ocrx scan <absolute-input.pdf> --model ovisocr2 --non-interactive`.
5. Use only the OvisOCR2 model and an installed local profile.
6. On exit code `10` (partial): read `result.json` `pages.degenerated`/`truncated` and the per-page `warnings`. Degenerated pages (repetition loop) are retried automatically with `--resume`; pure truncated pages need a fresh rescan with different render/budget parameters. The merged `document.md` marks incomplete pages with a visible `⚠️ ocrx` blockquote line — never present them as final content.
7. On exit code `130`, resume with `--resume`; on model or runtime errors, report the code and stop.
7. For long Markdown, read only the required pages/sections instead of loading the whole file.
8. Keep processing local; do not upload PDFs, OCR text, prompts, tokens, or credentials.
9. Ask for human verification of the final Markdown and extracted images.
