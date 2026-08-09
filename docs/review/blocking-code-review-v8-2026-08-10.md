# OCR-CLI 阻断性代码复评 v8（2026-08-10）

## 结论

**Request changes。** v7 的 stdout/落盘结果统一和 Windows stale-lock 恢复路径已基本接通，全部自动化质量门通过。但扫描流水线仍丢弃 adapter 的截断与 warning 信息，导致结果把不完整 OCR 错报为 success；图片计数在 `--keep-pages` 下也会错误包含整页调试图。

本报告为第八轮评审，只记录当前阻断项，不覆盖此前报告。

## 阻断项

### Critical：adapter 的截断状态被丢弃，`result.json` 会把不完整 OCR 报告为 success

位置：`crates/ocrx-ovisocr2/src/lib.rs:16-57`; `crates/ocrx-cli/src/main.rs:797-817`; `crates/ocrx-output/src/lib.rs:491-524`; `crates/ocrx-cli/src/main.rs:1190-1268`

`adapt_response` 能根据 runtime 的 `finish_reason=max_tokens` 产生 `Termination::MaxTokens` 和 `PageWarning::Truncated`，但 `write_page_artifacts` 最终只返回 `String` Markdown。`OvisRecognizer::recognize_artifact` 和 `PageArtifactRecognizer` 也只返回 `(PNG, Markdown)`，没有传递页面 termination/warnings。

因此 checkpoint 扫描路径中：

- `JobSummary.truncated_pages` 固定为 0；
- schema v3 的 `pages.truncated` 固定为 0；
- `warnings` 固定为空；
- 只要没有 I/O 错误，任务状态就是 `success`，即使一页或多页因 token 上限被截断。

这会让 Agent 把内容不完整的文档当作完整成功结果，违反需求对 truncated、warnings 和 partial 状态的稳定契约。

建议修复：让 adapter 返回明确的 `PageArtifactResult { markdown, artifacts, termination, warnings }`，通过 recognizer/ScanService 累计页面状态；checkpoint 保存 termination/warnings，resume 时恢复这些状态；`JobSummary.status` 和最终 `ScanResult` 据此生成 partial/truncated/warnings。增加 max_tokens 页面、混合正常/截断页面和 resume 截断页的端到端单元测试。

### Required：`--keep-pages` 会把整页 PNG 计入插图数量，结果字段语义错误

位置：`crates/ocrx-cli/src/main.rs:1253-1266`; 全页图写入见 `crates/ocrx-output/src/lib.rs:1039-1048`

image count 通过统计 `images/` 下所有以 `page-` 开头的 PNG 得出。bbox 插图名为 `page-0001-figure-001.png`，但 `--keep-pages` 保留的整页图名也为 `page-0001.png`；因此启用该参数后，每个成功页至少额外增加 1 张“图片”，即使 PDF 没有任何插图。

需求中的 images 是 OCR 提取插图数量，不应随调试/保留整页开关变化。Agent 会据此错误报告图片数量。

建议修复：图片数量来自 adapter 明确返回的 figure artifact 清单，或至少只匹配严格的 `page-NNNN-figure-NNN.png` 命名；不要扫描整个输出目录推断业务结果。增加同一输入在 keep_pages 开/关时 figure count 相等的测试。

### Required：成功热路径保留完整 `_legacy_result_json` 死实现

位置：`crates/ocrx-cli/src/main.rs:1202-1243`

代码先构造一整份 `_legacy_result_json`，执行所有路径扫描和字符串分配，然后完全丢弃，再调用 `serialize_scan_result` 构造第二份相同结果。这不是兼容层，而是明确的无用计算和重复 schema；变量名前缀只是压制 unused 检查。

它使每次成功扫描都维护两套长位置参数 JSON，并显著增加再次参数错位/schema 漂移的概率。`code-review-and-quality` 明确将 no-op 变量和 backwards-compat artifacts 视为应清理的死代码。

建议修复：直接删除 `_legacy_result_json`、旧 `markdown_path` 构造及重复图片扫描，只保留一个 `ScanResult` 构造/序列化入口。

### Required：stale-lock 以固定 1 小时年龄强制接管，可能删除仍活跃但阻塞的提交锁

位置：`crates/ocrx-models/src/lib.rs:331-386`

当前 stale 条件是“锁年龄超过 1 小时 **或** owner 已死亡”。这使 Windows 崩溃锁最终可恢复，但也意味着任何仍活跃、因慢盘/网络文件系统/安全软件而卡住超过 1 小时的 commit 会被第二进程强制删除锁。第二进程随后与第一进程并发操作同一最终文件和备份。

由于非 Unix `process_alive` 恒为 true，Windows 实际完全依赖这个时间阈值，无法同时做到活跃锁安全与及时崩溃恢复。

建议修复：使用跨平台 OS advisory lock；若暂时保留 lockfile，年龄只能作为“需要 repair/人工确认”的信号，不能单独授权删除已知 owner 的锁。至少使用 owner token + 平台存活检测，并加入活跃旧锁与死亡旧锁的双进程测试。

## 已确认修复

- `--report json` stdout 与落盘 `result.json` 现在复用同一个 schema v3 字符串。
- input 使用 canonical path，elapsed、profile quantization 和图片 count 不再固定占位。
- JSON 控制字符转义已补全。
- lockfile 空 owner 不再被直接当作 stale；Drop 删除前核对 owner。
- Windows stale lock 存在基于年龄的恢复路径。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo test --workspace --offline`：通过；103 passed、0 failed、1 ignored。
- 未运行 Windows CI 或真实 OvisOCR2/PDFium 端到端测试。

## 五维摘要

- 正确性：截断结果被错报为 success；插图计数受 keep_pages 污染。
- 安全性：时间阈值可绕过活跃安装互斥。
- 架构：adapter 结构化页面结果应贯穿 checkpoint 和最终报告；结果 schema 只保留一个实现。
- 性能：legacy JSON 会重复扫描图片目录并进行无用字符串分配。
- 可读性：`_legacy_result_json` 是明确死代码，应删除。
