# OmniDocBench 外部基线（ocrx / OvisOCR2 四档量化）— 2026-11-06

> **版本记录**：ocrx 二进制 `f3b6e068…`（Vulkan 构建，2026-11-05）@ repo `1d6d109`；模型 GGUF rev `8a22290a`（四档 Q4_K_M/Q5_K_M/Q8_0/F16）；llama.cpp wrapper 0.1.152 / sys 0.1.153（rev `9e3b928`，窗口深度 8）；数据集 revision `aa1ee96d`（v1.6 主线，1,651 页）；评测仓库 HEAD `ea2d138c`（toolkit 1.6.0，Docker 镜像 `repro-ubuntu2204`）；scorer=官方 `pdf_validation.py`，`quick_match`，workers 8（quality/reference 链 4）。

## 1. 目的与口径

v2 推理平台重构前的外部质量基线。官方榜单 OvisOCR2 BF16（transformers 路径）Overall = **96.58**（技术报告）；本基线回答：**llama.cpp + PDFium 量化部署落在哪，损耗在量化还是在管线**。仅作 baseline，未提交榜单。完整复现步骤见 `benchmarks/omnidocbench/README.md` 与 `manifest.json`。

- 图片→单页 PDF（`build-omnidocbench-pdfs.py`，MediaBox = px×72/300）：1,651 页 **0 缺失 0 重复 0 重编码**（1015 JPEG 原样 + 636 PNG Flate）；172 页超 2880² 渲染预算由 PDFium 等比缩（render-audit 记录）。
- 扫描：`ocrx scan pdfs -o scans/<profile> --dpi 300`（四档全部 1,651/1,651 完成）。
- 导出：`export-omnidocbench-predictions.py` → `<image stem>.md`（剥 bbox 图片链接与 `<think>` 泄漏）。
- 评分：官方 `pdf_validation.py` ×12（4 档 × all/en/zh；filter=`page_attribute.language`）。Overall = ((1−文本编辑距离) + 表格 TEDS + 公式 CDM)/3 × 100，与官方口径一致。

## 2. 评分总表（官方口径，v1.6 全集）

| Overall | all (1651) | en (755) | zh (765) |
|---|---|---|---|
| compact (Q4_K_M) | 75.60 | 68.83 | 84.93 |
| balanced (Q5_K_M) | **77.99** | 70.72 | **85.82** |
| quality (Q8_0) | 76.89 | 70.72 | 85.80 |
| reference (F16) | 75.85 | 69.16 | 85.38 |
| *官方 BF16 (transformers)* | *96.58* | — | — |

分项（all 切分，文本按 (1−edit)×100）：

| 档位 | 文本 | 公式 CDM | 表格 TEDS | 阅读顺序 edit | 扫描耗时 |
|---|---|---|---|---|---|
| compact | 83.76 | 0.7230 | 0.7074 | 0.2249 | 81.6 min |
| balanced | **84.61** | **0.7925** | 0.7011 | **0.2198** | 90.0 min |
| quality | 84.61 | 0.7428 | **0.7177** | 0.2290 | 96.0 min |
| reference | 84.08 | 0.7291 | 0.7056 | 0.2290 | 117.3 min |

## 3. 核心结论

**结论一：量化不是瓶颈。** 四档 Overall 挤在 75.6–78.0（极差 2.4 分），balanced (Q5) 反超 F16 reference 2.1 分。量化损耗完全被管线层损耗淹没；**选档应纯按速度/显存取舍，推荐 balanced（Q5_K_M，529→~660MB 级别档位中解码最快一档以上）**。

**结论二：差距在推理栈行为，不在权重精度。** llama.cpp F16 部署比官方 BF16 transformers 路径低 20.7 分——同权重不同推理路径，差距来自采样/批处理/prompt 执行差异。

**结论三：约 80% 的差距集中在 13% 的退化页上，可修。** balanced-all 逐页拆解（"bad" = checkpoint 标记 degenerated/truncated，共 241 页）：

| 分量 | 正常页 | 退化/截断页 | 全部修复后 |
|---|---|---|---|
| 文本 (n=1277/179) | 93.52 | 17.73 | +9.3 → 93.5 |
| 公式 CDM (n=271/42) | 93.05 | 8.99 | +11.3 → 93.1 |
| 表格 TEDS (n=372/86) | 92.03 | 14.27 | +14.6 → 92.0 |
| **Overall 投影** | | | **77.99 → ~92.9** |

> **勘误（2026-11-07）**：本表公式/表格分项为按页聚合口径，而官方 Overall 的公式/表格分量为**全样本** CDM/TEDS 均值，两者不可混用——"92.9 满收"与"≥88 = 修复过半"据此作废。官方口径复算见 `scripts/omnidocbench-projection.py` 与优化方案 v1.2 §1.1：Q1 退化语料 213 页全修复投影 **91.14**，θ=0.5 → 84.57。坏页账本精确化：213 退化（含 5 退化∩截断交集）/ 28 纯截断 / 1 缺失。详见 §7。

