# 模型后端可扩展架构重构——实施日志（滚动更新）

**Worktree：** `D:/Develop/OCR-CLI/.worktree/model-backend-ext`（分支 `refactor/model-backend-extensibility`）
**计划：** `docs/develop/model-backend-extensibility-refactoring-development-plan.md`（任务勾选状态在该文件内维护）
**基线：** `docs/develop/model-backend-extensibility-refactoring-baseline.md`（T001–T008 记录）

## 已完成（每阶段一个提交）

- **阶段 A（T001–T008）**：基线记录 + 契约冻结。构建/测试全绿（428 通过/21 env-gated ignored）。
- **阶段 B（T009–T018）**：新 crate `ocrx-primitives`（取消/进度/sha256，core 改为 re-export）与
  `ocrx-inference-api`（id/asset/identity/task/backend/service/management/error 七模块，36 测试）。
  关键决策：StepProfile/RuntimeTelemetry 定义在 inference-api（runtime re-export）；`BackendHandle` 上
  RuntimeBackend trait 带 telemetry()/step_profile() 默认方法；`StaticInferenceIdentity::from_execution`
  投影函数承载 RBLK-7 字段完备性测试。
- **阶段 C（T019–T030）**：`ocrx-models` 通用化：`builtin/ovisocr2.rs`（四 profile 冻结 manifest v2）、
  `catalog.rs`（static list/info/resolve + auto 内存阶梯）、`cache.rs`（具名角色路径，与旧布局逐字节一致）、
  `verify.rs`（presence/size/hash + BindingVerifier 端口调用）、`transport.rs`（HTTPS curl + FakeTransport
  最长前缀路由）、`install.rs`（revision 组事务/.part/备份/回滚/receipt 最后写/幂等离线成功）、
  `remove.rs`（共享资产引用检查，注入 installed 谓词）、`ModelManagerImpl`。旧平铺 API 原样保留至 T086。
- **阶段 D（T031–T043）**：
  - `ocrx-runtime`：worker 协议改为 slot 键控 `WorkerTask{slot,prompt,...}`（llama_mtmd.rs 内部）；
    冻结 prompt 只存在于 legacy `Runtime` 桥（T087 删）；新 `backend.rs`：`LlamaBackend`（cfg-gated）+
    `LlamaRuntimeFactory`（冻结身份）+ 可编程 `FakeRuntimeBackend`（echo 可发散/批级窗口失败/取消感知）；
    `DECODE_GENERATION` 单源化到 api（legacy re-export）。
  - `ocrx-ovisocr2`：新增 `adapter.rs`（OvisProtocolAdapter：encode 单图/字节不变 prompt；decode
    规范化→think 剥离 Q3.1 计数→双文本；echo 原样透传）。旧业务函数保留至 T087。
  - `ocrx-scan`：T037 复制——`ocr/parser.rs`（解析管线+think/退化裁剪，30 测试含 fixture goldens）、
    `ocr/markdown.rs`（bbox 替换/链接降级）、`ocr/artifacts.rs`（PageArtifactOutput 记录）、`ladder.rs`
    （detect_column_bands）。
  - `ocrx-inference`：`registry.rs`（静态 registry + RegistryVerifier）、`binding.rs`（三元组逐字段校验）、
    `probe.rs`/`device.rs`（探测迁移+编译∩硬件+批深分层+doctor DTO）、`service.rs`（固定 prepare 顺序+
    GPU 回退重选 8+static_identity 只读查询+session encode→generate→decode+窗口批处理）、`telemetry.rs`
    （VRAM 采样器）。

## 当前状态

- 全 workspace `cargo test --locked` 通过（37 个测试目标），新 crate clippy `-D warnings` 干净。
- 计划文档勾选：T001–T043 已标 `[x]`。
- 下一步：**阶段 E（T044–T051）** output crate 的 ScanCompatibility/v6 checkpoint/v5 迁移，随后
  阶段 F（T052–T068）scan 应用主体（最大块：CLI main.rs 7071 行的 run_scan 业务 + output ScanService
  5882 行迁移）。

## 关键实现备忘（后续阶段要用）

- v5 checkpoint 写格式（output lib.rs ~2415）与 v5 OCR 兼容串（main.rs ~2920）：
  `pdf-mode=…;quality-tier=…;classifier=pdf-route-v1;extractor=pdf-text-v1;platform=…;dpi=…;tokens=…;max_image_tokens=…;context_size=…;device=…;batch=…;route=native;decode=…` + 可选 `;ocr-profile=`/`;ocr-device=`/`;pending-ocr=true`；native/empty 宽松匹配=前五参数。
- result.json schema3 字段顺序在 main.rs:4654；model 对象在 main.rs:4622（现 `llama.cpp@0.1.152` →
  目标 `llama.cpp@0.1.161`，计划明示变更）；doctor schema2 在 main.rs:4881；scan-summary 在 main.rs:4255。
- `ocrx_output::CompatibilityKey` 现有匹配语义（output lib.rs ~1690）：native 分支比较 input/model/前五参数；
  v6 的 ScanCompatibility 按具名字段重建同语义 + RBLK-2/4/6/7/8 两阶段协议。
- scan crate 现有模块：`ocr/{mod,parser,markdown,artifacts}.rs`、`ladder.rs`；待建：window.rs、render.rs、
  resume.rs、state.rs、multi_doc.rs、compatibility.rs、report.rs、telemetry.rs、plan.rs、native.rs、
  ocr/prompt.rs。
