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
