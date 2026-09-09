# PDF Content-Aware v0.2 本机执行清单

> 本清单只覆盖当前机器可以执行、验证或生成证据的工作。人工视觉验收不计入自动完成；所有状态均以本文件为准。
>
> 状态标记：`-[ ]` 未完成；`-[~]` 部分完成/进行中；`-[x]` 已完成。

## 1. 真实 OCR 模型资产

- [x] 下载 OvisOCR2 四个 profile：`compact`、`balanced`、`quality`、`reference`。
  - 资产目录：`.ocrx-model-cache/ovisocr2/`
  - revision：`8a22290a0c42dbcf84739d6d4c2763f877494ae0`
- [x] 对四个 profile 执行离线安装和 SHA-256 校验。
- [x] 用真实 CLI 完成至少一页 OCR smoke test。
  - 要求：模型和 projector 实际加载，输出 `ocr_model_loaded=true`，页面成功完成。
  - 已验证：CPU 主机权限路径成功。
- [x] 记录真实模型资产的机器环境、文件大小、哈希、runtime 和 backend 信息到报告。
  - `assets/compatibility/ovisocr2-macos-arm64-runtime-report-2026-09-05.json`

## 2. 30 页 native 合格集自动证据

- [x] 固定 30 页 native candidate 输入、页码和 source PDF hash。
- [x] 固定 30 页 truth 文件，包含逻辑文字、页面类型、block、bbox、阅读顺序和来源字段。
- [x] 固定 30 页 preview，便于人工验收和后续复核。
- [x] 生成 PDFium native route evidence，证明候选页具备原生文字证据。
- [x] 使用真实 CLI 的 `--pdf-mode auto` 对 30 页执行 native-only E2E。
  - 要求：30/30 `native_text`，0 OCR，0 fallback，0 failure，模型不加载。
- [x] 执行 native 质量评分。
  - 指标：text NED、reading order、block precision/recall/F1、bbox validity/recall、duplicate、blank/fail。
- [x] 完成 native `auto` 冷启动性能测量至少 5 次。
  - 已保存：`assets/regression/pdf-native-text-candidate-v0.1/reports/stage6-auto-native-performance-2026-09-04.json`
- [x] 复核 native 性能报告是否包含峰值 RSS、输出大小、模型读取/初始化证据等正式发布字段。
  - `assets/regression/pdf-native-text-candidate-v0.1/reports/stage6-auto-native-performance-rss-2026-09-05.json`

## 3. 强制 OCR 质量对照

- [x] 用同一 30 页、同一 release CLI 执行：
  ```bash
  --pdf-mode ocr --model-profile quality
  ```
- [x] 生成 30 页 OCR prediction artifacts、page JSON、Markdown 和 result JSON。
  - 已修复 `--pdf-mode ocr` 同时进入 native first phase 导致重复页码的问题。
  - 30 页 page artifacts 和最终 `document.md` 已通过真实 CLI 复跑验证。
- [x] 对 OCR prediction 使用正式评分器执行质量评分。
  - 有效 CPU-only 强制 OCR 结果：30/30 完成，text NED=0.081038，reading order=1.0，blank=0，duplicate=0；block/bbox 对 OCR 按 N/A 处理。
- 指标必须与 native 评分一致：text NED、reading order、block precision/recall/F1、bbox、duplicate、blank/fail。
  - 当前已完成 text/blank/duplicate 评分；结果：text NED≈0.08083、blank=0、duplicate=0。
  - OCR 原始 PageDocument 不携带 native provenance，block/bbox 需补充兼容评分口径或报告为不适用。
  - 已在对照报告中将 block/bbox 明确记为 N/A，而不是误报为 0。
- [x] 至少完成 5 次同机冷启动强制 OCR 性能测量。
  - 已记录 5 次绝对耗时、中位数、成功页数、模型 profile/runtime/backend 和模型加载证据。
  - 已补充峰值 RSS 与输出大小：见 `assets/regression/pdf-native-text-candidate-v0.1/reports/stage6-quality-ocr-performance-rss-2026-09-05.json`。
- [x] 完成 native 与 quality OCR 的相对质量比较。
  - native text NED 不得比 OCR 对照退化超过计划阈值。
  - native reading order 不得比 OCR 对照退化超过计划阈值。
