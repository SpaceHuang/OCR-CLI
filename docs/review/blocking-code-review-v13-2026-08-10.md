# OCR-CLI 阻断性代码评审（v13，2026-08-10）

## 结论

**Request changes。** v12 的 worker 原始尺寸 metadata 与缩放比例字段已经开始实现，完整质量门全部通过。但 worker 调度器没有提交新生成的 audit 文件，随后还会删除其所在临时目录，导致启用 `OCRX_PDF_WORKER` 时每一页必然渲染失败；此外 `scale_ratio` 当前计算的是像素面积比，不是等比缩放的线性比例。worker 主路径仍不可用，属于发布阻断。

## 阻断问题

### Critical：worker audit 文件未从 segment 临时目录提交，启用 worker 后每页必然失败

位置：`src/ocrx-pdf/src/bin/pdfium-worker.rs:29-45`、`src/ocrx-pdf/src/lib.rs:285-298`、`src/ocrx-cli/src/main.rs:696-715`

`pdfium-worker` 现在为每页生成两份文件：

- `page-NNNN.png`
- `page-NNNN.audit`

但 `PdfWorkerScheduler::run_with_cancel` 验证 segment 后只将 PNG 从 `.worker-START-END` 重命名到正式 worker output root。循环结束后立即 `remove_dir_all(segment_root)`，audit 文件随临时目录被删除。

随后 `WorkerScanRenderer::render` 从正式 root 成功打开 PNG，却会读取同目录的 `page-NNNN.audit`。该文件从未被移动，因此 `read_to_string` 必然返回 NotFound；ScanService 将页面记为 `render_failed`。只要用户设置 `OCRX_PDF_WORKER`，正常 worker 生成的每个页面都会失败，最终只能得到 partial/退出码 10。

现有 scheduler success 测试只让 fake worker 生成 PNG，也只断言 PNG 被提交，所以无法发现真实 worker 与 scheduler 协议已经漂移。

建议修复：

- 把 PNG 与 audit 定义为同一个页面提交单元；`validate_worker_segment` 必须要求每个页号两者都存在、audit 可解析、尺寸非零且 rendered dimensions 与 PNG 一致。
- scheduler 应在删除 segment 前提交两份文件；任一重命名失败时不得留下看似完整的半页结果。优先把每页文件放入页级目录后原子 rename，或使用 manifest/完成标记完成两阶段提交。
- 更新 fake worker success/missing/invalid/extra 测试，覆盖 audit 缺失、损坏、页码不匹配和提交后可读取。
- 增加 `WorkerScanRenderer` 级测试，确保真实 scheduler 输出可以被消费，而不是分别测试生产端和消费端。

### Critical：`scale_ratio` 使用面积比，数值会把线性缩放比例平方

位置：`src/ocrx-cli/src/main.rs:716-742`、`src/ocrx-cli/src/main.rs:773-798`、`src/ocrx-output/src/lib.rs:364-391`

当前 numerator 使用 `model_width * model_height`，denominator 使用 `rendered_width * rendered_height`。这表示保留的像素面积比例，而需求中的“保持宽高比缩小”和“缩放比例”是线性尺度。例如宽高都缩小到 50% 时，当前 JSON 会输出 `1/4`，调用方会误以为页面线性缩小到了 25%。字段名 `scale_ratio` 也没有表明它是 area ratio。

建议修复：将比例定义为线性尺度，例如约分后的 `model_width/rendered_width`；由于像素取整，另一轴可能略有差异，可明确以宽度为基准，或分别记录 `scale_x`/`scale_y` 并验证误差在一个像素范围内。如果确实需要面积比例，应另命名为 `pixel_area_ratio`，不能用它满足线性 scale ratio 契约。为 1/2 尺寸缩放增加断言，确保输出为 1/2 而非 1/4。

## 已验证的修复

- worker 已能从 PDFium 获取真实的原始页面尺寸，并写出包含 original/rendered dimensions 的 audit sidecar。
- 主进程会解析 worker audit，并在能够读取时校验 rendered dimensions 与 PNG 实际尺寸一致。
- `RenderAudit`、checkpoint 和最终 JSON 已增加有理数比例字段。
- v11 的页级 warnings、audit 与字符串转义修复继续有效。

## 质量门

- `cargo test --workspace --offline`：通过；105 passed，1 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。

## 五维评审摘要

- 正确性：阻断；worker 生产/提交/消费协议不一致，主路径必然失败；scale ratio 语义错误。
- 可读性：worker 产物协议散落在 binary、scheduler 和 CLI 三处，缺少共享的强类型 manifest。
- 架构：建议由 `ocrx-pdf` 统一拥有 worker page artifact 类型及验证/提交逻辑，CLI 只消费验证后的结构。
- 安全性：worker 外部产物已做部分边界校验，但 audit 尚未进入 scheduler 的完整性校验。
- 性能：未发现新的独立性能阻断项。
