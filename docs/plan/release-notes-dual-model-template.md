# Release notes template — dual-model (OvisOCR2 + OvisOCR3)

> O3-T23 template: fill the bracketed values from the release freeze tables
> before publishing. The packaged archives contain NO model weights;
> installation is explicit and revision-pinned (provenance verified by
> SHA-256 at install time).

## Highlights

- Two supported OCR models, selected explicitly with `--model`:
  - `ovisocr2` (default; unchanged behavior, profiles `compact` / `balanced` / `quality` / `reference`)
  - `ovisocr3` (new; profile `quality` = Q8_0 + F16 `mmproj`)
- The default model stays **OvisOCR2**. Existing checkpoints, scripts, and
  outputs behave byte-identically to previous releases.
- Cross-model checkpoint isolation: a checkpoint written under one model is
  never reused by a scan of the other model (full rescan).

## Installing the models

```bash
# Default model (as before)
ocrx model install compact          # or balanced / quality / reference

# OvisOCR3 (explicit selector; quality is its only available profile)
ocrx model install --model ovisocr3 quality
ocrx model verify  --model ovisocr3 quality
ocrx model info    --model ovisocr3
```

## Using OvisOCR3

```bash
ocrx scan document.pdf --model ovisocr3
ocrx doctor --model ovisocr3 --report json
```

Every other flag (device, resume, progress, routing, output) behaves
identically for both models. A single scan invocation uses exactly one
model end to end.

## Supply chain (OvisOCR3)

- Upstream model: [ATH-MaaS/OvisOCR3](https://huggingface.co/ATH-MaaS/OvisOCR3)
  (Apache-2.0), technical report [arXiv:2610.33066](https://arxiv.org/abs/2610.33066).
- GGUF conversion: [michalland/OvisOCR3-GGUF](https://huggingface.co/michalland/OvisOCR3-GGUF)
  @ revision `7be644a312005c3a55e2c826aee985a5c93d06b2`
  - model asset: `OvisOCR3-Q8_0.gguf`
    sha256 `11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6`
    (811,843,712 bytes)
  - shared projector: `mmproj-OvisOCR3-F16.gguf`
    sha256 `698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd`
    (204,987,136 bytes)
- The upstream publishes Transformers/Safetensors weights only; the GGUF
  files are a community conversion of that upstream (see
  THIRD_PARTY_NOTICES for the full provenance and backup-repository note).

## Quality & benchmarking notes

- Cross-model comparisons are only meaningful inside this repository's
  harness (same pages, same params); official model-card scores are
  background reference only.
- Backends and quantization must be noted in any published comparison
  (GPU-CPU fallback noise ±3% applies).
