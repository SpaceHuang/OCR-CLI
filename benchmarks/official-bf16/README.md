# Official OvisOCR2 BF16 reference

This directory holds the external, non-distributable reference checkpoint used by
the Windows W0.4 quality-regression gate. The `ocrx` CLI does not load this
checkpoint; it is only the BF16 baseline that GGUF `reference` output is scored
against (requirement 17.2.3, NED `0.01`, formula `0.02`, TEDS `0.02`, reading
order `0.01`, bbox `0.02`).

## Provenance

- Repository: `ATH-MaaS/OvisOCR2`
- Revision: `65c619d374b55d4152e85150fc1b003700bc1f0c`
- Architecture: `Qwen3_5ForConditionalGeneration` (`model_type: qwen3_5`)
- Base model: `Qwen/Qwen3.5-0.8B`
- License: Apache-2.0
- dtype: bfloat16, 852,985,920 parameters

The full file list and SHA-256 values are in [`manifest.json`](manifest.json).
`model.safetensors` is ~1.7 GB and is intentionally gitignored.

## Download and verify

```powershell
powershell -ExecutionPolicy Bypass -File scripts/download-official-bf16.ps1
```

The script downloads from `hf-mirror.com` and verifies every file against the
pinned SHA-256 before reporting success.

## Generate the reference

The reference generator lives at
[`scripts/generate-official-bf16-reference.py`](../scripts/generate-official-bf16-reference.py).
It requires Python 3.11+, `torch`, `torchvision`, `transformers`, `accelerate`,
`safetensors`, `pillow`, and `pymupdf`.

```powershell
python scripts/generate-official-bf16-reference.py `
  --model-dir benchmarks/official-bf16/ATH-MaaS-OvisOCR2-65c619d374b55d4152e85150fc1b003700bc1f0c `
  --input benchmarks/fixtures/pdf/figure-1-page.pdf `
  --output-dir benchmarks/official-bf16/reference `
  --dpi 300
```

It emits scorer-compatible `page-0001.json` files. The prompt and adapter parsing
rules are ported 1:1 from `src/ocrx-ovisocr2/src/lib.rs`.

## Rendering parity note

`ocrx` renders pages with PDFium at the requested DPI and then scales them to the
fixed `2880 * 2880` pixel budget using the image crate's `Triangle` filter. This
generator reproduces that budget with PyMuPDF + PIL bilinear resize when given a
PDF. For byte-identical input, render once with `ocrx scan --keep-pages` and pass
the resulting `images/page-XXXX.png` directory as `--input`.
