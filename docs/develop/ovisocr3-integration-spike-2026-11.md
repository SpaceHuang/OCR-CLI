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

## O3-T04 网格与预算 spike ✅ **D-5 = 阶梯零改**

- `image_tokens_override=2048` **被接受且生效**：runtime 按 override 构建 min=max=2048 的独立 MTMD 投影器上下文（`llama_mtmd.rs:566-590` 既有机制），tokenize/生成无错误。
- **深探（含 OvisOCR2 对照）**：同三页，OvisOCR3 在 2048 rung（+retry sampling，生产 rung-2 形态）输出 460/68/1393 字节，与 plain 基线 458/68/1390 相当；OvisOCR2 对照 448/49/1373 同样正常（`ovisocr3-grid-record.json`）。
- 注记：首次探针（无 retry sampling 形态）在 2048 网格下出现一次 0 字节输出；生产阶梯的 2048 rung 恒带 retry sampling（Q1.2），复测正常。O3-T18 的阶梯判别测试以生产 rung 形态覆盖此差异。
- **裁定**：`SUPPORTED_IMAGE_TOKEN_GRIDS` 保持 `[2048]`，scan 退化阶梯零改，无 per-model `ladder_grid` 配置。

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
