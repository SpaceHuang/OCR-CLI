# OCR-CLI 阻断性代码复评 v6（2026-08-10）

## 结论

**Request changes。** v5 的 JSON 参数错位和回滚提前返回已修复，格式、完整测试和严格 Clippy 均通过。但 `result.json` 的字段真实性与序列化仍不符合稳定契约；新 stale-lock 恢复实现存在竞态，并在 Windows 上完全不能区分活跃锁与陈旧锁。

本报告为第六轮评审，只记录当前阻断项，不覆盖此前报告。

## 阻断项

### Critical：Windows 上安装锁会把任何活跃锁当作 stale，允许并发提交同一模型

位置：`crates/ocrx-models/src/lib.rs:327-365`

非 Unix 实现的 `process_alive` 无条件返回 false。Windows 是 v0.1 首发平台；当进程 A 持有 `.install.lock` 时，进程 B 读取到 A 的 PID 后仍会判定其“不存活”，删除 A 的锁并获取新锁。A 和 B 随后都可进入 `commit_pair`，同时 rename 相同的 model/mmproj/backup 路径，破坏互斥及回滚假设。

这会重新引入混合资产、备份冲突和事务失败，且只在目标首发平台发生。当前测试仅在本机 Unix 环境运行，无法捕获。

建议修复：使用跨平台 OS advisory lock（Windows 文件句柄共享模式/成熟锁库），让锁随进程退出自动释放。若必须使用 PID lockfile，Windows 必须实现真实进程存活检测，不能把未知状态视为 stale；无法确认 owner 已死亡时应保守拒绝接管。增加 Windows CI 的双进程互斥测试。

### Critical：lockfile 创建与写入 PID 分成两步，空文件窗口可让第二进程删除活跃锁

位置：`crates/ocrx-models/src/lib.rs:328-352`

获取流程先以 `create_new(true)` 创建空锁文件，随后才调用 `fs::write` 写 PID。两步之间，第二进程可能看到锁文件、读取为空、得到 `owner=None`，而 `owner.is_none_or(...)` 会立即把它视为 stale，删除该活跃锁并创建自己的锁。第一进程随后又会按路径写入第二进程新建的锁文件，最终两个进程都认为自己持锁。

此外，`Drop` 只按路径删除锁，不核对 owner token；在上述竞争后，A 的 Drop 还可能删除 B 的锁。

建议修复：不要用“路径存在 + 后写 owner”模拟互斥。持有 `create_new` 返回的文件句柄并直接向该句柄写 owner、flush；接管 stale lock 时需用不可伪造的 owner token，并在删除/Drop 前确认文件仍属于自己。但这仍无法彻底消除 TOCTOU，最佳方案是操作系统 advisory lock。增加 barrier 控制的并发测试，强制第二进程在 create 与 owner write 之间竞争。

### Critical：`result.json` 仍报告虚假或错误类型的数据，无法作为 Agent 稳定契约

位置：`crates/ocrx-cli/src/main.rs:1198-1227`; 需求基线 `docs/requirement/requirements-v0.1.md:452-487`

参数顺序现已正确，但 v5 已指出的字段问题没有解决：

- `input` 仍使用 `options.input` 原始字符串，可能是相对路径，而契约要求规范绝对路径；
- `elapsed_ms` 固定为 0；
- `images` 是 `{ "directory": ... }`，最低 schema 要求图片数量；
- `quantization` 固定为 `community`，不是实际 Q4_K_M/Q5_K_M/Q8_0/BF16；
- 缺少 name、family、adapter_version、prompt_schema_version 和 pinned runtime commit；
- stdout JSON 继续使用另一套 `ResultReport` schema。

这些值会让 Agent 对产物位置、模型基线、耗时和图片数量做出错误判断，不是非阻断的展示差异。

建议修复：由规范化 input/output、profile→quantization 映射、pinned manifest/runtime 常量、实际图片清单与真实 `Instant` 耗时构造强类型 `ScanResult`；stdout 与落盘复用同一结构。新增 success/partial golden test，并逐字段断言值而非只检查字符串。

### Required：路径转义仍不能覆盖全部 JSON 控制字符，且仍是手写 JSON

位置：`crates/ocrx-cli/src/main.rs:1277-1283`

`json_escape_path` 仅处理反斜杠、双引号、LF 和 CR，没有处理 tab、backspace、form-feed 及其余 U+0000–U+001F。合法 Unix 文件名包含这些字符时会生成非法 JSON。仓库仍没有对实际落盘 schema 做 JSON parser 回读测试。

建议修复：删除该 helper 和整段 `format!` JSON，使用可靠 serializer；测试路径至少覆盖引号、反斜杠、换行、tab 和其他控制字符，并确保写入结果可以被标准 JSON parser 回读。

## 已确认修复

- `result.json` 的 input/markdown_path 参数顺序已纠正；partial 使用 null。
- 回滚分支不再在第一个清理错误处提前返回，会继续尝试恢复两个备份。
- 增量 SHA-256 保持通过现有 NIST/分块测试，文件读取为固定内存。
- 模型测试已移除 `shasum` 依赖。
- PDF worker 成功测试使用 10 秒超时，本轮并发质量门未出现抖动。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo test --workspace --offline`：通过；103 passed、0 failed、1 ignored。
- 未运行 Windows CI 或真实 OvisOCR2/PDFium 端到端测试。

## 五维摘要

- 正确性：目标平台并发安装互斥失效；结果契约包含错误数据。
- 安全性：模型完整性验证本身已改善，但竞态可破坏已验证资产的提交边界。
- 架构：lockfile 应替换为 OS 锁；结果输出应替换为单一强类型 schema。
- 性能：未发现新的性能阻断项。
- 可读性：回滚控制流有所改善；手写 JSON 和 PID lockfile 仍在重复实现成熟基础设施。
