# PDF Content-Aware v0.2 本机执行汇总

## 状态

本报告汇总本机可执行证据，但不宣称 v0.2 已发布。当前阻断项为：

- 30 页 native candidate 仍等待人工 L2 验收；
- 真实 OvisOCR2 强制 OCR 的默认 Metal 构建在当前主机不可用；已通过 CPU-only `GGML_METAL=OFF` 构建和 CPU 参数修复完成 30 页 OCR，对照性能重复测量仍待补齐；
- 尚未构造出真实 native 校验失败、随后进入 OCR 的 CLI 路径；
- `--device auto` 在当前主机选择 Metal 后报稳定 `device_unavailable`；成功 CPU 回退属于需要可用 Metal 对照环境的另一分支。

## 已完成证据

| 范畴 | 证据 | 结果 |
| --- | --- | --- |
| 30 页 native candidate | `manifest.json`、30 份 truth、30 份 preview | 结构完整，等待人工验收 |
| PDFium native evidence | Stage 6 gate | 30/30 native_text |
| native 质量 | `stage6-gate-2026-09-05.json` | text NED 0，reading order/block/bbox 全部通过 |
| forbidden image set | 同一 gate | 20/20 image，0 native 误报 |
| native 性能 | `stage6-auto-native-performance-rss-2026-09-05.json` | 5 次完成，模型未加载 |
| quality OCR 性能 | `stage6-quality-ocr-performance-rss-2026-09-05.json` | 5 次完成，CPU-only，中位 546 秒，峰值 RSS 中位 2558032 KB |
| mixed/uncertain E2E | `assets/regression/pdf-mixed-e2e-v0.1/review/cli-auto-e2e-report.json` | native 1 页、OCR 2 页，页序合并正确 |
| workspace gate | `cargo fmt`、`cargo test --workspace`、`cargo clippy ... -D warnings` | 全部通过 |
| backend error contract | 当前 CLI 真实单页/30 页运行 | `device_unavailable` 稳定输出 |

## 强制 OCR 真实结果

命令契约为 `--pdf-mode ocr --model-profile quality`，30 页全部进入 OCR，模型加载字段为 true。CPU-only 构建（`GGML_METAL=OFF`）并修复 `main_gpu=-1` 后，30/30 页面成功完成，耗时 505308 ms，退出码 0。有效评分为 text NED=0.081038、reading order=1.0、blank=0、duplicate=0。

详见 `pdf-mode-ocr-compatibility-2026-09-05.json`。

此前的 `/tmp/ocrx-native-candidate-quality-ocr/result.json` 不纳入有效强制 OCR 基线，因为其中记录了 30 页 `native_text`，与其目录名不符。

## Stage 6 门禁结论

Stage 6 gate 已执行且结构、native 质量、forbidden 路由和性能证据通过；门禁最终状态仍为 `release_ready=false`，唯一已登记的发布阻断为 `human_native_acceptance_incomplete`。本报告不把 draft candidate 改标为正式合格集。
