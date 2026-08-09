# OCR-CLI 阻断性代码复评 v2（2026-08-09）

## 结论

**Request changes。** 上一版报告中的测试隔离、Clippy、参数传播及“缺失模型不安装”等问题已有明显改进，三个自动化质量门均已通过；但模型完整性、安装事务、断点产物校验和稳定结果契约仍存在阻断性缺口。

本报告为第二轮评审，只记录当前仍阻断合并/发布的问题，不覆盖 `blocking-code-review-2026-08-09.md`。

## 阻断项

### Critical：`scan` 仍不验证已存在模型的哈希与配对关系

位置：`crates/ocrx-cli/src/main.rs:896-943, 1070-1076`

扫描前的模型准备只判断 `model.gguf` 和 `mmproj.gguf` 是否为文件。只要两个路径存在，无论文件是否下载中断、被篡改、属于不同 revision，都会直接交给 `LlamaMtmdRuntime::load`。非离线和离线路径都没有调用现成的 `verify_manifest_files_with_sizes` 与 runtime/adapter compatibility preflight。

这意味着损坏或不匹配的资产可能进入原生模型加载层；即使加载最终报错，也无法给出稳定的模型完整性错误，更无法满足需求中“语言模型和 mmproj 不匹配时禁止启动推理”和“加载前必须执行兼容性 preflight”的约束。

建议修复：提取单一的 `prepare_model` 编排边界。对现有缓存先按 pinned manifest 校验尺寸、SHA-256、revision 和 runtime/adapter compatibility；校验失败时，离线模式返回模型错误，联网模式按明确策略重新安装并再次校验。`doctor` 的 installed 状态也应复用该规范验证，而不是仅检查文件存在。

### Critical：模型资产对的最终提交仍不是原子事务

位置：`crates/ocrx-models/src/lib.rs:307-339, 372-393`

修复已避免在失败清理时删除旧文件，但最终仍依次执行两次 rename：先覆盖 `model.gguf`，再覆盖 `mmproj.gguf`。若第二次 rename 失败，函数返回 `TransactionFailed`，缓存中却已经留下“新 model + 旧 mmproj”的混合资产对；旧 model 也无法恢复。以 PID 命名临时文件还会让同一进程内针对同 profile 的并发/重入安装共享临时路径。

因此当前实现仍不符合函数宣称的安装事务语义，并可能把原有有效安装变成不匹配安装。

建议修复：在独立、唯一的 staging 目录中准备并校验完整资产和 manifest，然后通过 revision/profile 目录级原子切换提交；或者在文件级提交时为两个旧文件建立可回滚备份并在任一步失败时恢复。临时事务 ID 应保证同进程多次调用也唯一。增加“第二个最终提交失败后旧资产对逐字节保持不变”的故障注入测试。

### Critical：稳定的 `result.json` 契约尚未实现

位置：`crates/ocrx-cli/src/main.rs:1096-1160`；仓库内没有写入 `result.json` 的实现

成功扫描只通过 `Reporter` 向终端输出精简的 `ResultReport`，没有在输出目录生成 `result.json`。当前 JSON 也不包含需求规定的 `markdown_path`、模型 revision/profile、后端、页级状态、渲染尺寸、耗时、warnings 和图片信息。代码却在输出目录冲突判断中检查 `result.json`，表明文件契约并未真正接通。

这直接阻断 Agent 场景：需求明确要求 Agent 从最终 JSON 的 `markdown_path` 获取结果，并要求 Skill 依赖稳定 CLI 与 `result.json` 契约。

建议修复：定义版本化、可序列化的最终结果模型，由扫描编排在页面产物和 `document.md` 提交后原子写入 `result.json`；stdout 的 `--report json` 应复用同一模型，避免两套 schema 漂移。为完整成功、部分失败、取消/恢复分别增加 golden tests。

### Required：断点摘要仍只覆盖 Markdown，图片产物损坏会被静默复用

位置：`crates/ocrx-output/src/lib.rs:820-839, 1031-1042`

`artifact_sha256(markdown, _image)` 明确忽略了传入的图片路径，只计算 Markdown 哈希。由识别器生成的 bbox 插图也不在 checkpoint 摘要中。因此 `--keep-pages` 场景下全页 PNG 被篡改不会触发重算；更重要的是 Markdown 引用的裁剪插图缺失或损坏时，checkpoint 仍会被判为可复用，最终文档留下断链图片。

建议修复：为每页建立确定性的产物清单，至少包含页 Markdown 及其引用的所有 figure 文件；摘要应覆盖路径、长度和内容，并在 resume 时验证文件存在与摘要一致。若全页 PNG 在非 `--keep-pages` 模式本就会删除，应将它明确排除在可复用产物模型之外，而不是保留一个未使用参数。

### Required：推理 token 预算缺少联合校验，合法 CLI 输入可保证运行期失败

位置：`crates/ocrx-cli/src/main.rs:266-282, 1070-1091`; `crates/ocrx-runtime/src/llama_mtmd.rs:96-139`

`--max-image-tokens` 和 `--context-size` 已成功传入 runtime，这是有效改进；但 CLI 仅分别校验最小值，没有验证 prompt/image tokens 与最大输出 tokens 是否能放入 context。默认值本身是 `context_size=16384`、`max_output_tokens=16384`，尚未计入 prompt 和默认 1024 image tokens。模型不提前产生 EOS 时必然在达到声明的输出上限前触碰 context 边界并把页面记为失败。

用户还可以显式传入 `--context-size 1024 --max-image-tokens 1024 --max-output-tokens 16384`，解析阶段仍会接受这个不可能执行的组合。

建议修复：建立统一的 `InferenceConfig::validate`，在下载/加载模型前验证上下文预算，并为 prompt/tokenization 的不可预知余量保留明确空间；或根据 `eval_chunks` 返回的位置动态把生成上限夹到剩余 context，并将正常耗尽映射为 truncated，而不是普通失败。增加默认配置与极端组合测试。

### Required：交互式自动下载绕过确认，可能无提示下载大型资产

位置：`crates/ocrx-cli/src/main.rs:896-925`

当模型缺失且未指定 `--offline` 时，无论 stdin 是否为 TTY、是否指定 `--non-interactive`，代码都直接将 `license_accepted=true` 并开始下载。需求规定交互模式必须先展示模型、来源、大小、磁盘空间和许可并确认；当前实现会无提示下载约数百 MB 至 GB 的模型资产。

建议修复：将安装策略建模为显式的交互/非交互分支。交互模式确认后才调用安装；非交互模式只允许受信 manifest 并遵从配置；两者都应在下载前执行磁盘预算检查。为拒绝确认、非 TTY 自动安装和 offline 三条路径增加测试。

## 验证记录

- `cargo fmt --all -- --check`：通过。
- `cargo test --workspace`：通过；102 passed、0 failed、1 ignored（需要真实模型的 gated E2E）。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- 未执行真实 OvisOCR2/PDFium 端到端：真实模型测试仍被环境变量 gate 忽略。

## 五维摘要

- 正确性：模型准备、事务提交、结果文件和 resume 产物完整性仍有阻断项。
- 安全性：缓存资产未在加载前验证，外部模型数据边界不够严格。
- 架构：建议收敛为 `prepare_model`、`InferenceConfig` 和单一最终结果 schema 三个明确边界。
- 性能：无新增独立性能阻断项，但无效 context 组合会造成长时间计算后失败。
- 可读性：修改总体改善了原问题；本轮不以文件大小或风格问题阻断。