- 测试命令基线：`cargo test --workspace --locked`（全绿）；clippy 逐 crate `-D warnings`。
- Python 补丁脚本临时目录：`C:/Users/WIN11/AppData/Local/Temp/zcode-work/`。
- pdfium.dll 已复制进 worktree `vendor/pdfium/bin`（测试需要）。


## 阶段 F 进展（滚动）

- 已完成：T055（resume.rs 两阶段协议）、T059（prompt.rs 冻结字节）、T060/T061/T062（ocr/artifacts.rs
  SessionRecognizer + assemble：TaskResponse 双文本→crop→bbox PNG→链接降级；column-split 冻结合并；
  adapter 增加 think_removed 警告通道）、T063 核心（window.rs run_pages_with_session + window_tests 5 项：
  深度=身份、渲染失败诊断、取消零提交、阶梯逃逸、批级失败回退深度 1）、T064（compatibility.rs 构造器，
  batch/decode 只从身份读）。
- output 侧新增：checkpoint_v6 的 commit_page_v6/commit_native_page_v6/commit_pending_ocr_v6/
  artifacts_intact；write_render_audit/cleanup_full_page_images 已 pub。
- scan/state.rs 的 RenderAudit 改为 re-export ocrx_output::RenderAudit（单一类型）。
- scan 测试基座：test_support.rs（identity/side/pdf/v6_ocr/v5 fixtures）；window_tests.rs（FakeSession
  共享核心 + FakeRenderer）。

## 阶段 I 进行中 + Python 测试的既有失败记录（2026-11-12）

**既有失败（非重构回归，T002 规则记录）：** `python -m unittest discover -s scripts -p "test_*.py"`
有 4 个失败（stage6-gate / pdf-native-candidate / pdf-native-forbidden / regression-dispatch），
断言均为 manifest 中记录的 input_sha256 与本地重新获取的 fixture PDF 哈希不一致——这些 PDF 被
.gitignore 排除（`/assets/regression/*/inputs/*.pdf`），属于本地重新获取的 fixture 漂移。
**在主工作区（未做任何重构）运行同一命令产生完全相同的 4 个失败**，故为既有环境问题而非本次回归。
架构检查（validate-architecture-boundaries.py，新增）与 test-release-scripts.sh 均返回 0。

## 最终状态（2026-11-12，T001–T102 完成 + T103/T105 环境注记）

**102/108 任务完成**（T103/T105 带 T096 既有失败注记；T106/T107 可选未执行——真实模型故障注入与
CPU/GPU 观察需要 OCRX_E2E_MODEL+真实资产环境，本 worktree 未配置，按 §11 记为非阻塞；T108 属后续独立立项）。

**T103 环境注记：** bootstrap-check.sh 的 Python unittest 步骤有 4 个既有失败（pdf-native
candidate/forbidden/stage6-gate/regression-dispatch），断言 fixture manifest 的 input_sha256 与本地
PDF 哈希不一致。这些 PDF 被 .gitignore 排除、本地重新获取后哈希漂移；**主工作区（未做任何重构）产生
完全相同的 4 个失败**（MAIN_UNITTEST=1 已验证），故为既有环境漂移而非本次回归。其余全部步骤
（fmt/build/test/architecture/release-scripts/version/doctor）返回 0。

**§11 完成定义逐项核对：** crate 边界落地 + 禁止依赖检查通过（validate-architecture-boundaries.py 返回 0）；
CLI 7071→~3300 行，旧编排器/绕路入口已删；生产仍单模型 OvisOCR2、pin 0.1.161/d8123504 未变；scan 只依赖
inference-api（架构脚本锁定）；模型管理/doctor 独立运行且与 scan 复用同一 manager；native/empty/OCR、
fallback、按需加载、P4 窗口合批（8/16 分层+机制 A+自一致）、退化阶梯（2048 网格/penalty 链/切栏/纯截断重试）、
X1 跨文档（FIFO/一次加载/scan-summary/退出码聚合）、StepProfile/CommitProfile 黄金串、复读退化、GPU 回退、
取消、失败继续、续扫与合并均通过测试；v6 checkpoint 完整身份 + v5 安全迁移；CLI/页产物/doctor/result/质量
报告契约兼容（golden 逐字节复验）；§9.1 门禁除 Python 既有漂移外全部通过，未以删除/放宽既有测试换取通过。

## 剩余工作（下一步清单）

1. **T052–T054/T056–T058/T065–T068**：ScanApplication 主体（lib.rs ScanRequest/ScanOutcome/ScanEvent；
   输入预检 port main.rs parse_page_selection/validate_numeric_options/resolve_output_dir/
   validate_scan_output_policy；页面计划用 ocrx_pdf::classify_pages + state 队列；native-first（port
   main.rs run_native_first_phase 语义，native 页经 output::commit_native_page_v6）；渲染器迁移（main.rs
   PdfWorkerScheduler/WorkerScanRenderer 1564–1856 行 + pdfium_owner.rs 整文件 → scan/render.rs，实现
   window::PageRenderer）；resume 恢复 pending 队列；document.md 合并（output::collect_page_markdowns/
   merge_markdown 已 pub）；routing sidecar + result 字段（port main.rs serialize_scan_result 4609–4683
   行，model 对象从 InferenceIdentity.display 派生，runtime=llama.cpp@0.1.161）；telemetry 映射；
   prepare 失败 pending 保留。
