# T-PROJ 配对投影工具对账记录（2026-11-15）

**任务：** next-phase 开发方案 §4.0 T-PROJ（BLK-4/D-31）——配对投影工具扩展
**状态：** 完成（单测 + 三个手工核对待验证样例 + 旧 θ 场景逐字节回归锁）

## 1. 交付形态

- **新脚本** `scripts/omnidocbench-paired-projection.py`：配对增量模式（D-31 ②）。
  旧脚本 `scripts/omnidocbench-projection.py` 保持逐字节不动（D-31 ① 由构造保证），
  另加回归锁测试防漂移。
- 选择新脚本而非就地扩展的理由：配对模式与 θ 场景的输入契约完全不同（θ 场景只读
  单 profile 报告 + export-report 异常账本；配对模式需要基线/候选双份官方同粒度明细
  + 冻结人群规格），就地共享 CLI 会把两种契约耦合在参数面上。

## 2. 冻结契约

输入：
- 官方 all 粒度明细 ×3（分母冻结：text 1,557 页 / CDM 2,352 样本 / TEDS 665 表格样本；
  以文件实际 n 为准，非硬编码）；TEDS 明细接受官方形状 `{"TEDS": x, "TEDS_structure_only": y}` 或裸浮点。
- 基线/候选同粒度明细 ×3（子集语料官方评估产物，键空间必须与官方 all 键一致）。
- 冻结人群规格 JSON：`{"schema_version": 1, "population_id": ...,
  "text_pages": [stem...], "tables": [样本键...], "formulas": [样本键...]}`（三个键均可选）。

算术（与官方评分器同构，D-11）：
- 文本分量 = 1 − mean(edit)；Overall = ((1 − edit_mean) + CDM + TEDS)/3 × 100。
- 投影：`projected = (Σ_all 基线 + Σ_人群 (候选 − 基线)) / N_official`。

错误路径（响亮失败，不静默）：
- 人群键不在官方 all 明细 → `ProjectionError`（分母漂移）。
- 基线键不在官方 all 明细 → `ProjectionError`。
- 候选缺人群键 → 默认 `ProjectionError`；`--allow-missing-candidate` 时按零增量计并计数
  `missing_candidate_n`（沿用三臂报告 missing_arm_n 口径）。

输出：JSON（`--output`）+ 人类摘要 stdout。字段：
`components.{text,cdm,teds}.{official_n, population_n, paired_n, missing_candidate_n,
official_all_mean, projected_all_mean, paired_mean_delta, population_delta_sum,
official_component, projected_component, delta_component}` 与
`overall.{official, projected, delta}`。

## 3. 三个手工核对待验证样例（手算值冻结于测试）

| 样例 | 构造 | 手算 | 断言（测试名） |
|---|---|---|---|
| M2 类页级 | 官方 3 页 edit 0.2/0.3/0.4；人群 {a,b}；候选 a 0.2→0.1、b 不变 | 配对均值 Δedit = −0.05；投影 edit 均值 = (0.9−0.1)/3 = 0.26̄；ΔOverall = ((1−0.8/3)−0.7)/3×100 = +1.111 | `M2 paired mean delta edit` 等 13 项 |
| M4 类逐表 | 官方 4 表 0.5/0.6/0.7/0.8；人群 {t1,t3}；候选 t1 +0.4、t3 −0.1 | 配对均值 ΔTEDS = +0.15；投影均值 = (2.6+0.3)/4 = 0.725；ΔOverall = 0.075/3×100 = +2.5 | `M4 paired mean delta TEDS` 等 4 项 |
| M5 类逐样本 | 官方 3 样本 0.2/0.9/0.9；人群 {c1}；候选 c1 0.2→1.0 | 配对均值 ΔCDM = +0.8；投影均值 = 2.8/3；ΔOverall = (0.8/3)/3×100 = +8.888… | `M5 paired mean delta CDM` 等 3 项 |

错误路径用例：基线键越界 / 候选缺键默认失败 / allow-missing 计数与零增量（4 项）。
CLI 端到端：合成树 9 份明细 + 人群 JSON → exit 0 且与库函数结果一致（2 项）。

## 4. 回归锁（D-31 ①）

`scripts/test_omnidocbench_projection_regression.py` 在合成 benchmark-root 树上运行
旧脚本，stdout 与冻结金样逐字节比对（含中文标签、θ 表、88/85 需 θ 注记）。
金样捕获自当前实现（characterization lock）；旧脚本此后任何 stdout 变化都会失败。

## 5. 运行方式

```
python scripts/test_omnidocbench_paired_projection.py      # 配对模式：26 项 check
python scripts/test_omnidocbench_projection_regression.py  # 旧 θ 场景逐字节锁
```

## 6. 与裁决门的接口

- B-M2.3 口径澄清：B-M2.3 本身即全集口径（全量重扫直读官方指标），不依赖本工具。
- B2 门⑤ / B3 门⑤ / C1 产出⑤ / C2 闸门⑤：人群规格 = 对应 B0b 冻结子集的样本键清单；
  基线/候选明细 = 对照/目标配置官方同粒度评估产物；命令与人群清单落 runbook（B4.1）。
