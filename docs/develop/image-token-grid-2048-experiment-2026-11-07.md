# E1 实验报告：图像 token 网格 1024 → 2048（OmniDocBench 报纸子集）

**日期：** 2026-11-07
**依据：** 优化方案 v1.2.1 §1.1"推理栈本征差异"讨论中提出的图像路径假设；实验设计与判据经会话内确认
**性质：** 一次性对照实验（非基线、非门槛复评）；证据服务于方案 Q1.1 候选 D 与 Q1.4 阶梯排序
**执行环境：** ocrx 二进制 f3b6e068（Vulkan，2026-11-05），GGUF rev `8a22290a`（balanced），RTX 4090 D

---

## 1. 假设演变（实验前提的核实过程）

1. 初始假设：官方 transformers 路径直接消费原始图片，我们经 PDF round-trip（图片 → 单页 PDF → PDFium 渲染），渲染失真可能是残差来源。
2. **渲染保真核实（推翻失真假设）**：批量核对 mapping 源图尺寸 × render-audit（前 400 页），**395/400 页 `rendered` 与源图像素完全 1:1**（`scaled=false, ratio=1/1`），仅超 2880² 预算页被重采样。（最初"480×360 放大到 2000×1500"为审计字段误读：`original` 记录的是 PDF 磅换算值。）
3. **收窄到 token 网格**：MTMD 图像 token 网格固定 1024（`image_min_tokens == image_max_tokens`，`src/ocrx-runtime/src/llama_mtmd.rs:428`），与源像素多少无关——这是我们栈内唯一的分辨率天花板，且 `--max-image-tokens` 为现成 CLI 参数（默认 1024），零代码改动可测。
4. 注：ocrx 仅接受 PDF 输入（`resolve_scan_inputs`，`src/ocrx-cli/src/main.rs:304`），"原图直通"对照不可行；由第 2 条的 1:1 证据，该对照已无必要。

## 2. 设计

| | arm A（冻结基线） | arm B（本实验） |
|---|---|---|
| 页面 | 报纸子集 151 页（GT `data_source: newspaper`） | 同 151 页 |
| 二进制 / 模型资产 | f3b6e068 / `8a22290a`（balanced） | 同 |
| profile / DPI / prompt | balanced / 300 / `ocr-markdown-v1` | 同 |
| 图像 token | 1024（默认） | **2048** |

arm A 数据取自冻结基线（`scans/balanced` + `export-report.json` + 官方逐页/逐样本明细），不重跑；arm B 为 fresh 扫描。

判据：① 同页配对退化率；② 官方口径子集评分（文本逐页 / 表格逐样本）。

## 3. 结果

### 3.1 退化率（同页配对，n=151）

| | arm A (1024) | arm B (2048) |
|---|---|---|
| 退化 | 92（60.9%） | 76（50.3%） |
| 截断 | 8 | 4 |

配对：A 退化 92 页中 **69 页仍退化（75%）**、**23 页逃逸（25%）**；7 页 A 正常 B 退化（阶梯语义下无回归风险——重试只在页面退化后进入下一档）。

**关键读法（服务于重试阶梯）**：arm A 提交退化页 = 1024 全阶梯失败（原始 + 0.94× + 1.06×，`attempts=3`）；arm B 救回的 23 页 **`attempts` 分布全部为 1**——2048 首试直接逃逸，无需档内扰动。即：**2048 对"阶梯完全失败"页面的边际逃逸率 = 25%，且只需一次尝试**。贪心解码确定性保证归因干净（逃逸全部来自输入变化，非"再掷一次骰子"）；对照：扰动重卷边际逃逸率 0%（同输入确定性复现）。

### 3.2 质量（官方口径，报纸子集）

| 分量 | arm A | arm B | Δ |
|---|---|---|---|
| 文本 (1−edit，n=151 页) | 37.81 | 52.10 | **+14.3** |
| 表格 TEDS all（n=69 样本） | 15.72 | 24.39 | **+8.7** |
| 公式 CDM | n=0 | — | 报纸页无公式样本（CDM 弱势在学术文献，与报纸无关） |

（阅读顺序 edit 0.4782 无同口径 arm A 子集参照，不引用。）

### 3.3 成本

arm B ≈ 6.3 s/页 vs arm A 3.27 s/页（全集速率折算），**≈2×**（图像编码成本随 token 数增长）；窗口上下文 8×(2048+16384+1024) ≈ 156k cells，RTX 4090 24GB 显存无压力。

## 4. 全集外推（线性估算）

非报纸部分用冻结数据拆分：文本 +1.28、表格 +0.88、公式 0 → **全集 Overall ≈ +0.7**，代价约 2× 扫描时间。子集评分为 2 分量口径（无公式样本），外推为估算值。

## 5. 结论

1. **图像路径假设结案**：渲染 1:1 保真排除 round-trip 失真；token 网格真实但温和（全集 ≈ +0.7 @ 2× 成本）。优化方案 §1.1 残差 ~5.4 分的主体归档为解码动力学 + kernel 数值层（真本征）。
2. **退化强吸引子再次确认**：2× 视觉分辨率只救回 1/4 的退化页；治退化的路径是 Q1 逃逸阶梯（采样 / 结构化 / token 档），不是看得更清。
3. **token 2048 作为阶梯末档候选**（方案 Q1.1 候选 D）：25% 边际逃逸率 + 一次尝试即逃逸 + 逃逸页质量提升；成本最高故排最后。全局默认值不动（避免全量 2× 成本 + v2 冻结面），2048 仅作为重试策略存在。

## 6. 边界与待办

- 仅报纸子集；学术/试卷等其他退化类别的边际逃逸率待 Q1.1 语料实验补测（fresh 2048 重扫 213 页语料，约 22 min，零代码改动）；
- 扫描日志 7 次 llama.cpp `find_slot: non-consecutive token position` 警告——评分虽改善，落地前需核实其对个别页输出的影响（待办，进 Q1.4c spike）；
- `--max-image-tokens` 进 checkpoint 兼容参数：重试路径内部换网格对兼容键的影响在 Q1.4c spike 中定案；
- arm B 含其自身 3 档扰动阶梯（2048 档内扰动）；23 个逃逸页 attempts=1，阶梯内部扰动对结论无影响。

## 7. 产物索引

- arm B 扫描：`benchmarks/omnidocbench/scans/newspaper-tokens2048/`（151/151 完成，exit 0）
- arm B 预测：`benchmarks/omnidocbench/predictions/newspaper-tokens2048/`（151 有效 + 1500 空 stem——导出脚本按全集 mapping 校验的预期行为）
- 官方评分明细：`benchmarks/omnidocbench/reports/newspaper-tokens2048_quick_match_*`
- 子集 GT / PDF：`benchmarks/omnidocbench/dataset/OmniDocBench-newspaper.json`（151 页）、`benchmarks/omnidocbench/pdfs-newspaper/`
- 对照数据：`benchmarks/omnidocbench/predictions/balanced/export-report.json`（arm A 坏页标记）、`benchmarks/omnidocbench/reports/omnidocbench-balanced-all_quick_match_*`（arm A 官方明细）
