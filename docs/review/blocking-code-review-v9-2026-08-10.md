# OCR-CLI 阻断性代码复评 v9（2026-08-10）

## 结论

**Request changes。** v8 的首次扫描截断统计、figure 计数、legacy JSON 死代码和按年龄抢占锁已修复，自动化质量门全部通过。但 checkpoint 没有保存页面截断状态，resume 会把原本 partial 的任务错误升级为 success；其他 adapter warnings 仍未进入最终契约。

本报告为第九轮评审，只记录当前阻断项，不覆盖此前报告。

## 阻断项

### Critical：resume 不恢复截断状态，同一结果第二次运行会从 partial 变成 success

位置：`src/ocrx-output/src/lib.rs:456-538, 750-766, 862-944`; CLI 判定见 `src/ocrx-cli/src/main.rs:1190-1275`

首次识别时，recognizer 返回 `was_truncated=true`，`ScanService` 会增加 `truncated`，最终正确输出 partial、退出码 10。但 `PageCheckpoint` 仍只保存 page、compatibility、complete 和 output SHA，没有 termination/truncated/warnings。

使用 `--resume` 时，兼容且完整的 checkpoint 在循环开始被直接复用：代码只执行 `completed += 1`，无法恢复该页原先的 truncated 状态。若所有页面都来自 checkpoint，summary 得到 `truncated_pages=0`、`failed_pages=0`，最终输出 success 并以 0 退出，即使页面内容仍是同一份被截断的 Markdown。

这会让任务状态依赖“本次是否实际重新推理”，而不是产物真实质量；Agent 重试 resume 后会被错误告知文档完整。

建议修复：升级 checkpoint schema，保存 termination 和结构化 warnings；resume 复用页时将它们累加到 summary。旧 checkpoint 不具备状态时应视为不兼容并重新处理，而不是默认完整。增加“首次 truncated → resume 不调用 recognizer → 仍 partial、truncated=1、退出码 10”的回归测试。

### Required：页面 warning 被压缩成布尔 truncated，其他诊断永久丢失

位置：`src/ocrx-ovisocr2/src/lib.rs:36-85`; `src/ocrx-output/src/lib.rs:420-532`; `src/ocrx-cli/src/main.rs:1332-1370`

`adapt_response` 已能产生 `PageWarning::Truncated`、`RepetitionRemoved` 等 warnings，但新的 `PageArtifactOutput` 只暴露 `truncated: bool`。recognizer trait 也只传 `(PNG, Markdown, bool)`。最终 schema 根据 truncated_pages 人工生成单一 `["truncated"]`，因此 repetition removal、render scaling、无效区域等已知诊断不会出现在 `result.json`。

需求要求 warnings 供用户和 Agent 判断结果质量；仅保留一个布尔值会让新增 warning 每次都需要继续扩展 tuple 和汇总逻辑。

建议修复：用显式 `PageArtifactResult` 替代 tuple，携带 termination、`Vec<PageWarning>` 和 figure artifacts。ScanService 汇总去重后的 job warnings，checkpoint 持久化页面 warnings，最终 `ScanResult` 直接序列化汇总结果。增加 repetition-removed 与多页 warning 去重测试。

### Required：Windows lock 存活判断依赖本地化命令文本且没有目标平台测试

位置：`src/ocrx-models/src/lib.rs:331-392`

按年龄强制抢锁已删除，这是正确修复。Windows 现在调用 `tasklist /FI "PID eq ..." /NH`，再用 stdout 是否包含 PID 字符串判断存活。该判断依赖外部命令、输出格式/本地化，并使用子串匹配；命令失败会返回 false，随后删除可能仍活跃的锁。当前模型测试没有覆盖 stale owner、active owner 或两个进程竞争，且本轮未运行 Windows CI。

建议修复：使用 Windows API/跨平台进程或 advisory-lock 库进行句柄级存活与锁定；至少严格解析 tasklist 行中的 PID 列，并把命令失败视为“未知、不可接管”。增加 Windows 双进程测试后再将该路径视为已验证。

## 已确认修复

- 首次 max_tokens 页面会计入 truncated_pages，任务为 partial 并退出 10。
- figure count 只统计带 `-figure-` 的 PNG，不再受 `--keep-pages` 整页图影响。
- `_legacy_result_json` 已删除，stdout 与落盘继续复用 `serialize_scan_result`。
- lockfile 不再仅凭一小时年龄抢占；Windows 增加 owner 存活探测。
- schema v3 新增 truncation/path escaping 单元测试。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo test --workspace --offline`：通过；104 passed、0 failed、1 ignored。
- 未运行 Windows CI 或真实 OvisOCR2/PDFium 端到端测试。

## 五维摘要

- 正确性：resume 会丢失质量状态并错误升级任务结果。
- 安全性：Windows lock 接管仍依赖脆弱的外部命令判断。
- 架构：页面结果应使用显式结构贯穿 adapter、checkpoint、summary 和最终 schema。
- 性能：未发现新的性能阻断项。
- 可读性：三元 tuple + bool 已不足以表达页面识别结果，应替换为命名模型。
