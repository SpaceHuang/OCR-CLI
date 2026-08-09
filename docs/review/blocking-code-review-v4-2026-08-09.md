# OCR-CLI 阻断性代码复评 v4（2026-08-09）

## 结论

**Request changes。** v3 的断点图片发现、运行时跨平台哈希依赖和 PDF worker 测试抖动已修复，格式、完整测试和严格 Clippy 均通过。但新哈希实现会在模型校验时产生超大内存峰值，`result.json` 仍未满足机器可读契约，模型事务回滚仍可能破坏已有安装。

本报告为第四轮评审，只记录当前阻断项，不覆盖此前报告。

## 阻断项

### Critical：模型 SHA-256 校验将整个 GGUF 读入内存，可能在启动前造成 OOM

位置：`src/ocrx-core/src/lib.rs:6-10`; 调用路径见 `src/ocrx-models/src/lib.rs:196-205, 253-257`

新的跨平台 `sha256_file` 使用 `read_to_end` 把整个文件装入 `Vec<u8>`，随后 `sha256` 又执行 `data.to_vec()` 创建第二份完整副本用于 padding。对需求中约 529MB、812MB、最高约 1.52GB 的模型文件，单次校验峰值接近文件大小的两倍；校验 model 后还要校验约 205MB mmproj。8GB 最低设备或当前可用内存紧张时，程序可能仅为哈希瞬时占用约 3GB，并在真正加载模型前 OOM/严重 swap。

这与下载校验必须可靠及低内存设备支持直接冲突，也违背“流式计算文件哈希”的修复目标。

建议修复：实现增量 SHA-256 state（固定 64-byte block buffer、累计长度、finalize padding），`sha256_file` 用固定大小缓冲循环读取；内存应为 O(1)。保留 `sha256(&[u8])` 作为小数据便利包装，但不要让文件接口读取全文件。增加多块输入、空输入、标准 NIST 向量和大稀疏/生成文件的恒定内存测试或基准。

### Critical：落盘 `result.json` 仍可能是非法 JSON，且未实现需求最低 schema

位置：`src/ocrx-cli/src/main.rs:1198-1223, 1273-1279`

修复已让 partial 的 `markdown_path` 为 null，并转义反斜杠、双引号、换行和回车；但仍通过 `format!` 手拼 JSON。`json_escape_path` 没有转义 tab、backspace、form-feed 及其他 U+0000–U+001F 控制字符，合法 Unix 路径包含这些字符时仍会生成无效 JSON。

文件 schema 仍缺少需求 8.4 的最低字段：`input`、结构化 model（revision、mmproj revision、adapter/version、prompt schema、runtime version、quantization）、`pages.truncated`、images、elapsed_ms，以及页面/渲染诊断。当前 `model` 只是字符串，页数也不是规定的结构。仓库没有针对实际落盘 `result.json` 的解析或 golden test；现有 `result_json_is_versioned_and_escapes_user_values` 测的是另一个精简 `ResultReport`。

该文件是 Agent 获取 `markdown_path` 和判断任务结果的稳定契约，不能以“通常可解析”的字符串拼接交付。

建议修复：定义单一、强类型、版本化的 `ScanResult` 并使用成熟 JSON serializer，stdout `--report json` 与落盘文件复用同一结构。为全部控制字符路径、success、partial 和需求最低 schema 添加“写入后由 JSON parser 回读”的测试。

### Critical：模型事务回滚仍忽略恢复失败，可能破坏已有有效安装

位置：`src/ocrx-models/src/lib.rs:268-304`

`commit_pair` 与 v3 相同：第二次提交失败后删除最终文件，再用 rename 恢复备份，但所有恢复结果均以 `let _ =` 丢弃。如果磁盘、权限、杀毒软件占用或目录状态导致恢复失败，函数只返回通用 `TransactionFailed`，旧模型可能丢失或只恢复一半。成功提交后的备份删除失败同样被忽略，可能遗留 GB 级备份。

两文件依次 rename 期间也没有读写锁，并发 scan 可以观察到资产缺失或半提交状态。现有测试仍只覆盖哈希失败发生在提交前，没有覆盖第二次提交失败与恢复失败。

建议修复：采用唯一 staging 目录和已提交 revision 目录的原子指针/目录切换，让扫描只读取完成标记后的不可变目录；或实现带锁和可验证状态机的文件级事务，任何恢复失败必须保留备份并返回可诊断状态。增加故障注入和并发读取测试。

### Required：Windows 测试仍依赖 `shasum`，跨平台质量门会失败

位置：`src/ocrx-models/src/lib.rs:638-650, 680-690`（测试模块）

生产代码已切换到 `ocrx_core::sha256_file`，但模型测试的 hash closure 仍启动 `shasum -a 256`。Windows 10/11 是首发平台，标准环境没有该命令；因此这些单元测试会在 Windows CI 直接失败，恰好无法验证本轮跨平台修复。

建议修复：测试同样调用 canonical Rust helper，另用固定公开 SHA-256 向量验证 helper 本身，避免测试与被测实现循环自证。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo test --workspace`：通过；102 passed、0 failed、1 ignored。
- 真实 OvisOCR2/PDFium 端到端测试仍因环境 gate 被忽略。

## 五维摘要

- 正确性：结果契约和模型事务仍有阻断风险。
- 安全性：跨平台内建哈希方向正确，但缺少独立标准向量验证；模型事务失败可能损伤缓存完整性。
- 架构：应引入流式 hash state、单一 `ScanResult` 模型和不可变 revision 提交边界。
- 性能：整文件及其副本同时驻留是明确的高峰内存阻断项。
- 可读性：图片摘要修复已与实际 Markdown 格式对齐；手写 JSON 仍是应删除的脆弱重复实现。
