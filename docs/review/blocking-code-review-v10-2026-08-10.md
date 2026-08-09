# OCR-CLI 阻断性代码评审（v10，2026-08-10）

## 结论

**Request changes。** v9 报告中的 resume 截断状态丢失和 adapter warnings 丢失已经修复，格式、测试和严格 Clippy 质量门全部通过。但渲染审计仍未进入 checkpoint/result，缩放 warning 也没有被汇总；同时渲染/识别失败原因被直接丢弃，最终契约无法定位失败页面及原因。这两项违反已经冻结的可审计输出契约，仍属于发布阻断项。

## 阻断问题

### Critical：渲染尺寸、缩放状态没有进入 checkpoint/result，`render_scaled` warning 永远不会由扫描路径产生

位置：`src/ocrx-output/src/lib.rs:499-521`、`src/ocrx-output/src/lib.rs:894-915`、`src/ocrx-cli/src/main.rs:1322-1378`

`RenderedPage` 已携带完整 `RenderAudit`，但扫描成功时只把它写入独立的 `page-NNNN.render-audit` 文件。`PageCheckpoint` v3 的序列化字段仅有兼容键、完成状态、摘要、截断和 warnings；最终 `ScanResultFields` 也只有聚合页数，没有任何页级原始尺寸、300 DPI 渲染尺寸、送模尺寸或缩放状态。因此冻结计划 P4.2 的“缩放 warning、页面尺寸与页段审计进入 checkpoint/result”仍未实现，Agent 仅解析稳定 JSON/checkpoint 时无法取得这些诊断。

此外，`image.audit.scaled` 在扫描路径中没有被转换为 `render_scaled` warning。当前汇总只接收 adapter 返回的 warnings，而 adapter 并不知道 renderer 是否缩放；所以即使真实页面发生缩放，`result.json.warnings` 仍不会报告它。旁路 `.render-audit` 文件不能替代约定的 result/checkpoint 字段，也不会在 resume 时恢复到汇总结果。

建议修复：将结构化 `RenderAudit` 纳入页面 checkpoint schema，并在最终结果中输出页级诊断；当 `scaled=true` 时在页面 warnings 中加入 `render_scaled`，首次扫描和 resume 均从同一 checkpoint 状态汇总。增加“超像素预算页面 → checkpoint/result 包含三组尺寸、比例/缩放状态及 warning”和“resume 后诊断完全一致”的回归测试。

### Critical：分页渲染和识别错误被吞掉，partial 结果只有计数、没有失败页面或原因

位置：`src/ocrx-output/src/lib.rs:499-525`、`src/ocrx-output/src/lib.rs:545-551`、`src/ocrx-cli/src/main.rs:1322-1378`

renderer 与 recognizer 的错误分支都使用 `Err(_)`，仅增加 `failed` 计数后继续。错误类型、消息、失败阶段和页码没有写入 checkpoint、`JobSummary` 或 `result.json`。最终 JSON 只能报告 `pages.failed=N`；多页任务中无法判断具体哪页失败，更无法区分 PDF 渲染、worker、模型推理或产物提交问题。

这与 P4.2 “页段失败由主进程映射为可审计页面失败”和稳定 Agent 契约冲突。尤其在 partial 结果中 `markdown_path=null`，调用方没有其他结构化入口恢复或提示用户处理失败页。

建议修复：定义结构化页状态，例如 `{page, status, stage, error_code, message, render_audit, warnings}`；保留安全、稳定的错误码并避免泄露敏感路径/模型输出。把每页状态原子持久化并纳入最终 result；resume 应保留已完成页诊断、重试不完整/失败页。增加 renderer failure、recognizer failure 和混合成功/失败页的 JSON 解析断言。

## 已验证的修复

- checkpoint 已升级到 v3，保存 `truncated` 和 warnings；复用页面时会恢复并汇总，旧版 checkpoint 不会被静默当作完整结果。
- adapter 的 `truncated`、`repetition_removed` 等 warnings 已通过显式 `PageArtifactResult` 传递到扫描汇总与最终 JSON。
- 安装锁不再按文件年龄强制抢占；Unix/Windows 都采用保守的进程存活判断，写锁失败也不再遗留空锁文件。
- stdout JSON 与落盘 `result.json` 继续复用同一序列化结果。

## 质量门

- `cargo test --workspace --offline`：通过；104 passed，1 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。

## 评审维度摘要

- 正确性/契约：阻断；页级渲染诊断和失败详情未进入稳定结果。
- 可靠性：阻断；失败原因丢失使 partial 任务不可定位、不可审计。
- 安全性：未发现新的阻断性问题；后续持久化错误消息时应使用稳定错误码并控制敏感信息。
- 性能：未发现新的阻断性问题；SHA-256 保持固定块增量计算。
- 可维护性：页面状态仍分散在旁路 audit、checkpoint 和聚合 summary 中，建议统一为强类型页面结果。
- 测试：质量门为绿，但缺少渲染诊断进入 checkpoint/result、缩放 warning 与失败页结构化输出的契约测试。