2. **T063 跨文档**：multi_doc.rs——文档 FIFO、共享 session 一次 prepare、scan-summary.json
   （schema_version 1，main.rs 4255 形态）、退出码聚合 130>3>10>0、`<stem>-<hash8>` 目录。
3. **T069–T078（阶段 G）**：CLI compose.rs 组合根（顺序见 §6.1/RBLK-9）；main.rs 切 model/doctor/scan
   命令到新 manager/device/ScanApplication；保留 golden；T077 删旧依赖。
4. **T079–T085（阶段 H）**：ocrx-eval crate（pdf 质量模块复制 + 双入口 + 同参重扫原语）。
5. **T086–T105（阶段 I）**：删除兼容层（core ModelId/RUNTIME_COMPATIBILITY、models 旧 API、runtime 旧
   Runtime/fake 包装、ovisocr2 旧业务、output 旧 ScanService/CompatibilityKey、CLI 旧编排）、
   validate-architecture-boundaries.py、bootstrap-check.sh、全门禁。
6. 计划文档勾选同步（T048/T049/T050/T051/T055/T059–T064 已标；T063 待全部窗口/跨文档测试后标）。

## 关键技术备忘（续）

- window loop 的 resume 分支：`resume_side(compatibility)` 从 OCR 身份派生 ScanOcrSide；statik=None（loop
  在 prepare 后跑，终比较用 session identity）。
- FakeSession 模式：Rc<Core> 共享核心 + Clone handle；image id = `page-{N}` 作为 fake 页标识。
- 临时目录 tag 必须按测试唯一（并行竞争教训）。
- adapter decode 警告：nonempty>0 → warnings=["think_removed"]；scan assemble 用它复现 ThinkRemoved
  页面警告（page_json golden 关键）。
- scan 不得依赖 ocrx-runtime/ocrx-models/ocrx-inference/ocrx-ovisocr2（T089 架构脚本会查）。


## 断点续接锚点（2026-11-12，阶段 F 中段）

