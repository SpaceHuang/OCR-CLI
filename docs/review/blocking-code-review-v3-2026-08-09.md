# OCR-CLI 阻断性代码复评 v3（2026-08-09）

## 结论

**Request changes。** v2 中模型加载前验证、交互确认、token 联合预算和 `result.json` 写入均已开始落地，顺序执行的格式、测试与严格 Clippy 均可通过。但结果 JSON、断点产物摘要、Windows 哈希能力和模型回滚仍存在阻断性问题。

本报告为第三轮评审，只记录当前阻断项，不覆盖 v1/v2 报告。

## 阻断项

### Critical：生成的 `result.json` 对合法路径可能不是 JSON，partial 结果还会指向不存在的 Markdown

位置：`crates/ocrx-cli/src/main.rs:1163-1216`

`result.json` 通过 `format!` 直接拼接 `markdown_path`、`output_dir`、profile 和 backend，没有进行 JSON escaping。合法 Unix 路径可包含双引号、反斜杠和换行；遇到这些字符会生成无法解析或语义错误的 JSON。代码库已有 `ResultReport::to_json` 的 escaping 能力，但新文件没有复用它或结构化序列化模型。

另外，`ScanService` 仅在 `failed == 0` 时合并 `document.md`，而 CLI 在 partial 状态仍无条件写入 `result.json`，并把 `markdown_path` 指向 `output/document.md`。因此任何页面失败都会产生一个稳定 JSON 文件，但其中最关键的路径不存在，Agent 无法按契约读取结果。

当前 schema 也仍缺少需求最低字段中的 input、结构化 model revision/adapter/runtime、truncated、images、elapsed_ms、页级诊断及渲染尺寸；现有测试只覆盖旧的精简 `ResultReport`，没有覆盖实际落盘的 schema。

建议修复：定义单一的版本化 `ScanResult` 结构并使用可靠 JSON serializer；stdout JSON 与落盘 JSON 共用该结构。明确 partial 行为：要么合并所有成功页并让路径真实存在，要么将 `markdown_path` 设为 null/省略并提供页产物。为特殊字符路径、partial、success 和 schema golden 增加落盘测试。

### Critical：断点图片摘要解析不到程序自己生成的标准 Markdown 链接

位置：`crates/ocrx-output/src/lib.rs:834-885`; 生成格式见 `crates/ocrx-ovisocr2/src/lib.rs:571-590`

`artifact_sha256` 用 `split_whitespace()` 后仅对整个 token 执行 `strip_prefix("images/")`。程序生成的链接形如 `![figure](images/page-0002-figure-001.png)`；整个 token 以 `![figure](` 开头，因此不会匹配。HTML 的 `src="images/..."` 同样不会匹配。结果是摘要仍只覆盖 Markdown，自身生成的 bbox PNG 被删除或篡改后，`--resume` 仍会复用 checkpoint，并输出断链或错误图片。

这是 v2 图片完整性问题的表面修复：新增了产物清单概念，但解析逻辑与实际输出语法不一致，且没有对应回归测试。

建议修复：不要再次实现一套基于空白的 Markdown 解析。让 `write_page_artifacts`/`commit_page` 显式返回或接收该页产物清单，checkpoint 直接保存排序后的相对路径与摘要；resume 对清单逐项验证。至少增加“提交含标准 Markdown 图片链接 → 删除/篡改 PNG → 不可复用”的测试。

### Critical：首发 Windows 路径依赖系统 `shasum`，模型安装和校验无法工作

位置：`crates/ocrx-models/src/lib.rs:253-269`; `crates/ocrx-cli/src/main.rs:1251-1263`; `crates/ocrx-output/src/lib.rs:834-885`

模型哈希、输入 PDF 哈希和断点产物哈希全部通过启动外部 `shasum -a 256` 完成。`shasum` 是 macOS/Unix 常见工具，不是 Windows 10/11 的标准命令；而 Windows x86_64 CPU 是 v0.1 明确首发平台。结果是 Windows 上模型安装/verify 会失败，扫描输入哈希被 `unwrap_or_default()` 静默降为空字符串，页面提交也会因找不到 `shasum` 而失败。

即使在 macOS，输入哈希错误被降为空字符串也会隐藏真实 I/O/工具错误并削弱 resume compatibility。

建议修复：使用 Rust 内部 SHA-256 实现并流式读取文件，形成 workspace 内唯一的 canonical hash helper；模型、CLI 和输出 crate 复用它。计算输入哈希失败必须作为输入错误退出，不能写空摘要。增加不依赖 PATH 的哈希 golden test，并在 Windows CI 运行相关测试。

### Required：模型资产回滚仍是 best-effort，失败时可能留下缺失或混合缓存

位置：`crates/ocrx-models/src/lib.rs:280-316`

`commit_pair` 已加入备份与回滚，这是明显进步；但所有恢复 rename 的结果都被忽略。第二个新文件提交失败后，代码先删除最终 model/mmproj，再尝试恢复备份；任一恢复失败，函数仍只返回通用 `TransactionFailed`，缓存可能缺文件。成功提交后删除备份也忽略失败，会积累接近 GB 级的孤儿备份。当前测试没有故障注入覆盖第二次提交和恢复失败。

此外，该方案仍不是资产对的单次原子可见切换；并发 scan 可能在两个 rename 之间观察到缺失/半提交状态。

建议修复：优先采用 revision/profile 目录级 staging 与原子目录切换，使读者只解析已提交目录；若保留文件级事务，则必须加锁、逐步记录状态、检查每次恢复结果并保留可恢复备份。增加第二次提交失败、恢复失败及并发读取测试。

### Required：PDF worker 成功测试使用 2 秒真实墙钟超时，质量门并发时发生失败

位置：`crates/ocrx-pdf/src/lib.rs:675-694`

本轮首次并发执行 `cargo test`、Clippy 和 fmt 时，`worker_scheduler_commits_only_valid_segments_and_reports_resources` 以 `worker timed out` 失败；隔离重跑及随后顺序重跑 workspace 均通过。测试启动 shell、base64 和子进程，却只给 2 秒墙钟预算，因此容易受构建锁和机器负载影响。

测试不稳定会阻断合并质量门，即使产品代码本身没有超时回归。

建议修复：成功路径使用足够宽松的测试超时，超时行为使用可控时钟/轮询注入或单独的确定性测试；避免以紧张的真实墙钟同时证明成功和超时语义。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：首次与其他质量门并发执行时失败，PDF worker 成功测试超时；随后顺序重跑通过，102 passed、0 failed、1 ignored。
- 真实 OvisOCR2/PDFium 端到端测试仍因环境 gate 被忽略。

## 五维摘要

- 正确性：落盘结果契约、resume 图片完整性和 Windows 核心流程仍被阻断。
- 安全性：模型验证已显著改善；哈希能力仍依赖外部工具和静默 fallback。
- 架构：应复用单一结果模型、显式页产物清单和 canonical hash helper。
- 性能：孤儿模型备份可能产生 GB 级磁盘浪费；未发现其他独立性能阻断项。
- 可读性：本轮修复方向总体正确，但字符串拼接 JSON 和基于空白解析 Markdown 都是脆弱的重复实现。
