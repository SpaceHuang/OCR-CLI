# OvisOCR3 接入 spike 记录（Phase 0，O3-T01–T05）

> 对应计划：`docs/develop/ovisocr3-integration-development-plan.md` v1.5
> 日期：2026-11-14；环境：Windows x64，CPU-only 构建，冻结 pin（wrapper/sys 0.1.161，upstream `d8123504…`）
> 探针：`src/ocrx-runtime/tests/ovisocr3_spike.rs`（env 门控 `OCRX_E2E_MODEL=1`，生产 actor 路径）
> 证据文件：`docs/develop/spike-assets/ovisocr3-spike-record.json`（T02/T03/T04）、
> `ovisocr3-grid-record.json`（T04 深探 + OvisOCR2 对照）、`ovisocr3-document_markdown.txt`（官方 prompt 原文）
> 注：huggingface.co 本机不可直连，下载/元数据经 `https://hf-mirror.com` 等价路径获取（同一 revision/文件，sha256 校验一致）。

---

## O3-T01 供应链选型与冻结表 ✅

**选定（manifest 冻结源）**：`michalland/OvisOCR3-GGUF` @ revision `7be644a312005c3a55e2c826aee985a5c93d06b2`（lastModified 2026-10-22，Apache-2.0，library llama.cpp，arch `qwen35`）

| 档位 | 文件 | sha256 | 字节 |
|---|---|---|---|
| quality（唯一档） | `OvisOCR3-Q8_0.gguf` | `11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6` | 811,843,712 |
| （revision 共享） | `mmproj-OvisOCR3-F16.gguf` | `698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd` | 204,987,136 |

- **GO 条件满足**：≥1 可用档位（Q8_0）且含对应 **F16** mmproj；模型与 mmproj 同源同仓（单一转换源，自洽）。
- **四档期望如实登记为不可得**：该仓库仅发布 Q8_0 一个量化；档位集 = `["quality"]`（D-7 可变档位集——单档正是 C15 判别场景之一）。
- **备选登记（不进 manifest）**：`prithivMLmods/ATH-MaaS-OvisOCR3-GGUF` @ `c29551652e828085ca1e892092f1319dbee22d2f`（2026-10-29）有 BF16/Q3_K_L/Q3_K_M/Q4_K_M/Q4_K_S/Q5_K_M/Q5_K_S/Q6_K/Q8_0 全阶梯，但其 mmproj 为 **BF16**（非 F16，不满足 GO 字面条件），且与 michalland 混装属跨转换源风险。仅在 michalland 仓库失效时作为灾备重评（sha256 已录于审查记录，必要时从本提交历史可取）。
- **预验证**：OvisOCR2 GGUF 头 `general.architecture` 即为 `qwen35`（本机解析）→ 冻结 pin 已支持该架构。
- 资产已按 manifest 布局落位默认缓存 `<cache>/ocrx/models/ovisocr3/7be644a3…/{quality/model.gguf, mmproj.gguf}`，下载后本地复验 sha256/字节一致。

## O3-T02 冻结 pin 加载 spike ✅ **GO（pin 不动）**

- `LlamaMtmdRuntime::load`（生产 actor）加载 Q8_0 + mmproj F16：**成功**，load≈502ms（ssm/hybrid 架构元数据见 record JSON；`qwen35.context_length=262144` → 现行 context 20480 充足）。
- 真实 golden 页（150 DPI ×3 页）经 `generate_with_progress` 生成成功，非空输出。
- **D-4 裁定：pin 不动分支**——`DECODE_GENERATION=3` 不变，wrapper/sys/upstream 均 0.1.161/`d8123504…`，OvisOCR2 零回归成本。

## O3-T03 协议与输出形态 spike ✅ **D-2 = 分支 1（原生直喂）；O3-T13b【不触发】**

裁定依据（按 BLOCK-2/D-2 要求，以**运行时最终拼接后的完整 MTMD 输入文本**为准）：

