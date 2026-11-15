# B0' 版面检测前置——候选 1 裁决记录（DocLayout-YOLO DocStructBench，2026-11-15）

**检测门（B1 硬前置）：** 表格/公式区域召回 ≥ 90%（B0a 冻结子集，GT IoU ≥ 0.7）
**状态：** 候选 1 **FAIL**（两档画布/动态配方三点实测均欠门）；候选 2（PP-DocLayout）待换型评估

## 1. 候选资产

- 模型：`doclayout_yolo_docstructbench_imgsz1024.onnx`（75,324,598 字节，
  hf-mirror.com/wybxc/DocLayout-YOLO-DocStructBench-onnx，MinerU 系布局模型的
  DocStructBench 权重，10 类含 table / isolate_formula）
- 运行时：onnxruntime 1.31.1 CPU（**ORT sidecar 路线可行性本身得到验证**——
  单次前向 ~0.47–1.4 s/页，无文本解码；仓库无需新增 ONNX 依赖的结论待候选 2 一并裁决）
- 评估器：`scripts/omnidocbench-b0-detect-probe.py`（B0a 冻结 744 页，
  GT 合并 equation_isolated+equation_semantic 为公式类；贪心 IoU 匹配 @0.7，
  conf ≥ 0.25——参考仓库推理配方逐行复刻）

## 2. 三点实测（744/744 页，0 错误 0 跳过）

| 画布配方 | 表格 recall | 表格 precision | 公式 recall | 公式 precision | 匹配 IoU | 单页耗时 |
|---|---|---|---|---|---|---|
| **钳制 1280** | **0.826** | 0.888 | **0.481** | 0.841 | 0.963 / 0.921 | 467 ms |
| 钳制 2048 | 0.644 | 0.738 | 0.461 | 0.815 | 0.883 / 0.911 | 1196 ms |
| 动态（护栏 2560，贴近参考配方） | 0.507 | 0.572 | 0.448 | 0.804 | 0.882 / 0.909 | 1420 ms |

产物：`benchmarks/omnidocbench/reports/analysis/analysis-b0-detect-gate{,-2048,-2560}.json`
（含逐页漏检清单 missed_top）

## 3. 结论与证据链

1. **门判定 FAIL**：最优工作点（钳制 1280）表格 82.6% / 公式 48.1%，双双 < 90%。
2. **域差距是主因**：DocStructBench 训练域为英文学术论文；本基准 56% 坏页为中文报纸、
   大量中文教材/试卷/PPT。匹配框 IoU 0.92–0.96（定位精度优秀）说明失败模式是
   检出缺失而非几何漂移；PPT 页在 missed_top 占绝对多数。
3. **预处理不是杠杆**：画布超过训练分辨率（2048/动态）反而单调退化——
   排除"分辨率饥饿可以靠更大画布补救"的假设。
4. **PP-DocLayout 换型理由**（候选 2）：训练域与本基准高度重合（中文文档布局），
   类表含 table / formula（isolated）；获取形态为 Paddle 推理格式
   （inference.pdiparams，hf-mirror.com/PaddlePaddle/PP-DocLayout-L），需
   paddle2onnx 转换后进同一评估器——换型评估与战略关闭的裁决以其实测为准。
5. **若候选 2 亦不过门**：按 D-30 矩阵第一行处置——B-LINE-2 与阶段 C 全部关闭，
   B4.3 降对照复核轮，M4/M5/M6b 记战略关闭；M2 收益（~89.8）不受影响。

## 4. 沿用资产（无论换型成败）

- `scripts/omnidocbench-b0-detect-probe.py`：检测门评估器已固化（贪心 IoU 匹配、
  逐页容错、跳过页落档），换型只换 `--model`。
- 极端长宽比页（报纸 2 万像素长条）会让无界画布的注意力层要求 315 GB 缓冲——
  任何 ORT 候选都必须带画布钳制（本评估器的 [640, min(候选最优, 2560)] 方案可复用）。
