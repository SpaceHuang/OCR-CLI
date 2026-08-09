# OCR-CLI 阻断性代码复评 v7（2026-08-10）

## 结论

**Request changes。** v6 的 lockfile 空文件竞态、路径控制字符转义及多数结果字段真实性已改善，自动化质量门全部通过。但 Agent 使用的 stdout JSON 仍是旧 schema，且 Windows 崩溃遗留锁无法恢复，仍阻断 v0.1 合并/发布。

本报告为第七轮评审，只记录当前阻断项，不覆盖此前报告。

## 阻断项

### Critical：`--report json` 仍输出旧的精简 schema，Agent 无法从最终 JSON 读取 `markdown_path`

位置：`src/ocrx-cli/src/main.rs:1185-1258`; `src/ocrx-output/src/lib.rs:20-46, 96-108`

CLI 已构造并落盘 schema v3 的 `result.json`，但随后传给 `reporter.result` 的仍是旧 `ResultReport`。当用户按需求执行：

```text
ocrx scan input.pdf --report json --progress jsonl --non-interactive
```

stdout 最终对象仍只有 schema_version=1、job/status、页数和 `output_dir`，没有 `markdown_path`、input、model/revision/profile、backend、warnings、images 或 elapsed_ms。需求 4.2 明确规定 Agent “读取最终 JSON 中的 `markdown_path`，而不是从终端日志中猜测输出位置”；当前主调用契约依然无法做到这一点。

落盘与 stdout 两套 schema 也会持续漂移：本轮修复只更新了手拼文件，已有 stdout golden test反而固定了旧格式。

建议修复：定义一个强类型 `ScanResult`，由单一 serializer 同时生成落盘 `result.json` 与 `--report json` stdout；text report 可以从同一对象派生摘要。把 CLI 成功测试改为解析 stdout 并断言它与落盘 JSON 的 schema/核心字段一致，尤其是 `markdown_path`。

### Critical：Windows 上崩溃遗留安装锁永远无法自动恢复

位置：`src/ocrx-models/src/lib.rs:331-381`

为避免误删活跃锁，非 Unix 的 `process_alive` 现在恒为 true。这保护了正常并发，但意味着 Windows 上只要安装进程被 kill、崩溃、断电或 abort，遗留 `.install.lock` 就会永远被视为活跃；该 profile 的每次后续安装都返回 `TransactionFailed`。模型安装是长下载后的关键步骤，异常中断是必须可恢复的现实路径。

锁文件只保存 PID，无创建时间和唯一启动标识；Unix 上 PID 被复用时也可能把旧锁误判为活跃。当前没有 stale-lock 或双进程测试。

建议修复：使用跨平台 OS advisory lock，使锁随进程句柄关闭自动释放。若坚持 lockfile，记录 owner token、PID、进程启动标识和时间，并在各平台实现可靠存活判断；未知状态可以保守拒绝，但必须提供基于年龄/显式 repair 的安全恢复路径。增加 Windows stale-lock 恢复和活跃双进程互斥测试。

### Required：实际 schema v3 仍由位置参数手拼，且没有解析回读测试

位置：`src/ocrx-cli/src/main.rs:1200-1237`

本轮修复补齐了 canonical input、真实 elapsed、profile→quantization 和图片 count，但实现仍是一条超长 `format!`，依赖十余个位置参数顺序。v5 已实际发生过参数错位导致非法 JSON，而当前 20 个 CLI 测试仍没有执行这段成功扫描后的落盘逻辑，也没有用 JSON parser 回读 schema v3。

图片 count 还统计 `images/` 中所有目录项，不限定本任务生成的 PNG；resume 或用户预置文件会让结果数量失真。`runtime` 字段使用 crate 版本形式而非需求要求的 pinned llama.cpp commit。

建议修复：删除手写 JSON，使用结构化 serializer；图片数量来自本任务明确产物清单，runtime 来自同一 pinned compatibility manifest。增加 success、partial、特殊字符路径和预置无关文件的 schema v3 回读测试。

## 已确认修复

- lockfile 创建后尚未写 PID时，第二进程不再将 owner=None 视为 stale。
- Drop 删除前会核对 owner，降低误删其他 owner 锁的风险。
- result.json 参数顺序正确，input 使用 canonical path，partial 的 markdown_path 为 null。
- 结果加入实际 elapsed、量化映射和图片 count 字段。
- JSON path helper覆盖控制字符。
- 回滚会继续尝试恢复两个旧资产。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo test --workspace --offline`：通过；103 passed、0 failed、1 ignored。
- 未运行 Windows CI 或真实 OvisOCR2/PDFium 端到端测试。

## 五维摘要

- 正确性：Agent stdout 契约仍缺关键字段；Windows 安装无法从异常退出自愈。
- 安全性：活跃锁保护改善，但 PID lockfile 仍不是可靠跨平台互斥原语。
- 架构：落盘与 stdout 应统一为一个 `ScanResult`；安装锁应使用 OS 锁。
- 性能：未发现新的性能阻断项。
- 可读性：单行位置参数 JSON 已实际导致过回归，应删除而非继续扩展。
