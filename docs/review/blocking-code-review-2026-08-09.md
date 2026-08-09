# OCR-CLI 阻断性代码评审（2026-08-09）

## 结论

**Request changes。** 当前实现存在会破坏核心扫描流程、已安装模型和断点续扫正确性的阻断性问题，不建议作为 v0.1 发布候选。

评审范围覆盖 Rust workspace 的正确性、可读性、架构、安全性和性能，并以 `docs/requirement/requirements-v0.1.md` 为行为基线。本文只记录阻断性问题；大文件拆分等非阻断建议未列入。

## 阻断项

### Required：workspace 测试不隔离用户 HOME，当前环境直接失败

位置：`crates/ocrx-cli/src/main.rs:1457-1475`

`doctor_json_report_has_stable_golden_shape` 期望 `skill_installed=false`，但被测函数直接读取真实 `HOME/.codex/skills/ocrx/SKILL.md`。当开发者机器已安装该 Skill 时，测试得到 `true` 并失败。本次运行 `cargo test --workspace` 即在此断言失败（19 passed, 1 failed，随后停止）。

这使合并质量门无法在受支持的真实开发环境中稳定通过，也说明 doctor 的测试边界没有隔离进程环境。

建议修复：让 `doctor_json_report_with_cache` 同时接收明确的 home/skill path（生产入口再注入真实 HOME），测试传入独立临时目录；不要在并行测试中修改进程级 `HOME`。增加 skill 存在和不存在两种确定性用例。

### Required：仓库声明的 Clippy 质量门无法通过

位置：workspace 多处；首批错误见 `crates/ocrx-output/src/lib.rs:275`、`crates/ocrx-skill/src/lib.rs:27`、`crates/ocrx-models/src/lib.rs:36`

`Cargo.toml` 为 workspace 启用了 `clippy::all` 和 `clippy::pedantic`，但 `cargo clippy --workspace --all-targets -- -D warnings` 当前失败。输出包含未嵌套模式、公共 `Result` API 缺失 Errors 文档、截断 cast、可折叠控制流及不必要分配等大量错误。无论这些单项是否影响运行时，它们共同意味着仓库自己的静态质量门是红色的，不能进入合并/发布状态。

建议修复：先修正高信号的类型转换和控制流告警，再补齐公共 API 文档；对确属项目有意选择的 pedantic lint，在最小作用域添加带理由的 `allow`，不要全局关闭 lint。修复后以同一条 `-D warnings` 命令纳入 CI。

### Critical：默认 `scan` 无法兑现“一条命令完成 OCR”的模型安装行为

位置：`crates/ocrx-cli/src/main.rs:875-893, 1013-1017`

`run_scan` 只在 `--offline` 且模型文件缺失时提前报错。非离线模式下，代码既不验证缓存资产，也不调用 `install_from_urls`；它直接把预期缓存路径交给运行时加载。因此全新安装后执行文档中的 `ocrx scan document.pdf`，最终只会因模型文件不存在而以 runtime 错误退出。`--non-interactive` 也没有参与任何下载决策。

这违反需求 4.1、5.3 和 6.3：默认扫描应在策略允许时安装受信任的公开模型，离线模式才应立即失败。它使产品的核心入口在干净环境中不可用。

建议修复：在创建 `RuntimeLoader` 前增加独立的“解析 profile → 验证本地资产 → 按 offline/interactive 策略安装 → 再次验证”编排步骤；复用 `ocrx-models` 的规范安装与验证函数，并把模型/下载错误保留为模型类退出码，而不是降级成 runtime 错误。

### Critical：失败的安装事务会删除原本完好的已安装模型

位置：`crates/ocrx-models/src/lib.rs:311-339, 375-394`

`install_from_urls` 和 `install_from_files` 在任何错误后，不仅删除 `.part` 文件，还无条件删除最终的 `model.gguf` 与 `mmproj.gguf`。当用户对已安装 profile 重试安装，而下载、源文件复制、哈希校验或第二次 rename 失败时，之前可用的模型会被删除。

这既是用户数据/缓存破坏，也是事务语义错误；函数注释声称“verification-before-commit”，但实现无法保留既有提交。URL 安装还会先覆盖 `model.gguf`，随后才提交 `mmproj.gguf`，中途失败可短暂产生不匹配资产对。

