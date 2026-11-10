# OmniDocBench 里程碑复评：组合栈（0.1.161 + 组合阶梯）实测

**日期：** 2026-11-10　**分支：** `scratch/q12-sampling-escape`　**对照基线：** `omnidocbench-baseline-2026-11-06.md`（77.99）
**复现命令：** `benchmarks/omnidocbench/scans/0161-full-balanced` + `predictions/0161-full-balanced`，官方 `pdf_validation.py` quick_match（Docker 镜像同基线 manifest）

## 1. 头条数字

| 切分 | 基线 | 组合栈 | 增量 |
|---|---:|---:|---:|
| **all（1,651 页）** | **77.99** | **84.92** | **+6.93** |
| en（755 页） | 70.72 | **81.58** | **+10.86** |
| zh（765 页） | 85.82 | 86.83 | +1.02 |

GO 线（≥85）判定：**84.92，差 0.08**——严格未达标，但已进入噪声区间；且 en 收益 +10.86 为全项目最大单项切分增量。

## 2. 分量实测

| 分量 | 基线 | 实测 | 增量 | 投影对照 |
|---|---:|---:|---:|---|
| 文本 (1−edit) | 0.8461 | 0.8864 | +4.0 | 投影 +3.6（**超预期**：正常页漂移净正） |
| 公式 CDM | 0.7925 | **0.9203** | **+12.8** | 投影 +8.3（**大幅超预期**） |
| 表格 TEDS | 0.7011 | 0.7410 | +4.0 | 投影 +4.2（符合） |

en 切分：公式 CDM 0.7624 → **0.9173**——**en 公式落差正式消除**（E2 结论在全集兑现）；en 文本 0.8467 → 0.9172。

## 3. 正常页漂移净效应（0.1.161 采用的关键问题）

投影（83.64）只计语料页收益；实测（84.92）高出 **+1.28**——差额即**正常页在 0.1.161 下的净漂移为正**（金集的伪影样本是少数派；全集口径下公式/报纸正常页普遍改善）。**0.1.161 采用的正常路径风险实测解除。**

## 4. 产品契约验收（随本轮复评一并落定）

- 退化账本：213 → **122**（-43%）；截断 33 → 34（+1，纯截断页 D-6 口径不变）
- `recognition_failed`：1 → **0**（Q4 链接降级生效）
- 导出 `think_blocks_stripped_total`：**0**（Q3 产品侧剥离生效，导出脚本对账为第二实现）
- 评测纪律：门禁绿构建 + manifest 冻结（Q7.1 runbook）全程执行

## 5. 残余与证伪链（D-6 接受的实证背书）

122 页残余 = 重复五连证伪（plain/2048/penalty/切栏/4032）后确认的模型读取边界：

| 类别 | 页数 | 病因 |
|---|---|---|
| newspaper | ~54 | 马赛克拼版/对开名单（几何切栏失效样本） |
| research_report | 12 | 0% 逃逸（完全抵抗类） |
| academic/exam | 27 | 密集公式读取（切栏有害，4032 无响应，4%） |
| 其他分散 | ~29 | — |

**v2 指向**：神经版面模型（MinerU/PP-DocLayout 路线，治报纸拼版）+ 公式区域裁剪重读（MinerU ADR，治公式密度）+ 栈对齐（剩余 ~11.7 分的推理栈本征部分）。

## 6. 裁决建议

1. **0.1.161 采用：GO**——正常页漂移净正实测、公式 +12.8 的主贡献者、per-sequence sampler 的唯一载体。落地前置项：`;decode=<gen>` 代次键 + 新旧不混扫回归 + checkpoint 兼容参数更新（D-13 规则 3）。
2. **组合阶梯合入：GO**——`[2048 → penalty 1.2×/64 → 切栏(类型门控:报纸类)]`，cap=4；扰动档除名（双零证据链）。
3. **Q1.5（截断页入阶梯）**：随 Q1.3 一并落地（Q5.1 触发条件已成立，D-14）。
4. **85 线处置**：84.92 距线 0.08，属噪声级；不建议为凑线追加高风险动作——v2 版面模型是下一个真实增量位（报纸质量缺口 ~+1.7 起步）。


