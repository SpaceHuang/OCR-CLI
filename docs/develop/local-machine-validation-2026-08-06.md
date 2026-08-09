# 本机可完成验收记录（2026-08-06）

主机：macOS arm64 / Apple M2 / 16 GB / Metal  
范围：计划内可在本机完成的验证（不含 30 页真值质量门禁与四 profile BF16 对照）。

## 通过项

| 项 | 结果 | 证据 |
|---|---|---|
| `cargo test --workspace` / `cargo fmt` | PASS | 本会话最终重跑 |
| compact 模型安装到 `.ocrx-model-cache` | PASS | `ocrx model verify compact` → installed |
| PDFium binary / fixture / encrypted | PASS | `macos-pdfium-binary-check`、`macos-pdf-fixture-check`、`macos-encrypted-pdf-check` |
| 真实教材 PDF 抽样渲染 | PASS | `macos-real-pdf-check=PASS` |
| PDF worker 分段 | PASS | `macos-pdf-worker-check=PASS`（环境缺 `pdfinfo`，页数抽样仍通过） |
| 100 页 RSS 采样 | PASS（直接采样） | `samples=131 min_kb=1856 max_kb=47696`，`pdfium-check` exit 0 |
| Metal runtime smoke | PASS | `mtmd-generate-probe` exit 0，生成 OCR 文本 |
| 门控 E2E `OCRX_E2E_MODEL=1` | PASS | `gated_real_model_writes_at_least_one_figure_png` |
| 1 页真实 scan + JSON | PASS | `/tmp/ocrx-local-scan-out`，`status=success`，写出 `document.md` |
| `--offline` 有模 / 无模 | PASS | 有模 exit 0；无模 exit 4 |
| 输出冲突 / `--resume` / `--overwrite` | PASS | 冲突 exit 9；resume/overwrite exit 0 |
| Skill install | PASS | 临时 HOME 下 Codex Skill 落盘 |
| macOS 打包脚本 | PASS | `/tmp/ocrx-dist/ocrx-0.1.0-macos-arm64.tar.gz` + `SHA256SUMS` |
| 兼容矩阵脚本 | PASS | `validate-compatibility.py` |
| 发布脚本/Skill 校验 | PASS | `test-release-scripts.sh` |

## 本机修复（为跑通验收）

1. PDFium 进程内只绑定一次（`shared_pdfium`），避免 `PdfiumLibraryBindingsAlreadyInitialized`。
2. adapter `write_page_artifacts` 对超限 token 做 clamp，避免默认 16384 直接失败。
3. overwrite 保护根改为 input/cache/cwd，避免把整个 `/Users`/`/tmp` 当成禁写区。

## 仍无法在本机完成（缺数据）

- ≥30 页人工真值回归集与质量评分门禁
- balanced / quality / reference / BF16 全 profile 对照
- P6 干净环境全量证据包与正式 go/no-go

## 关键路径

- 模型缓存：`/Users/space/Documents/Develop/OCR-CLI/.ocrx-model-cache/ovisocr2/compact/`
- 社区资产：`/tmp/ocrx-ovisocr2-community/{OvisOCR2-Q4_K_M.gguf,mmproj-F16.gguf}`
- PDFium：`/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib/libpdfium.dylib`
- 打包产物：`/tmp/ocrx-dist/ocrx-0.1.0-macos-arm64.tar.gz`
