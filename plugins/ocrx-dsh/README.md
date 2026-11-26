# @ocrx/dsh — OvisOCR (ocrx) for DeepSeek Harness

Convert PDF documents to Markdown **offline** with the local `ocrx` CLI, directly inside
[DeepSeek Harness (dsh)](https://github.com/deepseek-ai/deepseek-harness). Install the
plugin, ask your agent to convert a PDF, approve one download — done.

Keywords: `dsh-plugin` · `deepseek-harness` · `ocr` · `pdf-to-markdown`

## 30-second start

Prerequisite: **Node.js** (>= 24.9, required by dsh itself) and **pnpm** on your PATH
(`dsh plugin` forwards to pnpm). If you use dsh, you already have both.

```sh
dsh plugin --profile <your-profile> add @ocrx/dsh
```

Then, in a conversation with your agent:

> convert C:/docs/report.pdf to markdown

First use downloads the OCR model (OvisOCR3 quality, ≈ 1.0 GB) **only after you approve**
the consent prompt with the real size and destination. Every scan runs offline — the
tools never download anything else, ever.

## The four tools

| Tool | What it does |
| --- | --- |
| `ocrx_scan` | PDF → Markdown (files or directories, batch-aware). Always `--offline`. |
| `ocrx_doctor` | Installation health. Display only — never used to pre-gate scans. |
| `ocrx_model` | Catalog queries: list / info / recommend (with exact size + destination) / verify. |
| `ocrx_model_install` | The ONLY download path. Built-in consent gate; refuses without approval. |

`/ocrx` prints a status overview; `/ocrx doctor` runs the self-check. There is
deliberately **no** `/ocrx install-model`: commands run outside turns where no consent
prompt can be shown.

## Configuration (all optional)

```yaml
ocrx:
  command: C:/gpu-build/ocrx.exe   # GPU (CUDA) build or custom binary; bundled wins otherwise
  extraEnv:                        # e.g. OCRX_PDFIUM_LIBRARY, OCRX_TABLE_ENGINE_CMD
    OCRX_PDFIUM_LIBRARY: C:/libs/pdfium.dll
  offline: false                   # true = even consented downloads are refused by the CLI
  defaultTimeoutMs: 1800000        # scan execute timeout (30 min)
  maxTimeoutMs: 14400000           # scan pipeline cap (4 h)
  installTimeoutMs: 0              # unset = no install timeout (downloads cannot resume)
  enableScan: true
  enableDoctor: true
  enableModel: true
  enableModelInstall: true         # false = the tool is NOT registered (no download path at all)
```

Notes:

- The **bundled binary shadows PATH**: if you installed a GPU (CUDA) build yourself, set
  `command` to it — `/ocrx` shows which binary is in effect.
- There is no cache-directory config on purpose: the CLI's `scan` does not honor
  `OCRX_MODEL_CACHE` yet, so a config here could point doctor and scan at different
  caches.

## Trust model (read this)

- ocrx tools run with **your full user file permissions** — they can write any output
  directory and read any input path. There is no harness sandbox around them. Trusting
  this plugin equals trusting a locally installed ocrx.
- `ocrx_model_install` is the only tool that touches the network. It asks for consent
  (real size + destination + "cannot resume" warning) and **refuses to download without
  approval** — including when the approval service is unavailable. `ocrx_scan` always
  runs with `--offline`.
- `password` (the encrypted-PDF open password) is passed as a tool argument: it lands in
  the session history and the model context. There is no redaction channel. If a document
  is highly sensitive, run the bare CLI yourself instead.

## Install / upgrade / uninstall

- Upgrade = `dsh plugin update <profile> @ocrx/dsh` (the bundled binary upgrades with it).
- Uninstall = `dsh plugin remove <profile>`. The plugin writes nothing outside its own
  package (skills come from the provider, not disk). Model caches and any leftover
  `.part` staging files live in your user cache directory, shared with a directly
  installed CLI — uninstalling the plugin does NOT delete them; the next install cleans
  stale staging files automatically.
- pnpm 11's `minimumReleaseAge` (default 24 h) may delay a fresh release's installability;
  wait the window out if `dsh plugin add` cannot resolve the version yet.

## Compatibility matrix

| Plugin | ocrx (bundled CLI) | `engines.dsh` |
| --- | --- | --- |
| 0.1.0 | 0.1.0 (workspace version; >= 0.2.0 recommended for exact install sizes) | `>=0.2.2-rc.5 <0.2.3-0` |

`engines.dsh` is frozen to the CI-verified window; widening it ships with a plugin
release after that dsh version enters the CI matrix. Older ocrx binaries (pre-0.2.0)
work with degraded faces: install sizes show as "unconfirmed (~1.0 GB)" and `version`
faces report unknown.

## Platform support

`win32` x64 and `darwin` arm64 ship bundled binaries (CPU builds; CUDA stays a local
build + `command` config route). Other platforms are rejected at install time by dsh
(`UnsupportedBundlePlatformError`) — this is the chosen platform policy.

## Licenses

The plugin is repository-licensed (Apache-2.0, see the repo root). The bundled platform
packages carry the full license set (llama.cpp, PDFium, MS VC runtime) inside the
package.
