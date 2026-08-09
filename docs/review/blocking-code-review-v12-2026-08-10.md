# OCR-CLI 阻断性代码评审（v12，2026-08-10）

## 结论

**Request changes。** v11 的页级 warnings 序列化缺陷已经修复，并新增了 warning、audit 和特殊字符回归测试；完整质量门全部通过。但渲染审计仍不满足冻结需求：所有路径都没有记录缩放比例，PDF worker 路径还把渲染后位图尺寸冒充原始页面尺寸。最终 `result.json`/checkpoint 虽然出现了 audit 字段，但其中数据不完整且在 worker 模式下不真实，仍属于发布阻断项。

## 阻断问题

### Critical：渲染审计缺少缩放比例，worker 模式还会报告错误的原始页面尺寸

位置：`crates/ocrx-output/src/lib.rs:364-387`、`crates/ocrx-cli/src/main.rs:696-724`、`crates/ocrx-cli/src/main.rs:1366-1374`

需求 `docs/requirement/requirements-v0.1.md:304-312` 明确要求 `result.json` 和页面 checkpoint 同时记录原始页面尺寸、300 DPI 渲染尺寸、最终送模尺寸和**缩放比例**。

当前 `RenderAudit` 只有三组尺寸和 `scaled: bool`，checkpoint fragment 与最终 JSON 都没有 ratio。调用方只能自行猜测应使用宽度比、高度比还是像素面积比，无法读取冻结契约要求的明确比例；四舍五入后两个轴的比例也可能略有差异，不能用布尔值替代。

更严重的是 `WorkerScanRenderer`：worker 只返回 PNG，主进程读取 PNG 后将 `rendered_width/rendered_height` 同时赋给 `original_width/original_height`。因此只要设置 `OCRX_PDF_WORKER`，`diagnostics[*].audit.original` 就会错误地等于 300 DPI 位图尺寸，而不是 PDF 原始页面尺寸。非 worker 的 `PdfScanRenderer` 会调用 `render_page_rgb8_with_audit` 获得真实原始尺寸，两条执行路径由此产生语义不一致的同名字段。

这会直接误导质量诊断：Agent 无法判断原始页面、DPI 放大和模型缩放分别发生了什么，也无法跨 worker/非 worker 模式比较结果。

建议修复：

- 在 worker IPC/产物协议中携带每页结构化 render metadata，不要从最终 PNG 反推或伪造原始尺寸；主进程必须校验页码、非零尺寸及 PNG 实际尺寸与 metadata 一致。
- 为 `RenderAudit` 增加语义明确、可稳定序列化的缩放比例。可使用有理数 `{numerator, denominator}` 或明确精度的数值，并说明比例是最终送模尺寸相对 300 DPI 渲染尺寸；避免浮点进入 `Eq`/checkpoint 兼容判断时产生不稳定比较。
- checkpoint 与 `result.json` 使用同一结构化 audit 模型，resume 后不得改变任何审计字段。
- 增加 worker 与进程内 renderer 的等价契约测试，以及超预算页面首次扫描/恢复后的 JSON parser 逐字段断言。

## 已验证的修复

- `diagnostics[*].warnings` 已从 `PageDiagnostic.warnings` 实际序列化，不再固定为空。
- diagnostic 的 status、stage、error_code、message 和 warnings 已统一执行 JSON escaping。
- 新增 CLI 测试覆盖页级 warnings、scaled audit 以及引号、反斜杠和换行。
- checkpoint v4 继续恢复 audit 和 warnings；缩放页会汇总 `render_scaled`。

## 质量门

- `cargo test --workspace --offline`：通过；105 passed，1 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。

## 五维评审摘要

- 正确性：阻断；worker 模式的 original dimensions 数据错误，且缺少强制要求的 scale ratio。
- 可读性：页级 JSON 手工拼接仍较脆弱，但本轮新增字段已正确转义。
- 架构：worker IPC 没有承载完整 render metadata，导致主进程被迫构造语义不真实的 audit。
- 安全性：未发现新的阻断性安全问题。
- 性能：未发现新的阻断性性能问题。
