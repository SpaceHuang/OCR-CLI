---
name: ocrx
description: Convert PDF documents to Markdown offline with the local ocrx CLI through the ocrx_scan tool, bootstrap the OCR model on first use, and check installation health.
---

# OvisOCR (ocrx) — offline PDF → Markdown

Use the ocrx tools first; the bare `ocrx` CLI is only a fallback when no tool is available.

## Workflow

1. Call `ocrx_scan` directly with the user's PDF path(s) and an output directory. It always
   runs offline and never downloads anything. For large PDFs, pass `pages: "1-3"` to sample
   first, then run the full document.
2. If the scan fails with `model_missing`, the OCR model is not installed yet. Confirm the
   target with the user, then call `ocrx_model_install` — it asks for explicit consent with
   the real download size and destination before downloading anything.
3. After a successful install, retry `ocrx_scan` with the SAME arguments plus `resume: true`
   (drop `overwrite` if it was set — they are mutually exclusive). Do not re-enter the
   conversation loop with the user in between.
4. Use `ocrx_doctor` for installation health, `ocrx_model` to query catalog/profile
   questions ("which profile fits this machine?", "how big is the download?").

## Notes

- Results are delivered as a compact summary plus artifact paths. Read the produced
  `result.json` / Markdown files with file tools when the user asks for content.
- `password` (encrypted PDFs) is passed as a tool argument and therefore recorded in the
  session history; warn the user about sensitive documents.
- The default model is OvisOCR3 (quality). Explicit `model`/`modelProfile` choices are
  honored verbatim on install and retry.