- 运行时拼接 = `mtmd_default_marker()`（实测值 `"<__media__>"`）+ prompt，串行路径 `llama_mtmd.rs:828-834` 原样；探针记录了两种 prompt 的完整 MTMD 输入文本（record JSON `mtmd_input_text` 字段）。
- **原生直喂成立**：无 chat template 包装、无 EmbeddedInPrompt 需求——`<__media__>` 前置 + 原始指令直接生成正确输出。**分支 3 关闭，O3-T13b 标注【不触发——D-2 分支 1】**，runtime 文本协议零改造。
- **prompt 字节裁定**：官方 `prompts/document_markdown.txt` 与现行冻结 `OCR_MARKDOWN_V1` **语义逐字相同**，仅格式差（官方无前导 `\n`、"Preserve" 前为空格而非 `\n`）。三页 × 双 prompt 变体的生成输出**逐字节一致**（record JSON 六条 probe `raw_output` 全同）→ ovisocr3 pack 冻结沿用 `OCR_MARKDOWN_V1` 现字节（同字节、同三元组值、独立 pack 模块与冻结点），spike 等价性证据归档于此。
- 输出形态逐项：
  - **bbox**：`<img src="images/bbox_{l}_{t}_{r}_{b}.jpg" />` 出现且坐标在 [0,1000)（如 `bbox_55_26_962_637`）——与 OvisOCR2 约定一致；
  - **think 泄漏**：三页均出现 `<think>`…`</think>` **空内容块**——恰为 Q3.1"静默空块剥离 + 计数"语义的输入，现行 adapter strip 兼容（非空块致 `think_removed` 警告、空块静默计数的分型不变）；
  - **Markdown 结构**：标题层级（`#`/`##`）、正文、中英文混排正常；
  - **表格/公式**：本探针三页无表格/公式内容（质量覆盖交给 O3-T25 fixture gold 与 O3-T26 OmniDocBench 全量对照）；
  - **退化尾部**：本探针无重复尾形态（`ends_with_repeated_tail=false`；`degenerate_tail_crop` 兼容性由阶梯测试与 T25/T26 继续盯）。
- **`page_parser_version` 裁定**：双模型共用 `ovis-page-v1`（输出约定一致，无需 per-model 升版）。

## O3-T04 网格与预算 spike ✅ **D-5 = 阶梯零改；后经生产全页证据修订——基础预算 per-model（见下方修订注记）**

- `image_tokens_override=2048` **被接受且生效**：runtime 按 override 构建 min=max=2048 的独立 MTMD 投影器上下文（`llama_mtmd.rs:566-590` 既有机制），tokenize/生成无错误。
- **深探（含 OvisOCR2 对照）**：同三页，OvisOCR3 在 2048 rung（+retry sampling，生产 rung-2 形态）输出 460/68/1393 字节，与 plain 基线 458/68/1390 相当；OvisOCR2 对照 448/49/1373 同样正常（`ovisocr3-grid-record.json`）。
- 注记：首次探针（无 retry sampling 形态）在 2048 网格下出现一次 0 字节输出；生产阶梯的 2048 rung 恒带 retry sampling（Q1.2），复测正常。O3-T18 的阶梯判别测试以生产 rung 形态覆盖此差异。
- **裁定**：`SUPPORTED_IMAGE_TOKEN_GRIDS` 保持 `[2048]`，scan 退化阶梯零改，无 per-model `ladder_grid` 配置。

### D-5 修订注记（2026-11-14，O3-T18 生产证据）

spike 阶段的 150 DPI × 3 页探针不足以覆盖生产渲染形态。真实 30 页复合扫描（300 DPI，
4.4MP 页，scan 默认渲染预算 2880²）暴露：**OvisOCR3 在 scan 默认 `max_image_tokens=1024`
下大量密集页输出为空**（27 个 OCR 页中 14 页近空；同页 OvisOCR2 输出完整）。单页矩阵实验
（页 6）：300 DPI + 1024 → 0 字节；**300 DPI + 2048 → 2892 字节完整正文**；150 DPI + 1024
亦空（页 6 密度下 1024 网格本身不足）。

- **裁定修订**：`SUPPORTED_IMAGE_TOKEN_GRIDS` 保持 `[2048]` 不变、退化阶梯零改不变；
  **scan 级基础图像 token 预算默认值改为 per-model**——ovisocr3 默认 2048、ovisocr2 默认
  1024 逐字节不变（铁律 1）。这正是 D-5 预设的"rung1 网格值随模型配置"分支，以最小形式
  （CLI 默认值解析函数）落地，无通用能力声明框架。
- **检空盲区登记**：空输出（立即 EOS）不被退化阶梯拦截——状态 complete、termination eos、
  非 truncated/degenerated。该盲区对双模型同样存在（登记为后续独立质量工作，不在本计划
  范围内扩大）。

### 窗口解码串行地板（2026-11-14，O3-T26 生产证据——平台级发现）

D-5 修订后的真实全页扫描（300 DPI，30 页复合）仍出现**非确定性的近空页**（每次运行
空页集合不同，总计 ~11.7KB vs OvisOCR2 同页 ~30KB）。归因实验：

- **单流直接探针**（runtime 直驱，页 4，300 DPI + 2048 网格）：**完整正文 3003 字符**
  （公式 LaTeX、HTML 表格、标题层级全部正常）；
- **CLI 多页扫描**（窗口批量解码，batch_depth=8，同页同参）：页 4 近空（立即 EOS）；
- CLI 单页扫描（窗口含 1 页）：正常。

**证据链（2026-11-14，全部本机实测）：**

