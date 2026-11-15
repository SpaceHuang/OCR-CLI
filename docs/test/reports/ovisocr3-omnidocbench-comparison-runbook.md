# OvisOCR3 OmniDocBench 对照复评 runbook（O3-T26）

> 本文档固化**全量对照复评**的操作程序。本机（CPU-only 构建）无法在本立项窗口内完成全量
> 991 页基线复跑（单页 CPU 推理分钟级，全量 ≫ 8 小时）；本立项交付的对照证据为
> **同页同参 30 页复合样本对照**（见 `ovisocr3-integration-acceptance-report.md` §样本对照），
> 全量复评按下述程序执行。

## 前置

1. 数据集：`benchmarks/omnidocbench/dataset/OmniDocBench.json`（Ground Truth）与对应页图；
   venv：`benchmarks/omnidocbench/.venv`（toolkit 依赖，见 `benchmarks/omnidocbench/README.md`）。
2. 模型资产：两模型全档（ovisocr2 四档 / ovisocr3 quality）经 `model install` 安装并通过
   `model verify`。
3. 后端与量化注记：对照只在同 harness 同参内进行；官方模型卡分数**仅背景引用**
   （OvisOCR3 96.74 / OvisOCR2 96.58 不可与本仓 harness 基线直接比较）。

## 步骤 1 — OvisOCR2 基线复跑（确认链路对齐 84.92）

```bash
cd benchmarks/omnidocbench
export PDFIUM=/abs/path/vendor/pdfium/bin   # OCRX_PDFIUM_LIBRARY 指向
# 预测（CPU；GPU 构建同理，报告须注明后端）
../../target/release/ocrx scan <bench-pdf-dir> --model ovisocr2 --model-profile balanced \
  -o predictions/ovisocr2-balanced-rerun --progress jsonl
# 归一化 + 打分（think regex 归一化适用性：两模型同家族，think 语义相同）
$PY ../../scripts/export-omnidocbench-predictions.py ...
bash eval-balanced.sh   # quick_match / 全量按 README
```

对齐校验：总分类须落在 `docs/test/reports/omnidocbench-0161-milestone-2026-11-10.md`
基线 84.92 的噪声带内（vulkan/CUDA 噪声 ±3% 注记；CPU 复跑噪声按里程碑报告同口径）。

## 步骤 2 — OvisOCR3 全量

```bash
../../target/release/ocrx scan <bench-pdf-dir> --model ovisocr3 -o predictions/ovisocr3 \
  --progress jsonl
# 同一 harness/同参打分；退化/截断统计随 result.json/scan-summary 收集
bash eval-balanced.sh   # 以 --predictions predictions/ovisocr3 变体执行
```

## 步骤 3 — 对照归因

- 分数表：总体 + en/zh 分列（text/formula/table/reading-order）。
- 逐页差异抽样：两模型 `per_page_edit` / `per_sample_CDM` 输出做页级差值排序，
  差异最大页逐类（正文/表格/公式/阅读顺序/bbox 产物）人工归因。
- 退化统计：`truncated/degenerated/think_blocks_stripped_total` 双模型对照
  （2048 rung 对 OvisOCR3 生效，见 spike 记录 §O3-T04）。

## 判定口径（D-GATE 输入）

- 对照结论仅在本仓口径内成立；质量不及 OvisOCR2 同 harness 基线 → D-GATE **不切换**默认模型。
- 供应链可靠性（GGUF 仓库修订频率/哈希稳定性/下载成功率）随对照报告一并归档。