建议修复：把新资产写到同目录内唯一的事务临时名，全部下载、尺寸与哈希验证通过后再提交；覆盖已有安装时使用可回滚的备份或目录级原子切换。失败清理只能删除本事务创建的临时文件，绝不能删除进入事务前已存在的最终文件。增加“已有有效安装 + 新安装哈希失败后旧资产逐字节保留”的回归测试。

### Critical：断点续扫没有验证输入或页面产物，可能静默输出错误文档

位置：`crates/ocrx-cli/src/main.rs:1022-1029`; `crates/ocrx-output/src/lib.rs:475-481, 494-500, 815-816`

`CompatibilityKey.input_sha256` 实际保存的是输入路径字符串，`PageCheckpoint.output_sha256` 实际保存固定的 `page-N`。`is_reusable` 只检查这些字符串是否相等/非空，不读取或校验页面 Markdown/图片。

后果包括：

- 同一路径的 PDF 内容被替换后，`--resume` 仍会复用旧 OCR 页面；
- 页面 Markdown 被修改或损坏后仍被当作有效结果；
- 页面 Markdown 被删除时先跳过 OCR，最后合并才整体失败，而不是重新处理该页；
- checkpoint 文本中的换行或 `=` 也没有编码边界，路径可令 checkpoint 无法可靠回读。

这违反需求对输入 SHA-256、输出 SHA-256 和兼容性匹配的明确要求，可能产生表面成功但内容来自另一份 PDF 的结果。

建议修复：在扫描开始时流式计算真实输入 SHA-256；提交页产物后计算实际产物摘要（或明确的多文件摘要）；resume 时同时验证 compatibility、必需产物存在性和摘要。将 checkpoint 改为带 schema 的结构化序列化格式，避免自行拼接未转义字段。增加“同路径换文件”“篡改页 Markdown”“删除页 Markdown”三个回归测试。

### Required：`--max-image-tokens` 与 `--context-size` 被接受但完全不生效

位置：`crates/ocrx-cli/src/main.rs:177-190, 1018-1029`; `crates/ocrx-runtime/src/llama_mtmd.rs:96-108`

CLI 解析并校验这两个参数，但没有把它们传入 runtime：MTMD 的 image token 上下限固定为 1024，context 固定为 16384。compatibility key 也未包含二者。因此用户指定的内存/上下文限制被静默忽略，且未来修复后旧 checkpoint 仍可能被错误复用。

默认 `max_output_tokens` 同为 16384，而 prompt 与图像已占用上下文；若模型未提前生成 EOS，解码会在达到用户声明的输出上限前撞上固定 context 边界。这会把合法的长页识别记为普通页面失败。

建议修复：建立显式的 `InferenceConfig`，由 CLI 完整解析后传到 runtime 初始化及每页请求；在启动前校验 `prompt/image budget + max_output_tokens <= context_size`，或按剩余上下文限制输出；所有影响结果的字段都进入 checkpoint compatibility。增加参数传播和边界预算测试。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo test --workspace`：失败；`doctor_json_report_has_stable_golden_shape` 读取真实 HOME，结果为 19 passed、1 failed，测试进程随后停止。
- `cargo clippy --workspace --all-targets -- -D warnings`：失败；多个 workspace crate 存在 lint error。
- 未执行真实模型/PDFium 端到端测试：该验证依赖本机外部运行时与大模型资产，且上述问题可由静态控制流和现有单元边界直接确认。

## 质量维度摘要

- 正确性：存在上述四项阻断问题。
- 安全性：本轮未确认到独立的远程代码执行、注入或秘密泄露阻断项；模型完整性仍受“扫描前不校验缓存”影响，已包含在第一项修复路径中。
- 架构：模型准备、推理配置和 checkpoint 完整性缺少明确边界，是多个阻断问题的共同根因。
- 性能：未确认独立性能阻断项；上下文预算错误会造成长页无效计算和失败。
- 可读性：`ocrx-cli/main.rs` 与 `ocrx-output/lib.rs` 均超过 1,500 行，是后续拆分信号，但不单独阻断本次结论。
