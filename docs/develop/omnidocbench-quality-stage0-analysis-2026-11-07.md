# OmniDocBench 质量优化 · 阶段 0 分析结论（Q2.1 / Q5.1 / Q5.2 / Q6.1）

**日期：** 2026-11-07　**分支：** `feature/quality-optimization-tdd`　**数据冻结：** balanced 基线（`benchmarks/omnidocbench/manifest.json`，repo `1d6d109`，二进制 `f3b6e068`）
**脚本：** `scripts/analyze-omnidocbench-quality.py`（q51/q52/q21/q61 子命令）；机读数据 `benchmarks/omnidocbench/reports/analysis/analysis-*.json`（已入库）

---

## Q1.0 语料构建（已完成，`assets/regression/omnidocbench-degeneration-v0.1/`）

- 213 页退化语料（含 5 退化∩截断交集）冻结入库；账本与冻结 export-report 精确对账（213/33/5/28/1），构建脚本不一致即失败。
- `scripts/omnidocbench-projection.py` 入库并核实：77.9909 / θ=0.5→84.5674 / θ=1→91.1439，与方案 §1.1 逐项一致（D-11 唯一数字来源生效）。
- 子集评分包：真值子集 + 子集预测导出 + 一键评分脚本（`benchmarks/omnidocbench/degeneration-v0.1/`，gitignore 侧，数据集 research-only）。
- **渲染降采样联查（Q1.0 附带）**：语料 88/213 页（41.3%）的 300dpi 渲染请求超 2880² 像素预算被 PDFium 等比缩，全集仅 171/1650（10.4%）——**4.0× 富集，渲染降采样与退化强相关**（报纸大版面是主要载体，见 Q2.1）。
- 待办：基线二进制对语料确定性复扫（与 Q1.1 实验首轮合并执行，GPU）。

## Q2.1 报纸退化形态分类学（92 页）

| 维度 | 结果 |
|---|---|
| 环内容类型 | **line_fragments 88 / table_html 4 / formula 0 / bbox 序列 0** |
| 多栏版面（GT 栏估算 ≥2） | 90/92（98%） |
| 跨栏图片页 | 84/92（91%） |
| 渲染超预算被缩 | 85/92（92%） |

**对 Q1.1 阶梯排序的含义：**
1. 报纸环是**行片段复读**而非表格 HTML——repeat-penalty 采样档（Q1.2）对报纸可用中高惩罚档，低惩罚保留给 4 个 table_html 环页（MinerU 风险点仅覆盖 4%）。
2. **切栏候选（Q1.1c）适用面受限**：91% 跨栏图片页是切栏已知失效形态，适用页清单需逐页核对后才能计入预期收益；切栏不能作为报纸主杠杆。
3. token 2048 档（候选 D）与报纸高度对症：92% 渲染超预算 → 像素栅格密而 token 网格饿（1024），与 E1 实测 25% 边际逃逸一致。

## Q5.1 纯截断页三向特征化（28 页）

| 形态 | 页数 | 说明 |
|---|---|---|
| loop_numeric_increment（数字递增环） | 19 | 如普特南题号 "2857.\n\n2858.\n\n…"——数字递增使环检测相似度判据失效 |
| loop_exact_period（严格周期环） | 2 | 周期 119 / 345 字符，超 p≤64 token 窗口 |
| loop_suspect_weak（弱周期嫌疑） | 3 | 掩码后 ≥3 周期重复 |
| true_long_content（真长内容） | 4 | 长试卷/长报纸，非周期 |
| budget_edge | 0 | — |

**裁决：环漏检形态占比 24/28（86%）——"可观"成立，Q1.5 条件立项触发**（方案 §4.1 Q1.5）：需要显式修改重试触发条件（`truncated` 进入阶梯末档采样重试）、`--resume` disposition 与场景 C 测试更新，并出独立决策记录。4 页真长内容维持 D-6 接受残余。

## Q5.2 近纯图页空输出特征化（balanced）

- true_empty_model_output 22（raw 与 md 均空）+ tiny_output 23（<64 字符片段）= 45 页。
- **无 think-only、无适配器误伤、无路由问题。裁决：模型行为（接受）**，转记录项，不立项。

## Q6.1 en 落差分类学

- **公式部分：按 E2 关闭**（token 2048 已消除 en 公式落差，0.7624 → 0.9365，"结构性落差"为分辨率饥饿伪影），不再对 1024 输出做公式分类学。
- **表格部分（en 最差 50 页，官方 per-table TEDS 页均值口径）**：missing_or_dropped_table 26 / transcription_suspicion 19 / representation_suspicion 5。
- **裁决：表示类仅 10%，D-9 杠杆不成立——不立项表示规范化**；缺表 26 页中可辨认的退化页修复后自然回收，剩余记为模型本征差异。MinerU ADR/OTSL 借鉴方向维持"后续立项参考"。

---

## 汇总：对阶段 1（Q1.1）的输入

1. 阶梯排序输入（类别感知）：**公式/学术/试卷类 token 档提前（E2 79%）**；报纸行片段环靠后（E1 25%）；切栏仅对适用页清单启用（Q2.1：跨栏图片页 91%，适用面待逐页核对）。
2. 惩罚档位建议：报纸 line_fragments 环可用中高惩罚；table_html 环（语料中 4 页报纸 + 学术公式环）低惩罚档。
3. Q5.1 触发 Q1.5：截断页进入阶梯末档的采样重试需要独立决策与测试变更（场景 C）。
4. 渲染降采样富集（4×）进一步支持候选 D 优先级，但**不构成改全局默认 1024 的论据**（E1/E2 收益分解铁据不变）。