已完成并提交：T001–T057 中的 T001–T055、T059–T064（勾选状态见计划文档，`grep "^- \[x\]" 统计）。
T053 已提交（plan.rs 纯校验移植，56 scan 测试）。workspace 全绿。

**下一个任务（按序）：**
1. T052：scan/lib.rs 的 ScanApplication/ScanRequest/ScanOutcome/ScanEvent 骨架（库入口不打印/不退出/不装
   signal handler）。
2. T056：native-first（port main.rs run_native_first_phase/route_native_validation 语义 ~900-990 行 +
   native 提交经 ocrx_output::commit_native_page_v6）；T057 最终 OCR 队列合并 + 按需 prepare（队列 0 时
   不 prepare——用 static_identity 预比较即可完全复用）。
3. T058：渲染器迁移（main.rs 1564-1926：SegmentRenderer/PdfWorkerScheduler/WorkerScanRenderer/
   parse_dimensions/linear_scale_ratio/gcd + pdfium_owner.rs 整文件 1101 行 → scan/render.rs；实现
   window::PageRenderer；SwitchDocument 协议在 pdfium_owner）。
4. T065/T066/T067/T068：合并汇总（output::collect_page_markdowns/merge_markdown 已 pub）、
   routing sidecar（port main.rs write_routing_checkpoint_sidecars 755-800 行）、result 字段（port
   serialize_scan_result 4609-4683，model 从 InferenceIdentity.display，runtime=llama.cpp@0.1.161）、
   StepProfile/CommitProfile 接线、prepare 失败 pending。
5. T063 跨文档：multi_doc.rs（port main.rs run_scan_multi 4267-4509 + DocScanEnv/ModelSlot 语义改为
   共享 session；scan_summary_json 4190-4266；aggregate_scan_exit 4204）。
6. 阶段 G/H/I 按实施日志前节清单。

**重要：** main.rs 的对应函数在 T074/T077 前不删；port 时用 python 脚本或 Write 工具（heredoc 会被长
内容截断）。scan crate 依赖现况：core/primitives/inference-api/pdf/output/image。测试必须每个临时目录
唯一 tag。每次 commit 前 `cargo test --workspace --locked`。


## 断点续接锚点 2（2026-11-12，阶段 G 起点）

**已完成并提交：T001–T069**（61 项勾选）。workspace 全绿（37 个测试目标）；scan 80 测试；
新 crate clippy `-D warnings` 干净。阶段 F 全部完成：ScanApplication 端到端管线（native-only 零
prepare、auto 回退一次 prepare、多输入共享 session、prepare 失败 pending 保留、T1 逐文档遥测增量、
CommitRecorder sink 全路径接线）。

**T069 已落地：**
- `src/ocrx-cli/src/compose.rs`：§6.1 顺序组合根（catalog → probe/devices+llama factory → registry →
  RegistryVerifier → ModelManagerImpl(注入 verifier) → InferenceServiceImpl → ScanApplication），
  测试证明单模型 list/doctor 概览/static_identity 解析。
- `ocrx-scan`：`OwnerPdfAccess`（PdfDocumentAccess over PdfiumOwner 通道；`renderer(recorder)` 把同一
  owner 交给窗口循环；`switch_document` 重建 owner）。PdfiumOwnerHandle/OwnerPageRenderer::new 已 pub。
- CLI 新依赖：ocrx-inference / ocrx-inference-api / ocrx-scan。scan 导出
  ScanApplication/DocumentOptions/DocumentOutcome/ScanEvent/ScanOutcome/PdfDocumentAccess 等。

**T070–T078 执行要点（下一步）：**
1. **T070**：拆 args.rs/config.rs/interaction.rs/present.rs（main.rs 的 parse_scan_args/resolve_config/
   format_cli_result/print_reporter 等移动；`pub(crate)` 互引保持编译）。
2. **T071/T072**：`model list/info/recommend` → `composed.inference.manager().list()/info()/recommend()`
   映射到现有输出串（golden：`model=ovisocr2` / `model=ovisocr2 profiles=… installed=`）；
   `model install/verify/remove` → manager().install/verify/remove（verify 经注入 RegistryVerifier，
   T072 专项：资产对但 registry 身份不匹配 → IncompatibleBinding）。
3. **T073**：doctor → DeviceManagerImpl::diagnose() + manager 概览（schema 2 golden 字段序不变，
   runtime backend 首字母大写 Cpu 来自 BackendId→展示映射）。
4. **T074/T075**：scan 命令 → ScanApplication::run（OwnerPdfAccess::spawn(pdfium_dir, input,
   RenderOptions{dpi}, password, window_depth*2) + renderer(commit_recorder) + compose 的 service；
   serialize_scan_result 从 DocumentOutcome+InferenceIdentity.display 派生，runtime=llama.cpp@0.1.161，
   多输入 scan-summary.json + 退出码聚合 130>3>10>0）。
5. **T076**：install_sigint_handler 保留（ctrlc crate 已在 CLI deps）。
6. **T077**：删除 CLI 对 runtime/models/ovisocr2/pdf 的直接依赖与 run_scan/run_scan_multi/
   OvisRecognizer/PreloadedLoader/WorkerScanRenderer/pdfium_owner（CLI 自己的拷贝）等旧代码。
7. **T078**：全部 CLI 子进程测试（cli_subprocess.rs 264 行 + multi_input_contract.rs 206 行）。

**关键事实：**
- scan crate 的 `run_pages_with_session` 第 6 参是 `&CommitRecorder`；app `run()` 第 7 参同。
- CLI 的 pdfium dll 目录解析：`platform_paths`/`OCRX_PDFIUM_LIBRARY` 链路在 main.rs ~2126 与
  pdf.rs 2553-2620（`vendor/pdfium/bin` 默认）。
- 现有 CLI golden（T003 表）在基线文档；doctor json `recommended_profile:"quality"`（63GB→quality）。
- 测试：每个临时目录唯一 tag；`cargo test --workspace --locked` 必须全绿后 commit。
- 计划勾选状态：`grep -c "^- \[x\]"` 当前 61。


## 断点续接锚点 3（2026-11-12，阶段 G 中段）

**已完成并提交：T001–T073**（67 项勾选）。workspace 全绿（37 测试目标）；CLI bin 107 测试；
`cargo fmt --all -- --check` 通过；doctor/model 命令 golden 在本机二进制上逐字节复验（T003 表）。

**阶段 G 已完成项：**
- T069 compose.rs（见锚点 2）。
- T071/T072：全部六个 model 命令切换到 compose 的 ModelManager（list/info/recommend/install/verify/
  remove），输出串与退出码逐字节保持（4/5/9 错误串一致）；verify 走注入的 RegistryVerifier；
  install 走组事务（TrustedInstallInteraction 保持冻结的非交互策略）；remove 走共享引用检查。
- T073：doctor 切换到 DeviceManager DTO 形态——`doctor_diagnostics_from_snapshot()` 从已交集的
  snapshot 派生 DTO（formatter 测试注入人工 snapshot 时 JSON 必须镜像之，golden 保持权威）；
  installed 概览经 v2 manifest 的 verify::is_installed；schema 2 字段序/转义/skill_installed 合并
  逐字节一致。

**T070/T074–T078 执行要点（下一步，按序）：**
1. **T074/T075（核心）**：scan 命令切到 ScanApplication::run：
   - 组装：`OwnerPdfAccess::spawn(pdfium_dir, input, RenderOptions{dpi}, password, window_depth*2)`
     （pdfium_dir 解析链在 main.rs ~2126 + pdf.rs 2553-2620，默认 `vendor/pdfium/bin`）；
     `access.renderer(commit_recorder)` 给窗口循环；service 来自 compose（多文档共享同一 service/session）。
   - DocumentOptions 从 ScanOptions 映射（pdf_mode/quality_tier/dpi 默认 300/budgets/resume/keep_pages/
     column_split_retry/pages/requested_profile+device/install policy 从 offline+non_interactive）。
   - output_for：单输入 `resolve_output_dir`（历史形态）；多输入 `output_dir_for_doc`（`<stem>-<hash8>`）。
   - serialize_scan_result 从 DocumentOutcome 派生：model 对象字段经 InferenceIdentity（display 通道的
     name/family/quantization；runtime=`llama.cpp@0.1.161`；prompt_schema_version=数值 1）；失败分支
     （DocumentFailure→错误对象+退出码）；多输入 scan-summary.json（main.rs scan_summary_json 形态）+
     aggregate_scan_exit（130>3>10>0）；`--report json` 单输入 stdout 一个聚合对象。
   - 保留 install_sigint_handler（T076）。
   - 验收：T003/T004 golden + cli_subprocess.rs + multi_input_contract.rs 全部通过；fake 扫描端到端。
2. **T077**：删除 CLI 对 runtime/models/ovisocr2/pdf 的直接依赖（Cargo.toml + 源码 import）与
   run_scan/run_scan_multi/scan_document/DocScanEnv/ModelSlot/OvisRecognizer/PreloadedLoader/
   load_runtime_with_fallback/WorkerScanRenderer/SegmentRenderer/cli 自带的 pdfium_owner.rs/system_probe.rs；
   注意 scan crate 已有这些的迁移版。每删一批跑 `cargo check --workspace`。
3. **T070**（可与 T077 合并做）：main.rs 拆 args.rs/config.rs/interaction.rs/present.rs/platform_paths.rs。
4. **T078**：全部 CLI 子进程测试 + 编排 vs 逐个调用一致。

**注意：** Windows link.exe LNK1104 偶发（exe 被占用）——删 target/debug 下对应 exe 重跑即可，非代码问题。

## 评审响应（2026-11-13）：修复代码评审 BLOCK 的 9 项阻断

评审报告：`docs/review/model-backend-extensibility-refactoring-code-review.md`（范围 main@2f6f637...c8ce717）。
全部 9 项已按"先判别测试、后修复"处理，逐项落点与证据：

- **BLK-1（整页 PNG 零字节）**：window.rs 提交端不再 `unwrap_or_default()`——预编码 artifact 字节
  非空才用，否则同步 `encode_page_png(image)`（RgbImage→PNG，空输出报错）；编码失败是页级失败而非
  空文件。测试 `keep_pages_true_keeps_full_page_pngs_non_empty_and_decodable` 断言非空+可解码+尺寸。
- **BLK-2（终比较失败页丢失）**：app.rs demotion 块重建 `decode_pages = service_pages_from_ocr_queue(&final_queue)`
  并以 `reuse_stats: HashMap<u32,(u32,bool,Vec<String>)>` 回滚逐页复用统计。测试
  `final_compare_mismatch_reprocesses_reused_and_pending_pages_blk2`（TwoPageOcrPdf，批深 8→16，
  断言两页均按新身份完成、checkpoint batch_depth=16、文档无缺页）。
- **BLK-3（--max-output-tokens 不参与推理）**：WindowConfig 增加 `max_output_tokens`，
  SessionRecognizer::new 接收并下传全部单页/窗口/重试请求（替换硬编码 DEFAULT_MAX_OUTPUT_TOKENS）。
  测试 `max_output_tokens_rides_every_request_blk3` 捕获请求断言 128。
- **BLK-4（越界 --pages 静默过滤）**：app.rs 在过滤前校验原始 selection（越界→usage 2）；CLI 子进程
  测试 `cli_out_of_range_pages_is_usage_error_blk4` 用真实二进制+vendored pdfium 断言 exit 2 与错误对象。
  修复过程中发现并修正第二处回归：单输入退出码此前被 aggregate_scan_exit 折叠为 3——现按 X1.4 冻结
  语义恢复"单输入保留自身结构化错误码（2/3/5/9/10/130），多输入才聚合 130>3>10>0"。
- **BLK-5（keep_pages 条件写反）**：window.rs 改回 `if !config.keep_pages` 清理；测试覆盖开关两态
  （默认清理整页 PNG 保留 figure crop；--keep-pages 保留整页且非空）。
- **BLK-6（校验/写入哈希不一致）**：checkpoint_v6.rs `artifacts_intact` 改用与写入端相同的
  `artifact_sha256`（md+全部引用图片），并新增页面 JSON 存在性与 raw sidecar 指针校验；
  ocrx-output/src/lib.rs 将 `artifact_sha256` 置 pub。判别测试：完好含图页可复用、图片缺失/篡改不可复用、
  JSON 缺失不可复用、md-only 摘要不等于 artifact 摘要。resume.rs/CLI 测试种子同步改用该算法。
- **BLK-7（页级失败退出码 0/3）**：main.rs 新增 `document_exit_code`（页级失败→10，文档失败→自身码，
  取消→130，干净→0），多输入经 `aggregate_scan_exit` 聚合；单输入保留文档自身码（见 BLK-4）。
  单测覆盖页级失败/截断/退化→10、文档失败→3、取消→130 与聚合阶梯。
- **BLK-8（T1 插桩不写盘）**：app.rs 成功且未取消时逐文档写出 `step-profile.json`（step_profile_delta）
  与 `commit-profile.json`（recorder.enabled()），finalize 计时入 recorder；run 循环每文档
  `commit_recorder.reset()` 隔离跨文档串数；disabled 保持 no-op、取消/失败零写盘。
  测试 `t1_json_files_written_on_success_and_absent_on_disabled_blk8` 断言两文件存在/缺席。
- **BLK-9（零解码文档借用共享身份）**：app.rs 以 `used_session = has_pending_decode` 生成本文档身份与
  遥测/step-profile delta——零解码文档 identity=None、无 session delta，与单跑一致（RBLK-8）。
  测试 `zero_decode_followup_document_reports_no_identity_blk9`。

**门禁复跑（修复后）：** cargo fmt ✓；`cargo clippy --workspace --all-targets --locked -- -D warnings` ✓
（修正 2 处新测试 lint：cloned_ref_to_slice_refs、IIFE-let-else）；`cargo test --workspace --locked`
37 个套件全过 0 失败（scan lib 85、output 108、CLI bin 55）；validate-architecture-boundaries.py ✓；
test-release-scripts.sh ✓；bootstrap-check.sh 中除 4 个既有 Python 漂移失败（T103 注记，主工作区同现）
外全部通过。清理：删除误跟踪的 wt.log（T104 无日志规则）、恢复被评审复现覆盖的
benchmarks/fixtures/pdf/normal-1-page-ocr/{document.md,result.json}、去除 ocrx-eval/Cargo.toml EOF 空行
（`git diff --check` 干净）。

## 重审响应（2026-11-13）：修复重审 BLOCK 的 4 项（RBLK-1..4）

重审（`4e65fe2`）：BLK-1/2/3/4/5/9 解除；RBLK-1..4 未闭环/新增。逐项：

- **RBLK-1（页面 JSON 篡改仍判完整）**：根因是重构版 `artifact_sha256` 偏离了冻结基线——基线把
  页面 JSON 作为必需成员并把图片依赖从 JSON 的 `visual_region` 结构块派生（markdown 里的
  `images/` 字符串只是散文）。已从 main 逐字恢复该算法（含 `create_contained_child_dir` 回到
  ocrx-core），写入端（commit_page/commit_native_page 先写 JSON 再算摘要）与校验端同源。
  判别测试修正：篡改 JSON（去块/改字段）与缺失 JSON 均不可复用；种子 JSON 带 1 个 visual_region
  块使图片真正参与摘要。顺带修复 1 处测试 mock JSON 恒为非法的潜伏 bug（旧摘要不解析故未暴露）。
- **RBLK-2（T1 四类违约）**：(1) `LlamaBackend::step_profile` 在 backend 边界按
  `StepProfile::env_enabled()` 返回 None（可测的 `gate_step_profile` 助手 + 测试）；
  (2) 写盘条件补 `summary.failed_pages == 0`；(3) 两处 `atomic_write` 失败降级为
  warning 行（"warning: step-profile.json write failed: …"，照基线字符串）不再中断扫描；
  (4) `commit_recorder.reset()` 在 run_document 开头逐文档隔离、`record_finalize` 在
  finalize_document 前后计时接线。测试：页级失败零写盘（有 delta+开启 recorder 也不写）、
  step-profile.json 被目录占用时扫描仍成功且 commit-profile 照写、双文档同页码
  `"page":1,"generation":1` 各恰一行 + `finalize_us` 非零。原 blk8 测试的第二次 run 原本
  依赖"失败也写 step-profile"的 bug（fake 队列已耗尽），补第二个 program 使其为真成功路径。
- **RBLK-3（取消被掩盖/折叠为 0）**：`document_exit_code` 取消检查前移（文档失败+取消→130）；
  新增 `final_scan_exit(cancelled, multi, codes)` 终出口——取消优先于一切（零文档启动的取消
  codes=[] 不再折叠为 0），多输入走 X1.4 聚合，单输入保留自身码。测试覆盖 failure+cancel→130、
  summary.cancelled+failure→130、零文档取消→130（单/多输入）、非取消各档。
- **RBLK-4（PNG 编码失败中断整扫描）**：window.rs `commit_terminal` 的 `encode_page_png(image)?`
  改为页级失败分支（failed+=1 + mark_terminal + recognize 诊断 + continue），与老 ScanService 的
  冻结面（encode_error_diagnostic）一致。回归测试直接驱动 commit_terminal：坏 rgb8 页失败、
  后续页照常提交完整产物集（md/json/真实 PNG 可解码），函数整体仍返回 Ok。

**门禁复跑：** fmt ✓；clippy `-D warnings` ✓（补 artifact_sha256 的 # Panics 文档）；
`cargo test --workspace --locked` 37 套件全过 0 失败（scan 89、output 108、CLI bin 56、
runtime 45）；architecture ✓；release-scripts ✓；`git diff --check` 干净；bootstrap-check.sh
仅余 4 个既有 Python 漂移失败（T103 注记，主工作区同现）。

## 评审闭环（2026-11-13）：第二轮重审 PASS

第二轮重审（`d79ecf6`）结论 **PASS**：RBLK-1～4 均确认修复，未发现新增合并阻断
（报告：`docs/review/model-backend-extensibility-refactoring-code-review.md`）。评审复核要点：
基线摘要算法（md+页 JSON+visual_region 图片）写入/校验同源；T1 环境门控/失败零写盘/写失败
降级/逐文档 reset+finalize 全部接线并有判别测试；取消 130 在文档映射与终出口均为最高优先
（含零文档启动）；PNG 编码失败为页级失败不中断扫描。评审验证：工作区测试、严格 Clippy、
格式、架构边界、release 脚本、`git diff --check`、真实 CLI 越界页码（exit 2）全部通过。

**验证边界（评审明示）：** 未运行真实模型/GPU/环境门控 E2E——PASS 表示代码审查及上述验证
范围，不扩大为环境证据。该边界与 T106/T107（可选、需 OCRX_E2E_MODEL+真实资产）注记一致。

分支状态：`refactor/model-backend-extensibility`，评审链 c8ce717（初评 BLOCK，9 项）→
4e65fe2（修复 9 项）→ d79ecf6（闭环 4 项）→ PASS。计划任务 102/108 完成（T103/T105 既有
环境漂移注记；T106/T107 可选 E2E；T108 后续独立立项），合并待用户决定。

## main 合并（2026-11-13）：同步上游安全加固与协议更新（2f6f637..5e995d0）

`git merge main`（PR #4 CI 门禁 / #5 上游致谢 / #6 输出与 owner 防护 / #7 管线安全加固）。冲突
7 处全部语义化解决（保留重构形态 + 移植上游加固），自动合入 ocrx-pdf/llama_mtmd/multi_sequence_spike
及经重命名检测的 scan 侧 pdfium_owner（断连 seam）。逐项：

- **ocrx-core**：取上游硬化版 `atomic_write`（`create_new` 防临时路径符号链接 + `sync_all` +
  碰撞重试）与 `create_contained_child_dir`（RBLK-1 时我加的重复副本删除）；上游把手写 JSON
  字符串解析换成 serde_json 结构化解析（`parse_block`/`block_string`/`block_u32`）自动合入；
  上游新增的旧版 CancellationToken 丢弃（primitives re-export 保持）。
- **ocrx-output**：`atomic_write` 委托 core 硬化版、`validate_output_root` 在规范化前拒绝父目录
  穿越、`commit_page` 走受控 images 目录、`is_reusable_at` 显式检查页面 JSON——全部自动落位；
  `artifact_sha256` 两侧同文（RBLK-1 时即取自上游），保留我们的 pub + 文档注释；测试 mock JSON
  采用上游 `serde_json::json!` 写法。
- **ocrx-models**：上游对旧管线的安装事务加固移植进重构版 `install.rs`——补上了重构时丢失的
  revision 级 `InstallLock`（create_new 原子检测 + 死进程锁回收，含 Windows tasklist CSV 探活），
  在任何磁盘变更前获取；新增 `InferenceError::InstallLocked`（kind `install_locked`）；判别测试：
  持锁并发安装被拒且活跃事务文件逐字节无损、不触网。上游 URL 方案拒绝已由 transport 层 https
  守卫覆盖。
- **ocrx-inference-api**：`DECODE_GENERATION` 2→3（上游 token 分片按字节累积后再 UTF-8 解码；
  llama_mtmd 相关改动自动合入），常量单一来源在 inference-api，runtime 桥接保留。
- **ocrx-cli**（取我方骨架 + 移植四段上游语义）：① pdfium 解析 env→可执行文件同目录捆绑
  （Windows pdfium.dll / macOS ../lib/libpdfium.dylib / Linux ../lib/libpdfium.so）→"."，含 doctor
  布局测试；② 生产路由版本升 `pdf-route-v2/pdf-text-v2`（scan app 侧，旧 checkpoint 按设计失配
  重扫）；③ 多输入在 owner 启动前校验聚合输出根 + scan-summary.json 存在需 --resume/--overwrite
  （exit 9）；④ X1.2 owner fail-loud——`OwnerPdfAccess::poisoned()` 公开 + CLI 在调用级以
  "scan aborted: pdfium owner failed"（exit 3）中止，永不降级为逐文档跳过。
- **协议对齐**：`PdfDocumentAccess` 增加 `begin_document`（每文档边界一次，含首文档——上游的
  switch-at-every-boundary 协议），OwnerPdfAccess 覆写为无条件 switch，断连 seam 测试由此按
  预期触发毒化。
- **上游新契约的跟进修正**：`PageSignals.non_text_objects`（空页判定需 image+non-text 全零）
  补 scan 测试字面量；`validate_output_root` 拒绝原始 `..` 后，CLI 测试的
  `CARGO_MANIFEST_DIR/../..` fixture 拼写全部改 canonicalize（blk4/cli_subprocess）；resume 种子
  兼容键同步 v2+decode 3；compose 断言 generation 3。
- **架构白名单**：`ocrx-skill → ocrx-core` 放行（上游加固复用受控目录/原子写原语）。

**门禁复跑（合并后）：** `cargo test --workspace --locked` 37 套件全过 0 失败（含上游新增
multi_input_contract 契约测试与断连 seam 测试）；fmt/clippy `-D warnings` ✓；架构校验 ✓；
release-scripts ✓；`git diff --check` 干净；bootstrap-check.sh 仅余 4 个既有 Python 漂移失败。
（一次 LNK1104 偶发按备忘删 exe 重跑即过。）route v2 + decode 3 的组合意味着 main 时代与更早
的 checkpoint 不再复用——与上游"版本升级即重扫"的兼容键语义一致。

## 评审闭环（2026-11-13）：合并 main 后重审 PASS

第三轮评审（`2509dd4`，范围 d79ecf6..2509dd4）结论 **PASS**：合并冲突处理未造成既有修复回退，
上游安全加固已接入重构架构，无新增合并阻断。评审复核要点：九项原始修复全部保持；core 统一
atomic_write、受控 images 目录、原始 `..` 拒绝、聚合输出根/scan-summary 保护接入；revision 级
InstallLock 在事务变更前获取；DECODE_GENERATION=3 单源 + 路由 v2；owner 每文档边界 switch +
CLI fail-loud；`ocrx-skill → ocrx-core` 白名单一致。评审验证全绿（工作区测试含上游新增契约/
断连测试、严格 Clippy、格式、架构、release 脚本、git diff --check、真实 CLI 越界页码 exit 2）。

**验证边界（评审明示）：** 未运行真实模型/GPU/环境门控 E2E——与 T106/T107 可选注记一致。

**最终状态：** 评审链 c8ce717（BLOCK 9 项）→ 4e65fe2（修 9 项）→ d79ecf6（闭环 4 项）→ PASS →
2509dd4（合并 main@5e995d0 + 安全加固移植）→ PASS。计划任务 102/108 完成（T103/T105 既有环境
漂移注记；T106/T107 可选 E2E；T108 后续独立立项）。分支 `refactor/model-backend-extensibility`
包含完整重构 + 上游全部安全加固，可合并回 main。

## T106/T107 真实验证（2026-11-13，本机 RTX 4090 D 24GB + CUDA 12.4 + compact 资产）

### T106：真实 Ovis 故障注入 + 多页冒烟 + 取消/续扫 —— 全部通过（并修复 2 个真实缺陷）

- **故障注入（2/2）**：`OCRX_E2E_MODEL=1 real_scan_fault_injection --ignored`——render 注入
  partial/10/stage=render ✓；recognize 注入发现**接缝回归**：`OCRX_E2E_INJECT_INFERENCE_FAIL_PAGE`
  在重构后从未被生产路径读取（旧 CLI 的 inject_inference_failure 成了死代码）。修复：缝接到窗口
  组装处（注入页在 decode 前转 recognize 终态，形状与真实识别失败一致，同级页继续，机制 A 保持）。
- **真实模型编排（9/9）**：`multi_doc_orchestration --ignored` 首次真跑（基线注记该 9 项重构前
  从未执行）。发现并修复：(a) prepare 的 "[model] resolved and verified" 逐调用诊断行在重构版
  丢失——移植回 ocrx-inference prepare（OCRX_STEP_PROFILE 门控）；(b) fallback_auto_resume 的
  能力跳过门只查 stderr——exit 6 在单输入 --report json 下走 stdout（冻结路由），门改为双流探测。
- **多页冒烟**：8 页 figure 文档（release CPU）20/20 页级完成（含 20 页文档）；CPU/CUDA 产物
  逐字节一致（page-0001/0004/0008 md IDENTICAL）。
- **取消+续扫**：windows-ctrl-c-smoke.ps1 真实 CTRL_BREAK（脚本首次按 CPU-only 二进制跑通——
  原脚本硬编码 --device vulkan + 16 页锚点，已参数化 -Device 推导锚点）：**cancelled=130、窗口 1
  检查点保留且复用（mtime 未变）、机制 A 零提交、无 .part 残留、--resume 完成（code 10）**。
- **续扫复用修复（真跑暴露的最重缺陷）**：真实 20 页 `--resume` 曾 20/20 全量重解码。根因是
  两处 post-prepare 分类的 `ResumeContext { statik: None }`——OCR 候选在 phase-1 门即被拒
  （"no static identity for OCR pre-compare"），预分类的 reuse 全部被推翻：(a) app 层
  final-compare 重分类（权威 RBLK-6 门）；(b) window.rs 窗口组装分类。假模型测试未覆盖
  "有 pending 解码 + 候选复用"组合（新增回归测试 resume_reuses_candidate_pages_even_with_pending_
  decode，stash 验证无修复必失败）。修复后真实证据：干净截断页 2/4/6/8 **checkpoint mtime 未变、
  零提交行**（复用），退化页 1/3/5/7 按阶梯带新预算重扫（§3.5），9-20 新解码，20/20 完成。

### T107：CPU/GPU/资源/回退观察（单一硬件，不外推）

| 项 | CPU（release，8 页 figure） | CUDA（同文档） |
|---|---|---|
| 墙钟 | 75.2 s（scan 内 73.2 s） | **7.2 s**（scan 内 5.2 s，≈10.4×） |
| 峰值 RSS | 3457 MB | 1549 MB（解码移至 VRAM） |
| backend | cpu | cuda |
| 产物 | 同参逐字节一致（抽 3 页 cmp IDENTICAL） | 同左 |

- **CUDA 构建/打包**：`package-windows-x64.ps1 -GpuPackages cuda`（vcvars64 + VS Ninja，
  19m37s）；包内 ocrx.exe（CPU）+ ocrx-cuda.exe；安装器在本机把 cuda 变体提升为默认 ocrx.exe。
- **GPU→CPU 回退（真实观察）**：发现 `OCRX_E2E_FORCE_RUNTIME_FALLBACK` 是死缝（仅测试引用，
  生产未读；相关门控测试在 CPU-only 二进制上经能力跳过门空过）。已在 ocrx-inference prepare 的
  GPU 加载尝试点接线（上游 main:1991 语义：注入在失败点，走真实 Err→CPU 加载→警告→身份重建→
  深度降档分支）。真实观察：`--device cuda` + 缝 → **backend=cpu、扫描成功、checkpoint
  inference.backend=cpu / batch_depth=8（16→8 降档）/ decode_generation=3**。补充观察：
  `--device auto` 正常选 cuda；显式 `--device vulkan`（无 vulkan 构建）→ exit 6 不可用（符合契约）。
  CUDA_VISIBLE_DEVICES="" 不影响驱动 API 枚举（backend 仍 cuda）——记录为枚举语义观察，不作回退证据。

**门禁（T106/T107 修复后）**：37 套件全过 0 失败（scan 90）、fmt/clippy -D warnings ✓。安装包
重建并重装（SHA256 5c0fc194…，含全部修复 + cuda 变体）。计划勾选更新：T104/T105（实质完成有
证据）/T106/T107 完成；T103 保持未勾——4 个既有 Python 漂移失败属 pdf-native 管线的冻结 fixture
断言（主工作区同现），修复越出本重构授权，已按 T105 语义记录环境原因。
