# B0' 版面检测前置——最终裁决（2026-11-15，四候选实测收口）

**检测门（B1 硬前置，冻结判据）：** 表格/公式区域召回 ≥ 90%（B0a 冻结 744 页子集，GT IoU ≥ 0.7，conf ≥ 0.25）
**裁决：** **NO-GO**——四候选全部过表格门（最新 97.3%），公式召回全部停在 ~60% 平台期，无一过门。
**后果（D-30 矩阵第一行）：** B-LINE-2 与阶段 C 关闭；B4.3 降对照复核轮；M4/M5/M6b 记战略关闭；M2 收益不受影响。

## 1. 候选实测汇总（744/744 页，0 错误 0 跳过）

| 候选 | 运行时 | 输入 | 表格 recall / precision / IoU | 公式 recall / precision / IoU | 表格门 | 公式门 |
|---|---|---|---|---|---|---|
| DocLayout-YOLO DocStructBench (wybxc ONNX) | ORT 1.31.1 CPU | 1024 钳 1280 letterbox | 0.826 / 0.888 / 0.963 | 0.481 / 0.841 / 0.921 | ✗ | ✗ |
| PP-DocLayout-L (paddle 3.4 PIR) | paddle CPU | 640×640 拉伸 | 0.860 / 0.812 / 0.961 | 0.590 / 0.096 / 0.850 | ✗ | ✗ |
| PP-DocLayout_plus-L | paddle CPU | 800×800 拉伸 | 0.946 / 0.823 / 0.964 | 0.618 / 0.107 / 0.854 | ✓ | ✗ |
| **PP-DocLayoutV2** | paddle CPU | 800×800 拉伸 | **0.973** / 0.828 / 0.966 | 0.599 / **0.748** / 0.904 | ✓ | ✗ |

预处理敏感性数据点：DocLayout-YOLO 画布 2048/动态（2560 护栏）单调退化（82.6%→64.4%→50.7%）；
plus-L letterbox 无实质变化（表格 95.8%、公式 60.4%）——公式缺口不在预处理几何。
产物：`reports/analysis/analysis-b0-detect-gate{,-2048,-2560,-ppdoclayout,-ppdoclayout-plus,-ppdoclayout-plus-kr,-ppdoclayout-v2}.json`。

## 2. 公式 ~60% 平台期的定性

1. **跨架构一致**：YOLO 系与两代 RT-DETR（PaddleX 官方主推的 plus/V2）全部停在
   0.48–0.62 区间；plus→V2 的公式 precision 从 0.10 升至 0.75（检出的都是真公式），
   recall 不动——缺的是**检出**不是精度。
2. **GT 语义差**（待重开时核验）：GT 公式 2,226 = equation_isolated 2,066 + equation_semantic 160；
   官方评估分母 2,352（工具链增广）。检测器类表只有 display/isolate formula——
   多行公式块、公式组、题注粘连的 GT 切分与检测器框法可能系统性错位
   （V2 匹配 IoU 0.904 偏低亦支持此说）。
3. **人群自查与 spike 一致**：漏检页集中在 PPT/讲义类版式（187 页有漏检）。

## 3. 裁决依据与后果

- 判据冻结在先（方案 §4.2 B0'：召回 ≥ 90% @ IoU 0.7 是 B1 硬前置），四候选实测
  均未达标——按方案"不过门 → B-LINE-2 与阶段 C 全部关闭"执行。
- **M2（已落地）不受影响**：rung 在重试路径，与区域来源无关；B-M2.3 的
  修复验证（retry-wide 光栅）独立推进。
- **重开条件**（方案 §8/§4.2）：新版面检测能力证据（公式召回 ≥ 90% 的候选，
  或 GT 公式语义与检测类定义对齐后的复评）；候选评估器
  `scripts/omnidocbench-b0-detect-probe{,-paddle}.py` 与 B0a/B0b 冻结资产全部可复用。

## 4. 沿用资产清单

- `benchmarks/omnidocbench/region-truth/`（B0a 冻结，744 页 GT 区域 + 类内序数）
- `benchmarks/omnidocbench/b0b-corpus/`（B0b 冻结，分带/断裂/保护集/账本）
- `scripts/omnidocbench-b0-detect-probe.py`（ORT 评估器）/
  `scripts/omnidocbench-b0-detect-probe-paddle.py`（paddle 评估器，
  含坐标交换修正与 V2 8 列行兼容）
- 模型资产：`C:/t/m23/b0-models/`、`C:/t/m23/b0-ppdoclayout{,-plus,-v2}/`
