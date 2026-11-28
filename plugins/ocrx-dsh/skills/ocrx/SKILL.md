---
name: ocrx
description: Convert PDF documents to Markdown offline with the local ocrx CLI through the ocrx_scan tool, bootstrap the OCR model on first use with a consented install, and check installation health with ocrx_doctor.
---

# OvisOCR (ocrx) — offline PDF → Markdown

Use the ocrx tools first; the bare `ocrx` CLI is only a fallback when no tool is available.

## Workflow

1. **Start with a scan.** Call `ocrx_scan` with the user's PDF path(s) and an output
   directory. It always runs offline and never downloads anything. For large PDFs pass
   `pages: "1-3"` to sample first, then run the full document. You may also run
   `ocrx_doctor` first when the user asks about installation health.
2. **On `model_missing`**, the OCR model is not installed yet. Do NOT ask the user to open
   a terminal. Confirm the target (the scan result's `readiness.recommend` carries the
   model/profile/size/destination), then call `ocrx_model_install` — it shows a consent
   prompt with the real size and destination before downloading anything.
3. **After a successful install, retry `ocrx_scan` with the SAME arguments plus
   `resume: true`.** Drop `overwrite` if it was set (they are mutually exclusive).
   This batch-retry rule matters: the first failed batch wrote `scan-summary.json`, and a
   retry without `resume` would be rejected with an output error. Do not re-enter the
   conversation loop with the user in between.
4. `ocrx_model` answers catalog questions ("which profile fits this machine?", "how big
   is the download?", "is the install intact?"). `ocrx_doctor` reports installation
   health; its `installed_profiles` field covers exactly one model and misses custom
   cache overrides — display only, never a pre-scan gate.

## Notes

- Results are delivered as a compact summary plus artifact paths. Read the produced
  `result.json` / Markdown files with file tools when the user asks for content; bodies
  are never inlined into scan results.
- `password` (encrypted PDFs) is passed as a tool argument and therefore recorded in the
  session history; warn the user about sensitive documents.
- The default model is OvisOCR3 (quality). Explicit `model`/`modelProfile` choices are
  honored verbatim on install and retry.
- If the model download was refused (no consent / offline config), the tool returns an
  offline-placement guide; relay it instead of retrying.
- Bare CLI fallback (no tools available): `ocrx scan <input> --output-dir <dir> --offline
  --non-interactive --report json`; `ocrx model install [--model <m>] <profile>` downloads
  with real sizes shown by `ocrx model recommend`.