- [x] 完成 native `auto` 与 quality OCR 的性能比较。
  - native `auto` 中位数必须满足计划规定的相对性能门槛。
  - 对照报告：`assets/regression/pdf-native-text-candidate-v0.1/reports/stage6-native-vs-quality-ocr-comparison-2026-09-05.json`；native 中位 5 秒，quality OCR CPU-only 中位 546 秒，wall ratio≈0.00916。

## 4. 真实混合 PDF E2E

- [x] 准备一个包含 native text、image-only 以及 mixed/uncertain 页的混合 PDF。
  - fixture：`assets/regression/pdf-mixed-e2e-v0.1/inputs/native-image-mixed-3-pages.pdf`；已记录 source 和 SHA-256。
- [x] 执行混合 PDF 的 `--pdf-mode auto`。
- [x] 验证逐页分类、计划方式和实际方式：
  - native text 页 → `native_text`
  - image-only 页 → `ocr`
  - mixed/uncertain 页 → `ocr`
  - evidence：`assets/regression/pdf-mixed-e2e-v0.1/review/cli-auto-e2e-report.json`。
- [x] 验证模型只加载一次，且只为最终 OCR 队列初始化。
- [x] 验证 native 和 OCR 页面最终按原 PDF 页码合并到同一份 `document.md`。
- [x] 验证 `result.json` 的 routing、diagnostics、计数和模型字段完整准确。

## 5. 动态 fallback 真实路径

- [x] 已完成动态 fallback 的代码级队列、排序、去重和 checkpoint 测试。
- [x] 已完成模型准备顺序测试：先分析和 native 校验，后决定是否准备模型。
- [x] 已完成模型不可用时保留 native 成功页、标记 OCR 未完成页的测试。
- [x] 已将 PDFium 路由分析与 native 页面提取/验证解耦，确保两阶段不会因复用同一 `NativePage` 派生结果而遮蔽验证失败。
- [x] 构造一个真实 CLI 可复现的 native 校验失败页。
  - 真实 PDFium fixture：0.5pt 原生文字页，`text_bbox_coverage=0.000017`，分类仍为 `native_text`，L2 验证失败。
- [x] 执行“native 成功页 + native 校验失败页”的真实 E2E。
  - fixture：`assets/regression/pdf-dynamic-fallback-v0.1`；第 1 页 native 成功，第 2 页 native 验证失败。
- [x] 验证失败页进入最终 OCR 队列并由真实模型识别。
- [x] 验证模型只准备/加载一次，且 native 成功页不重复 OCR。
  - 真实 CLI 日志中模型加载元数据出现 1 次，结果为 `native_text=1, ocr=1, fallback=1`。
- [x] 验证 OCR 失败或模型不可用时：
  - 已提交 native 页保留；
  - fallback 页记录 `ocr_required_but_unavailable` 或等价稳定错误；
  - 任务状态为准确的 partial/failed；
  - 不伪装为 native 成功。
- [x] 验证中断后 `--resume` 可从兼容 pending OCR checkpoint 恢复，不重复 native 提取。
  - 模型恢复后仅处理第 2 页；第 1 页 checkpoint output hash 保持不变。

## 6. `--pdf-mode ocr` 兼容性回归

- [x] 保留旧版 OCR 行为相关单元测试和 output fixture。
- [x] 使用当前真实模型和 release CLI 生成当前 `--pdf-mode ocr` 基线样本。
  - CPU-only 构建下 30/30 成功，证据见 `assets/regression/pdf-native-text-candidate-v0.1/reports/pdf-mode-ocr-compatibility-2026-09-05.json`。
- [~] 与阶段 0 基线逐项比较：
  - 选页行为；
  - 输出文件；
  - checkpoint；
  - `result.json`；
  - 退出码；
  - 模型加载次数；
  - OCR 内容和结构。
- [x] 形成兼容性回归报告，并记录所有差异及其是否符合 v0.2 契约。

## 7. Backend/device 错误契约

