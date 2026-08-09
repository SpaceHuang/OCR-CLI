# OCR-CLI 阻断性代码评审（v14，2026-08-10）

## 结论

**Request changes。** v13 的 worker audit 提交和线性缩放比例已经修复，完整质量门全部通过。但 worker 执行协议仍未传递用户的 `--dpi`：进程内路径按用户参数渲染，worker 路径始终固定为 300 DPI。更严重的是 checkpoint compatibility 记录的是用户指定值，而真实 worker 输出并未使用它，可能让错误参数生成的页面被标记成对应参数的可复用结果。该参数静默失效与断点真实性问题属于发布阻断。

## 阻断问题

### Critical：worker 模式静默忽略 `--dpi`，checkpoint 兼容键与真实渲染参数不一致

位置：`src/ocrx-cli/src/main.rs:1154-1175`、`src/ocrx-pdf/src/lib.rs:251-284`、`src/ocrx-pdf/src/bin/pdfium-worker.rs:23-29`；兼容键：`src/ocrx-cli/src/main.rs:1208-1218`

CLI 创建进程内 `PdfScanRenderer` 时会构造：

```text
RenderOptions { dpi: options.dpi.unwrap_or(300) }
```

但创建 `WorkerScanRenderer` 时没有保存 DPI，`PdfWorkerScheduler` 调用 worker 的参数也只有 library、PDF、页段和输出目录。worker 最终固定执行 `RenderOptions::default()`，即 300 DPI。

因此设置 `OCRX_PDF_WORKER` 后，合法的 `--dpi 150`、`--dpi 600` 等参数都会被接受并写入任务 compatibility，却对真实渲染完全无效。用户无法通过结果或退出码察觉这一点，audit 只记录尺寸、不记录实际 DPI。

该问题还破坏 resume 语义：compatibility key 使用 `options.dpi`，所以 300 DPI 实际产物可以被标记成 `dpi=150` 的 checkpoint；后续 `--resume --dpi 150` 会复用这份并非按 150 DPI 生成的 OCR 结果。相反，省略 DPI 与显式 `--dpi 300` 还会形成不同 compatibility 字符串，尽管实际行为相同。

建议修复：

- 将解析并归一化后的有效 DPI（默认值也展开成 300）作为强类型 `RenderOptions` 贯穿 CLI、WorkerScanRenderer、PdfWorkerScheduler 和 worker argv/IPC。
- worker 明确解析并校验 DPI，再用该值构造 `RenderOptions`；不要依赖 `default()` 隐式决定协议行为。
- compatibility key 使用有效参数值，例如始终记录 `dpi=300` 而不是 `dpi=None`/`dpi=Some(300)`，保证相同行为得到相同键、不同实际行为必然得到不同键。
- audit/checkpoint 可额外记录实际 DPI，便于验证尺寸含义。
- 增加 worker fake/argv 测试与渲染契约测试，覆盖默认 300、显式 150/300/600，以及首次扫描后 resume 的 compatibility 一致性。

### Required：新增比例测试仍使用与尺寸不一致的手工数据，未验证真实计算

位置：`src/ocrx-cli/src/main.rs:1832-1872`

测试中的 rendered/model 宽度为 `80 → 64`，真实线性比例应为 `4/5`，但 fixture 手工填入 `1/4`，且测试只断言 `scaled=true`，没有断言序列化后的 ratio。它能验证字段存在，却不能防止线性比例再次退化为面积比或任意错误数值。

建议修复：直接测试 `linear_scale_ratio(64, 80) == (4, 5)`，并在 JSON 回读中断言 numerator/denominator；再覆盖不缩放 `1/1` 和取整后的非整比例。

## 已验证的修复

- scheduler 已将每页 PNG 与 audit 一并从 segment 目录提交，真实 worker 输出不再因 audit 丢失而必然失败。
- worker segment 验证会要求 audit 存在，并校验 rendered dimensions 与 PNG 尺寸一致。
- 两条 renderer 路径都使用约分后的 `model_width/rendered_width` 线性比例，不再使用像素面积比。
- fake worker success 测试已生成双文件并覆盖 scheduler 提交流程。

## 质量门

- `cargo test --workspace --offline`：通过；105 passed，1 ignored。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。

## 五维评审摘要

- 正确性：阻断；worker 模式实际 DPI 与用户参数及 compatibility key 不一致。
- 可读性：渲染有效参数应在解析边界归一化，避免 `Option`/默认值在多条路径重复解释。
- 架构：worker IPC 缺少 RenderOptions，导致进程内与隔离执行路径行为漂移。
- 安全性：未发现新的独立安全阻断项。
- 性能：不同 DPI 会显著改变位图内存和耗时，静默固定 300 也破坏用户的资源控制预期。