1. 单流直接探针（runtime 直驱，页 4，300 DPI + 2048 网格）：完整正文 3003 字符（LaTeX
   公式、HTML 表格、标题层级全部正常）；
2. CLI **单页**扫描（同页同参）：完整输出（~2.9KB）；
3. CLI **多页**扫描（batch_depth=8 窗口）：非确定性近空页（每次运行空页集合不同）；
4. CLI 多页扫描 + **串行窗口地板（batch_depth=1，全部页走串行路径）**：近空页集合与 3
   **完全相同**（14/27 OCR 页）——**串行地板假设被证伪**：地板不是根因；
5. **上下文复用探针**（同一 runtime 连续生成页 2 → 页 4，共享 KV 语义）：两页全部完整
   （1391 / 3003 字符）——跨页状态复用在直驱路径上无害。

**当前结论（诚实登记；2026-11-15 复审证据更新）**：OvisOCR3 在 scan 生产路径上存在
**未定位根因的生成退化**——同一模型、同页、同参在直驱路径与 scan 路径上输出差异巨大
（直驱完美，scan 路径间歇性立即 EOS），且空页集合跨运行非确定。已排除：网格预算
（2048 已生效）、窗口批量（地板后仍复现）、跨页 KV/SSM 污染（复用探针无害）、模型资产
（sha256 冻结校验）。**剩余候选因素**：scan 路径的 actor Generate 处理与直驱
`generate_with_progress` 的实现差异、渲染/生成的同进程 pdfium owner 线程交接
（`render/pdfium_owner.rs`——线程 + channel，非独立进程）的干扰、或其他路径细节——
已立项（`docs/develop/scan-path-empty-generation-root-cause-plan.md` v1.1，
评审修订后待实施）。

**复审实测更新（2026-11-15，重要）**：检空安全网落地后，真实 30 页扫描
30/30 完成、0 失败/0 截断/0 退化、退出码 0——**此前空输出的 12 页全部在第 3 次尝试
恢复非空正文**（该次尝试同时变更"重新渲染 + 重试采样链"两个变量，单变量归因待
对照实验——见 root-cause 立项 v1.1 事实校正 2）；仅 18/29 两页为空白路由页（本就该空）。即该退化
是**阶梯可救回的**（重复罚分采样改变采样轨迹即可绕开立即 EOS），不再是硬性内容丢失；
代价为重试墙钟（30 页 ≈ 1535s）。立即 EOS 的底层根因与重试成本优化仍按计划留作
root-cause 立项。

- **已落地的防御性措施（保留）**：per-model 基础预算 2048（页 6 实证必要）与
  `batch_depth_floor_for` 串行地板（防御混合 SSM 批量语义风险，判别测试钉死；即使非
  本缺陷根因，也是该架构在本 pin 上的正确保守语义）。
- **质量结论（D-GATE 输入，R5 口径）**：本机 30 页复合样本上 OvisOCR3 scan 产物显著
  差于 OvisOCR2（~11.7KB vs ~30KB，14/27 OCR 页近空）；**默认模型不切换**；接入按
  "能力可用 + 差距分析入档" 交付（G1 面全部走通）。
- **检空盲区（已修复，2026-11-14 评审 BLOCK 响应）**：空输出（立即 EOS）原本绕过退化
  阶梯被静默提交为 Complete（且可被 --resume 复用）——已修复：空 OCR 最终文本即
  degeneration 案例（`empty_ocr_output` 警告），进入阶梯重试；阶梯耗尽仍空则提交为
  退化页（result.json partial + 退化计数 + 简历永不复用）。判别测试：artifact 层 2 例 +
  window 层 2 例（阶梯全走 + rung 恢复早退）。附带健壮性修复：`detect_column_bands`
  对 1 像素宽等退化几何回退单带（原会越界 panic）。

## O3-T05 结论冻结回写 ✅

| 决策 | 状态 |
|---|---|
| D-1 adapter crate `ocrx-ovisocr3` | 已冻结（计划既定，镜像形态） |
| D-2 prompt 归属与包装 | **已冻结——分支 1（原生直喂）**；ovisocr3 pack = `OCR_MARKDOWN_V1` 现字节 + 同三元组值；分支 3 关闭，**O3-T13b【不触发——D-2 分支 1】** |
| D-4 runtime pin | **已冻结——GO（pin 不动）**，`DECODE_GENERATION=3` 不变 |
| D-5 网格能力 | **已冻结——阶梯零改**，无 per-model 网格配置 |
| `page_parser_version` | 双模型共用 `ovis-page-v1` |
| O3-T06 冻结表输入 | §O3-T01 表（单档 `quality`/Q8_0 + F16 mmproj @ `7be644a3…`） |

**Phase 2 开工前触发状态已知**：O3-T13b 不触发（D-2 分支 1）——满足 v1.4 复审 BLOCK-3 的时序要求。
