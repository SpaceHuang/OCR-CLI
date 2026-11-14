# OvisOCR3 接入实施日志（断点续接锚点）

> 对应计划：`docs/develop/ovisocr3-integration-development-plan.md`（v1.5 终稿）
> 工作树：`.worktree/ovisocr3-integration`（分支 `feat/ovisocr3-integration`，基于 main@16577de）
> 推进方式：TDD（先判别/golden 测试，后实现；每提交全绿门禁）
> 状态标记：✅ 完成 / 🔄 进行中 / ⬜ 未开始 / ⏭ 跳过

---

## 环境事实（断点续接必读）

- 本机：Windows x64，CPU-only 构建（无 vulkan/cuda feature；GPU 对照属 Phase 5 注记级）。
- ovisocr2 四档 + mmproj 已装于默认缓存（`%LOCALAPPDATA%/ocrx/models/ovisocr2/8a22290a…/`）。
- huggingface.co 本机不可直连（DNS 污染）；**下载一律走 `https://hf-mirror.com`**（API 与 resolve 均可用，偶发 502 需重试）。
- vendored pdfium 在主 checkout `D:/Develop/OCR-CLI/vendor/pdfium/bin`（worktree 通过相对路径回退，spike 测试已内置该回退）。
- 基线：`cargo test --workspace --locked` 全绿（2026-11-14，@16577de + spike dev-deps）。

## Phase 0（调研与冒烟 spike）

### O3-T01 ✅ GGUF 供应链选型与冻结表

候选仓库（2026-11-14 经 hf-mirror API 核实）：

| 仓库 | revision | 文件（sha256 / 字节） | 判定 |
|---|---|---|---|
| **michalland/OvisOCR3-GGUF**（选定） | `7be644a312005c3a55e2c826aee985a5c93d06b2`（lastModified 2026-10-22） | `OvisOCR3-Q8_0.gguf` = `11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6` / 811,843,712 B；`mmproj-OvisOCR3-F16.gguf` = `698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd` / 204,987,136 B；LICENSE（Apache-2.0 全文）在仓库内 | **GO**：≥1 可用档（Q8_0）+ 对应 mmproj **F16**；单一转换源，模型/mmproj 自洽 |
| prithivMLmods/ATH-MaaS-OvisOCR3-GGUF（备选） | `c29551652e828085ca1e892092f1319dbee22d2f`（2026-10-29） | BF16/Q3_K_L/Q3_K_M/Q4_K_M/Q4_K_S/Q5_K_M/Q5_K_S/Q6_K/Q8_0 全阶梯 + `mmproj-bf16`（sha256 见 spike 记录 JSON） | 不满足字面 GO（mmproj 为 **BF16** 非 F16）；且与 michalland 混装属跨转换源风险。仅登记为灾备（michalland 失效时的替代），不进 manifest |

**冻结表（manifest 将采用，O3-T06 输入）**：

- 档位集 = **仅 `quality` 一档**（Q8_0 → quality，对齐 OvisOCR2 的 Q8_0→quality 映射）；GO 条件"≥1 档含 F16 mmproj"满足；"四档期望"如实记录为不可得（无单一转换源的 Q4_K_M/Q5_K_M/BF16 + F16 mmproj 组合）。
- revision pin = `7be644a312005c3a55e2c826aee985a5c93d06b2`；URL 基址 `https://huggingface.co/michalland/OvisOCR3-GGUF/resolve/<revision>`（本机测试下载走 hf-mirror 等价路径）。
- 单档位集正是 C15"仅单档"判别场景，D-7 可变档位集语义有真实判别载体。

**预验证事实**：OvisOCR2 GGUF 的 `general.architecture` 即 `qwen35`（本机 GGUF 头解析核实）→ 冻结 pin 0.1.161 已支持 qwen35 架构，T02 GO 预期强。

资产落位：两文件已按 manifest 布局下载到 `%LOCALAPPDATA%/ocrx/models/ovisocr3/7be644a3…/{quality/model.gguf, mmproj.gguf}`，sha256/字节均与冻结表一致（下载后本地复验通过）。

### O3-T02/T03/T04 🔄 真实模型 spike（探针已写，运行中）

- 探针：`src/ocrx-runtime/tests/ovisocr3_spike.rs`（env 门控 `OCRX_E2E_MODEL=1`；生产 actor 路径 `LlamaMtmdRuntime::load` → `generate_with_progress`）。
- 输出：`docs/develop/spike-assets/ovisocr3-spike-record.json` + 本文档结论回写。
- 已知输入：官方 `prompts/document_markdown.txt` 与 OvisOCR2 `OCR_MARKDOWN_V1` **语义逐字相同**，仅格式差（官方无前导 `\n`、"Preserve" 前为空格；现行冻结字节为 `\n` 前导 + `\n` 中缝）。原文已存 `docs/develop/spike-assets/ovisocr3-document_markdown.txt`。
- T03 关键裁定输入（D-2）：运行时最终 MTMD 输入 = `mtmd_default_marker()` + prompt（两 prompt 变体的输出形态都将记录）。

### O3-T05 ⬜ spike 结论冻结回写（待 T02–T04 完成）

## Phase 1 ⬜（O3-T06–T09）
## Phase 2 ⬜（O3-T10 / T13b / T11 / T12 / T13 / T14）
## Phase 3 ⬜（O3-T15–T19）
## Phase 4 ⬜（O3-T20–T24）
## Phase 5 ⬜（O3-T25–T29）

---

## 提交序列

（每提交全绿门禁后登记：任务 / 内容一句话）

1. **Phase 0（O3-T01–T05）**：spike 探针（env 门控真实模型测试）+ spike 记录 + 冻结表 + 计划回写（D-2 分支 1 / D-4 GO / D-5 零改 / T13b 不触发）+ 实施日志。门禁：fmt/clippy/test/arch/release-scripts/bootstrap 全绿。