正常页/正常样本在官方口径下为 90–94（文本 93.9 / 公式样本 89.8 / 表格样本 89.8，213 页剔除口径，勘误 §7）。修复 213 个退化页即可把 Overall 从 78.0 投影到 **91.1**（`scripts/omnidocbench-projection.py`），剩余 ~5.4 分为推理栈本征差异与未修集合（纯截断/空输出页）。

**结论四：退化与量化无关，与版式强相关。** 四档退化页数 203–218（~13%），F16 同样退化；来源分布高度集中——**报纸 151 页中 92–94 页退化（61%）**，其次学术文献（~20%）、试卷（~16%）；中英比例与全集一致。多栏密排 + 阅读顺序复杂是触发条件。

**结论五：语言切分呈结构性落差（~15 分）。** en 69–71 / zh 85–86。文本中英持平（~84），落差来自英文公式 CDM（0.68–0.76 vs 0.92）与英文表格 TEDS（0.51–0.58 vs 0.81）——英文学术三线表/嵌套表与密集 LaTeX 是重灾区。

## 4. 已知缺陷（基线中发现，均跨档复现）

| 缺陷 | 规模 | 影响 | 处置 |
|---|---|---|---|
| 重复退化整页报废 | 203–218 页/档（61% 为报纸） | 主要质量损失（见结论三） | 待修（v2 靶点） |
| 输出截断 | 23–38 页/档 | 计入退化同上 | 待修 |
| `<think>\n\n</think>` 泄漏 | 251–329 处/档 | 导出时已剥离并计数 | 产品侧修复待办 |
| 单个非法 bbox 链接导致整页 `recognition_failed` | 每档 0–1 页（balanced/quality 各 1 页报纸） | 该页无输出（按空预测计） | 稳健性待修 |
| 近纯图 PPT 页空输出 | compact 25 / quality 11 / balanced 5 / reference 10 页 | 极低分页 | 观察项 |

## 5. v2 重构验收建议

1. **质量门槛**（2026-11-07 勘误，官方口径）：`balanced-all Overall ≥ 85`（≈ Q1 退化语料缺口关闭 53%，即"退化修复过半"原意）为 GO；`≥ 88`（θ≈76%）为进取检查点；满收参考 **91.1**（213 页全修复投影）。原"≥88 为过半 / 92.9 为满收"因聚合口径混用作废，见 §7。
2. **退化修复实验清单**（质量评估模块可直接量化）：llama.cpp 升级 0.1.161（phase-3 审计已点名，逐序列采样器）+ repeat-penalty 调参；报纸多栏页的窗口深度/输出预算；退化页自动重扫（现 `--resume` 只补崩溃不补退化）。
3. **对照锚点**（2026-11-07 勘误后数值）：官方 96.58 = 分量级 96+/97+/97+；本项目正常页/正常样本在官方口径下为 90–94（勘误 §7），故"追平官方"的主体等价于"消除退化页"，不必重训模型。

## 6. 产物索引

- 原始评测输出（12 轮 metric/per-page/log）：`benchmarks/omnidocbench/reports/omnidocbench-*`（gitignore 外，随库提交）
- 版本冻结与复现：`benchmarks/omnidocbench/manifest.json`、`README.md`
- 工具链：`scripts/build-omnidocbench-pdfs.py`、`scripts/export-omnidocbench-predictions.py`、`scripts/run-omnidocbench-eval.py`
- 预测与扫描产物（不入库）：`benchmarks/omnidocbench/{scans,predictions,pdfs,dataset}/`

## 7. 勘误（2026-11-07）

评审 `docs/review/omnidocbench-quality-optimization-development-plan.md`（BLK-1）发现 §3/§5 的收益投影混用了聚合口径：文本为逐页 (1−edit) 均值，而公式/表格的官方 Overall 分量为**全样本** CDM/TEDS 均值（`all.CDM.all = 0.7925`、`all.TEDS.all = 0.7011`；按页聚合值 0.8177/0.7742 不是官方构成值）。§3 分项表的正常/退化页均值同为按页口径，不能据此外推官方 Overall。

权威复算：`scripts/omnidocbench-projection.py`（官方逐页/逐样本明细 + export-report 坏页标记 × mapping 连接）：

| 场景 | Overall |
|---|---:|
| 当前（=官方 77.99） | 77.9909 |
| Q1 退化语料 213 页全修复（θ=1） | 91.14 |
| 缺口关闭 50% | 84.57 |
| 全部坏页（241+缺失 1）全修复 | 93.48 |

坏页账本精确化：**213 退化页（含 5 个退化∩截断交集）/ 28 纯截断页 / 1 缺失页（bbox 链接整页失败）/ 5 近纯图空输出页**；`<think>` 泄漏为 329 页（其中 311 页为正常页）。§3 结论一/二/四/五不受影响；结论三的定性结论（缺口集中于退化页、正常页已近官方）保持，受影响的是满收数值（92.9 → 91.1/93.5 依修复集合而定）与"≥88 = 过半"的门槛解释（→ GO ≥ 85，88 ≈ 关闭 76%）。优化方案 v1.2 见 `docs/develop/omnidocbench-quality-optimization-development-plan.md`。
