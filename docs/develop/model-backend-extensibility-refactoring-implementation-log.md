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