- [x] 增加 backend 初始化错误的稳定分类单测。
- [x] CPU 模式主机权限真实 OCR 成功。
  - 使用 `GGML_METAL=OFF` CPU-only 构建，并修复 CPU 模型 `main_gpu=-1`；真实 CLI 30/30 强制 OCR 成功。
- [x] 将 runtime 原始 backend 错误完整贯通至 CLI JSON diagnostics。
  - 受限环境真实 CLI 的 `null reference from llama.cpp` 已稳定归类为 `device_unavailable`，不再误报为 `recognition_failed`。
- [x] 验证 `--device auto` 在 Metal 不可用时的行为。
  - 当前主机实测：选择 `backend=metal` 后返回稳定 `device_unavailable`；契约允许“回退 CPU 或输出稳定设备错误”，因此本机分支验证完成；成功回退 CPU 需具备可用 Metal 的对照环境。
- [x] 验证 `--device metal` 在 Metal 不可用时返回明确 `device_unavailable`，不输出泛化识别错误。
  - 真实单页 CLI：退出码 10，`error_codes=["device_unavailable"]`。

## 8. Stage 6 正式门禁

- [x] native formal manifest 已接入 validator、scorer 和 Stage 6 gate。
- [x] forbidden fixture 和 route evidence 已生成。
- [x] 使用完整 native candidate、forbidden fixture、OCR 对照和混合 PDF 运行最终 Stage 6 gate。
  - 结果已落盘：`assets/regression/pdf-native-text-candidate-v0.1/reports/stage6-gate-2026-09-05.json`；门禁结构通过，但发布状态仍被人工验收阻断。
- [~] 验证完整 native 门禁：
  - 100% 最终 `native_text`；
  - 0 fallback/OCR/failure；
  - 绝对质量指标全部通过。
- [x] 验证完整 forbidden 门禁：image-only 页全部不进入 native 合格路径。
- [x] 验证 OCR 质量对照门禁和相对阈值。
  - 有效 30 页对照已生成，native text NED/reading order 均未退化。
- [x] 验证性能门禁和 5 次重复测量门槛。
  - native 和 quality OCR 均已有 5 次成功重复测量；quality OCR 报告：`assets/regression/pdf-native-text-candidate-v0.1/reports/stage6-quality-ocr-performance-rss-2026-09-05.json`。
- [x] 验证纯文字任务无模型文件读取、下载或 runtime 初始化证据。

## 9. 发布报告与最终状态

- [x] 汇总固定集合版本、source PDF hash、truth 版本和 benchmark 版本。
- [x] 汇总所有执行命令、机器环境、PDFium revision、runtime、模型 profile 和 backend。
- [x] 汇总 native、OCR、mixed、fallback、forbidden 四类 E2E 结果。
  - dynamic fallback、模型不可用和 resume 证据见 `assets/regression/pdf-dynamic-fallback-v0.1/review/`。
- [x] 汇总性能原始值、中位数、峰值 RSS、输出大小和模型加载证据。
- [x] 汇总所有未通过项、失败原因和是否需要人工处理。
- [x] 明确人工验收状态；人工未验收前不得把 native candidate 标记为正式发布合格集。
  - 汇总报告：`assets/regression/pdf-native-text-candidate-v0.1/reports/local-execution-release-report-2026-09-05.md`。
- [x] 运行最终 workspace 门禁：
  ```bash
  cargo fmt --all -- --check
  cargo test --workspace
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  ```
  - 2026-09-05：三项命令均通过；PDFium 缺失动态库环境测试已修正为安全跳过。
- [~] 只有当计划阶段 0–6、需求 14.2、质量门禁 14.3 和本清单必需项全部通过后，才将 v0.2 标记为完成。
  - 本机可执行项已完成；最终 `[x]` 仍需人工验收 30 页 native truth 并确认 Stage 6 正式合格集。

## 当前优先级

1. 完成第 3 节：30 页 native candidate 的 quality OCR 对照和性能测量。
2. 完成第 4 节：真实 mixed PDF E2E。
3. 完成第 5 节：真实 dynamic fallback E2E。
4. 完成第 6 节：真实 `--pdf-mode ocr` 兼容性报告。
5. 完成第 8 节：最终 Stage 6 gate。
6. 完成第 9 节：发布报告，最后交由人工验收 native truth 和页面资格。