## 7. 最终交付阶梯验证（2026-11-10 追加，D-16 量化收口）

扫描 `scans/0161-full-balanced-finalladder`（最终合并候选阶梯 [2048 → penalty 1.2×/64 → 切栏]，cap=4；
二进制 SHA `318f44ea…`，manifest `manifest-0161-final.json`）：

| 分量 | 里程碑（实验栈） | **最终阶梯实测** |
|---|---:|---:|
| 文本 (1−edit) | 0.8864 | **0.8961** |
| 公式 CDM | 0.9203 | **0.9223** |
| 表格 TEDS | 0.7410 | **0.7607** |
| **Overall** | 84.92 | **85.9707** |

**GO 线（≥85）：达标，余量 +0.97。** 退化账本 122 → 106（切栏档 -16）；incomplete 0；导出 think_stripped 0。
组合栈判据（D-16）量化成立；Q1.2 采样单项 NO-GO 维持原判。


## 9. O-4 四档终评（2026-11-10，方案完全闭环）

合并候选（`feature/q13-sampling-landing`，decode=2 代次键 + 组合阶梯 + Q1.5）四档全量实测（n=1651/档）：

| 档位 | 基线 | 终评 | 增量 |
|---|---:|---:|---:|
| compact (Q4) | 75.59 | 83.61 | +8.02 |
| balanced (Q5) | 77.99 | **87.85** | +9.86 |
| quality (Q8) | 77.67 | 86.14 | +8.47 |
| reference (F16) | 75.85 | **86.69** | **+10.84** |

- **四档全部 +8 以上**：治理收益与量化档位无关（"量化不是瓶颈"结论在新栈下复核成立且更强化——基线极差 2.4 → 终评极差 4.24，量化排序不再单调）。
- **reference 增量最大（+10.84）**：F16 全精度权重 + 2048 档的保真度组合收益最高，与 E2 分辨率饥饿结论同向。
- 选档结论维持：balanced 仍是质量/速度最优推荐（87.85 为四档最高）。
- 官方产物：`reports/omnidocbench-q13-final-{compact,quality,reference}_quick_match_metric_result.json`（balanced 见 O-3）。
- **方案开放项全部关闭**（§7A O-1~O-5）。剩余为 v2 重构（已 GO：pins 修订落 main `7e945c3`）与已论证的 v2 战略方向。


## 10. 证据追溯索引（v1.5.3，评审 BLK 解除补充）

O-3/O-4 验收数字的可追溯证据链（全部在本分支归档）：

| 声明 | 证据载体 |
|---|---|
| 运行身份（源码 commit/二进制 SHA/pins/参数） | `benchmarks/omnidocbench/reports/q13-acceptance-manifest.json`（source_commits: code_effective=c3a1d31, docs-only→9e0dc29；二进制 SHA 04306fe7；dataset revision；阶梯配置；export/eval 环境与命令） |
| 截断 38→7 / decode=2 覆盖 1651 / incomplete 0 | `q13-acceptance-manifest.json` scan_ledgers（自 checkpoint 复算）+ 四份 `export-report.json` + 四份 `scan-summary.json`（已归档，官方导出器产出） |
| think_blocks_stripped_total = 0 | 同上 export-report.json（每页与总量字段） |
| 门禁（27 套件/fmt/clippy） | `benchmarks/omnidocbench/reports/q13-gates-record.json`（source_head 9e0dc29 + 二进制 SHA） |
| 官方评分复算 | `omnidocbench-q13-landing-all` / `-q13-final-{compact,quality,reference}_quick_match_metric_result.json`（已归档） |
