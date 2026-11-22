# M4.2 前置：SLANet-plus 专用表格模型 Oracle Spike 裁决（2026-11-22）

## 结论：**GO** —— 通过预注册门，进入 M4.2 集成计划起草

## 背景

OvisOCR3 基线（r3-ovisocr3，Overall 90.87）在 53 张表上得 0 分，集中在 40 个
"页面 emitted 零表格块"（block_N==0）的页——即 M4.1 F6 的目标人群。
M4.1 spike 已证明：用同一个 VLM 重读救不动这批表（0/24 采纳，能力边界）。
本 spike 回答的问题是：**换专用表格结构模型（SLANet-plus），这批表能不能救回来？**

## Oracle 实验设计（排除检测变量）

- 人群：40 个纯零页（F6 目标人群的主体），其上共 **57 张 GT 表**
- 输入：**GT 真值框裁剪**（不含检测误差），RGB→BGR 喂给 SLANet-plus
  （TableRecognitionPipelineV2, CPU, enable_mkldnn=False；模型冻结
  hf-mirror revision `bae6e5f8`，7.7MB，paddleocr 3.7.0）
- 评分：官方 OmniDocBench 评测（per-table TEDS），与 r3-ovisocr3 同键对齐
- **预注册门**：均值 TEDS ≥ 0.60 且 ≥60% 表 ≥ 0.5 → 过线才做集成

## 结果

| 口径 | n | 均值 TEDS | ≥0.5 占比 | 基线均值 | 胜/负(±0.05) |
|---|---|---|---|---|---|
| 全部 57 表 | 57 | **0.604** | 64.9% | 0.086 | 41/10 |
| 有产出 45 表 | 45 | **0.765** | 82.2% | 0.072 | 41/3 |
| 空产出 12 表 | 12 | 0.000 | 0% | 0.137 | 0/7 |

- **门 1**（均值 ≥0.60）：0.604 ✓（压线通过）
- **门 2**（≥60% 表 ≥0.5）：64.9% ✓
- 25 张表 TEDS ≥0.85（其中 13 张 =1.000），而基线在这批表上均值仅 0.086

## 空产出归因（12 表，机械性失败而非识别失败）

7 个预测文件 0 字节 + 2 个纯换行文件（覆盖 5 表）= 12 张 0 分表全部对应
管线 `table_res_list` 为空 / `pred_html` 为空的**推理失败**：
TableRecognitionPipelineV2 的内嵌单元格检测在这批裁剪上未产出结构。
集成时需兜底：空产出 → 保原输出（零损害）。

## 与 M4.1 spike 的对照

| | M4.1（VLM 重读） | 本 spike（SLANet-plus） |
|---|---|---|
| 人群 | 16 页 24 表（含 raw/empty 臂） | 40 页 57 表（GT 裁剪） |
| 采纳/得分 | 0/24 采纳 | 均值 0.604，64.9% ≥0.5 |
| 结论 | 能力边界，停止 | **通过预注册门，GO** |

## 局限（诚实声明）

1. **GT 裁剪是上界**：生产集成还需表格区域检测（这些页 layout 未发射表格块），
   检测质量会打折；M4.2 计划须包含检测臂的独立验证。
2. 均值 0.604 压线：若 12 张空产出被兜底逻辑放弃，全人群均值就是 0.604；
   但"有产出子集"0.765 说明模型本体识别力远高于门线。
3. CPU 推理延迟未系统测量（spike 期单表秒级），集成计划需含吞吐预算。

## 证据链

- 裁决 JSON（逐表）：`C:/t/m23/slanet-oracle-adjudication.json`
- 预测 md：`D:/t/slanet-oracle-predictions/`（40 文件）
- 官方评测：`benchmarks/omnidocbench/reports/slanet-oracle_quick_match_*`
- 人群冻结：`D:/t/oracle-pages.json`（40 pure-zero 页）
- 下一步：M4.2 集成计划（F6 触发 + SLANet-plus 路由 + 检测臂 + 兜底 + 回归门）
