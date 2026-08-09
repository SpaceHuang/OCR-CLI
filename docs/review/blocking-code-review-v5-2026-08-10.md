# OCR-CLI 阻断性代码复评 v5（2026-08-10）

## 结论

**Request changes。** 流式 SHA-256、模型测试跨平台性和显式回滚检查已明显改善，全部自动化质量门通过；但新 `result.json` 的格式参数顺序错误会直接生成非法 JSON，且模型安装锁在进程异常退出后会永久阻塞安装。

本报告为第五轮独立评审，只记录当前阻断项，不覆盖此前报告。

## 阻断项

### Critical：`result.json` 的格式参数错位，正常成功结果通常不是合法 JSON

位置：`crates/ocrx-cli/src/main.rs:1198-1227`

格式字符串在 status 后依次要求：

1. `input`（位于 JSON 字符串引号内）；
2. `markdown_path`（调用方负责提供 JSON string 或 null）；
3. `output_dir`。

实际传入顺序却是：

1. `markdown_path`；
2. `json_escape_path(options.input)`；
3. `output`。

成功时 `markdown_path` 变量本身已包含双引号，因此输出类似：

```json
{"input":""/abs/out/document.md"","markdown_path":relative/input.pdf}
```

partial 时则会得到 `"input":"null"`，并继续把未加 JSON 引号的输入路径放入 `markdown_path`。除非输入文本碰巧是合法 JSON token，否则落盘文件不可解析。当前测试没有执行这段实际落盘序列化，所以质量门未捕获。

建议修复：停止使用位置参数手拼 schema，定义强类型 `ScanResult` 并交由 JSON serializer 序列化；至少先纠正参数顺序，并新增“生成实际 result.json → JSON parser 回读 → 逐字段断言”的 success/partial 测试。不要只比较字符串片段。

### Critical：扩展后的 `result.json` 字段仍不满足真实稳定契约

位置：`crates/ocrx-cli/src/main.rs:1205-1222`; 需求基线 `docs/requirement/requirements-v0.1.md:452-487`

即使修正参数顺序，当前实现仍有会误导 Agent 的确定性数据错误：

- `input` 使用用户原始参数，可能是相对路径；需求要求路径为规范化绝对路径；
- `elapsed_ms` 永远为 0，不是任务耗时；
- `images` 被写成 `{ "directory": ... }`，而最低 schema 要求图片数量；
- `quantization` 固定写成 `community`，这不是 compact/balanced/quality/reference 对应的量化格式；
- model 缺少 name、family、adapter_version、prompt_schema_version，runtime 字段也没有表达 pinned llama.cpp commit；
- stdout 的 `ResultReport` 仍是另一套 schema，没有复用落盘结构，契约继续漂移。

这不是可选的丰富元数据：Agent/Skill 依赖该文件定位产物、判断模型基线和解释 partial 结果。

建议修复：在扫描开始记录 `Instant`，从规范化 input/output、pinned manifest、profile→quantization 映射、adapter/runtime 常量及实际图片清单构造同一个 `ScanResult`。stdout JSON 和 `result.json` 必须复用同一序列化入口，并以需求最低 schema 做 golden test。

### Required：安装锁在崩溃后永久残留，后续安装无法自愈

位置：`crates/ocrx-models/src/lib.rs:318-337`

`InstallLock` 通过 `create_new(.install.lock)` 获取锁，只在 Rust `Drop` 中删除。正常返回和 panic unwind 可以释放，但进程被 kill、断电或 abort 时不会执行 Drop；此后锁文件永久存在，所有安装都会立即返回 `TransactionFailed`。没有 PID/时间元数据、存活检测、过期策略或用户可操作错误，`model remove` 也不会清理 profile 父目录中的该锁。

模型下载/安装本身正是容易被用户中断的长流程，因此 stale lock 属于现实故障路径，而不是理论边角。

建议修复：优先使用操作系统 advisory file lock，使锁随进程退出自动释放；若保留 lockfile，写入唯一 owner/PID/创建时间，获取失败时验证 owner 是否存活并安全接管 stale lock。增加“预置 stale lock 后能够恢复”和“活跃锁阻止并发提交”的测试。

### Required：回滚在清理新文件失败时提前返回，跳过旧资产恢复

位置：`crates/ocrx-models/src/lib.rs:291-307`

提交失败后，代码先删除新 `model`/`mmproj`。若任一 `remove_file` 失败且路径仍存在，会立即返回，后面的 backup rename 完全不执行。此时旧的有效资产仍在 `.bak`，规范路径却保持半提交状态；扫描无法使用，且 stale lock/备份需要人工理解内部命名才能恢复。

检查错误结果是进步，但回滚状态机仍应“尽最大努力执行所有恢复步骤，再汇总错误”，不能在第一项清理失败时短路其余恢复。

建议修复：记录每一步结果而不提前返回；尽可能恢复两个旧资产，保留无法替换的备份，并返回包含恢复状态的诊断错误。更稳妥的结构仍是不可变 revision staging 加单一 committed 指针/目录切换。增加清理失败和第二次 rename 失败的故障注入测试。

## SHA-256 专项结论

本轮未发现增量状态机的阻断性算法错误：文件接口使用固定 64KiB 读取缓冲，状态内部为 64-byte block buffer；空输入、`abc` 和逐字节 chunking 测试通过。建议后续补充长度 55/56/63/64/65 bytes 与百万个 `a` 的标准向量，以覆盖 finalize 的双块 padding 分支，但本轮不单独阻断。

旧的整块 `sha256` 实现仍与 `Sha256State` 重复；后续可让它变为 `state.update(data); state.finalize()`，删除第二套压缩流程，避免实现漂移。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo test --workspace --offline`：通过；103 passed、0 failed、1 ignored。
- 真实 OvisOCR2/PDFium 端到端测试仍因环境 gate 被忽略。

## 五维摘要

- 正确性：落盘 JSON 当前不可可靠解析；安装崩溃/回滚路径不可自愈。
- 安全性：SHA-256 边界已由外部命令收敛为跨平台内建实现。
- 架构：应删除手拼 JSON和双 SHA 实现，使用强类型结果及单一 hash state。
- 性能：流式模型哈希已消除 v4 的超大内存峰值。
- 可读性：安装事务的错误检查更明确，但仍缺显式状态机与故障注入测试。
