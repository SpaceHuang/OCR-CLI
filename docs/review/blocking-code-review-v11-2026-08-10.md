# OCR-CLI 阻断性代码评审（v11，2026-08-10）

## 结论

**Request changes。** v10 的渲染审计、缩放告警以及失败页诊断主链路已经实现，checkpoint 已升级为 v4，自动化质量门全部通过。但最终 `result.json` 序列化会无条件丢弃每页 warnings，使新增的页级可审计契约与内存/checkpoint 状态不一致；同时没有任何测试覆盖新增 diagnostics。该问题会让 Agent 无法把质量告警定位到具体页面，仍属于合并/发布阻断项。

## 阻断问题

### Critical：`result.json.diagnostics[*].warnings` 被硬编码为空，页级告警在最终契约中再次丢失

位置：`src/ocrx-cli/src/main.rs:1362-1368`；上游状态来源：`src/ocrx-output/src/lib.rs:500-554`、`src/ocrx-output/src/lib.rs:905-919`

扫描层目前会正确完成以下工作：

- adapter warnings 写入 `PageCheckpoint.warnings`；
- `image.audit.scaled=true` 时追加 `render_scaled`；
- resume 时从 checkpoint 恢复 warnings；
- `PageCheckpoint::diagnostic()` 把这些值复制到 `PageDiagnostic.warnings`。

但是 `serialize_scan_result` 在构造每条 diagnostic 时固定输出 `"warnings":[]`，完全没有读取 `diagnostic.warnings`。因此首次扫描和 resume 的最终 JSON 都会丢失页级 `truncated`、`repetition_removed`、`render_scaled` 等信息。全局 `warnings` 虽然仍存在，但只能表示任务中曾发生某类问题，无法说明发生在哪一页；这破坏了 v10 要求的结构化页状态，也使 result 与 checkpoint 对同一页给出不同诊断。

建议修复：使用统一 JSON string escaping helper 序列化 `diagnostic.warnings`，不要手写固定数组；最好将 `ScanResult`、`PageDiagnostic` 定义为强类型可序列化模型，避免继续用 `format!` 维护嵌套 schema。增加 JSON parser 回读测试，至少覆盖：

1. 缩放页面的 diagnostic 同时包含 audit 与 `render_scaled`；
2. 截断/重复清理 warnings 保留在对应页；
3. resume 后 diagnostics 与首次扫描一致；
4. warning/message 含引号、反斜杠或控制字符时仍生成合法 JSON。

### Required：新增 diagnostics/checkpoint v4 行为没有回归测试，当前测试无法发现上述数据丢失

位置：`src/ocrx-cli/src/main.rs:1747-1773`、`src/ocrx-output/src/lib.rs:1600-1785`

CLI 的 `scan_result_json_preserves_truncation_and_escapes_paths` 仍传入空 diagnostics，只断言全局 warnings；output 的 checkpoint round-trip 使用 `audit: None`，也没有断言 scaled audit、页级 warnings 和 resume diagnostic 的联合恢复。测试数量与上一轮相同，新增实现没有对应的行为测试，所以 `"warnings":[]` 这类直接契约错误仍能让全部质量门通过。

建议修复：为新增行为补充端到端数据流测试，而不只是结构体或字符串片段测试。测试应从 fake renderer/recognizer 产生 scaled、truncated 和失败页，运行 ScanService，序列化最终结果，再用 JSON parser 逐字段断言首次运行及 resume 输出。

## 已验证的修复

- `RenderAudit` 已写入 checkpoint v4，并在 resume 时恢复到 `PageDiagnostic`。
- `scaled=true` 会生成全局 `render_scaled` warning。
- renderer/recognizer 失败不再只保留计数；最终 summary 已包含页码、阶段、稳定错误码和安全消息。
- v9 的 truncated/warnings checkpoint 恢复语义继续保留。

## 质量门

- `cargo test --workspace --offline`：通过；104 passed，1 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。

## 五维评审摘要

- 正确性：阻断；最终 JSON 与内存/checkpoint 的页级 warnings 不一致。
- 可读性：嵌套 JSON 继续由单行 `format!` 手工拼接，已导致字段遗漏。
- 架构：页面诊断模型已经形成，但 serializer 没有完整消费该类型；建议统一强类型序列化边界。
- 安全性：未发现新的独立安全阻断项；新增字符串字段仍应统一转义。
- 性能：未发现新的阻断性问题。
