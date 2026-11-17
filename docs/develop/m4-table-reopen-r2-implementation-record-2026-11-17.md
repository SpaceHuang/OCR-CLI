# M4 表格版重开 — R1/R2 实施记录（2026-11-17）

状态：**R2.0–R2.7 全部完成（TDD，门禁全绿）**；R3 评测战役执行中（双扫描后台）。
计划文档：`m4-table-reopen-development-plan-2026-11-16.md`（v1.6，六轮评审收敛）。

## 提交链

| 任务 | 提交 | 内容 |
|---|---|---|
| R2.5 身份字段 | 9311af9 | `reread_prompt_version`/`reread_rules_generation` 进 `OcrIdentity`/`ScanOcrSide`/`scan_side_matches`；三条 resume 不复用回归（prompt 切换/校准代次+1/规范化版本切换=代次+1 契约测试）；serde 默认向后兼容 |
| R2.0 manifest 链 | cd4800e | 磁盘 manifest（真实冻结值：revision `b7366822…`、graph `91457e49…`/1592566B、weights `45404a84…`/212170944B、apache-2.0、runtime{800,255,0.25,21,route A}——当日从 hf-mirror 实取并逐字节验证）；运行时 `LayoutModelManifest`（复用 AssetDescriptor 契约）+ 注册表 + 获取链（钉扎 URL 下载→sha256+尺寸验证→原子提交；算改缓存=硬错误）；`sha256_file` 短长度测试补强（顺带发现测试竞态，实现本身无缺陷） |
| R2.5b CLI 开关 | 2552fc1 | `--table-reread on|off` + `--layout-model`（冻结 v1 仅 pp-doclayout-v2）+ `--layout-regions`（文件或目录）；完整性校验前置；生效值经 manifest 链解析，资产哈希=冻结规则 sha256(manifest 序 role:sha256 行)；`ScanOcrConfig::with_reread` 单一映射（存储身份与预比较侧同源）；result.json `reread` 对象同步；顺手补回有文档无解析的 `--column-split-retry` |
| R2.6 金集防线 | 9d34f00 | `run_pages_with_session` 挂 `Option<&mut dyn RegionProvider>`；`apply_reread_gate` 唯一武装点（四个 Committed 站点）；off=构造前返回（零调用）；`apply_table_reread` 单一后处理产物函数（page_json 解析→编排→splice→page_json 同步→定点 markdown 替换，嵌套感知 k-th `<table>` 片段，缺失降级 warning）；生产解码器 `SessionTableDecoder`（同会话 + 冻结 TABLE_REREAD_V1 + 默认 1024 网格）；计数器链 `PageArtifactResult`→`JobSummary`→result.json pages |
| R2.7 持久化闭环验收 | 940da9d | ①武装未触发=产物逐字节不变（含零解码调用）；②替换 diff 恰为目标表（md 片段交换 + json 块级比对）；③导出器输入侧实测（reread 表被导出）；④checkpoint 哈希=新 markdown 的 artifact_sha256 |
| R3 准备 | 0f64e94 / f294109 / c457074 | regions sidecar（458/458 页，冻结预处理逐字节对齐 B0' 探针，含 mkldnn 禁用修复）；pairing-check regions-dir 模式+逐页判定+跨空间重缩放；B2 五门裁定器+合成夹具单测；provider 目录形态（编排模式按 stem 解析） |

## R3 战役配对状态（regions 模式逐页判定）

- sidecar 区域文件 458/458；配对判定：**384 pair / 41 skip（22 计数 + 16 排序歧义 + 3 去重）/ unsafe 0 / crop-quality 旗标 2** —— 与 R1b 活检测验收同构。
- 安全合格页 = 384 pair 页；门① 分母 = 池表（worst/mid/bad-low）在安全合格页上的成员（D-46）。
- 语料键 → 官方逐表 TEDS 键拼接 **665/665**（经 mapping 原始扩展名重写）。
- T-PROJ 输入齐备：official=round-2 全集逐表/逐样本/逐页明细；baseline=语料对照评测；candidate=语料目标评测；population=r3-b0b-table-corpus。

## 五门（冻结口径，裁定器脚本固化）

① 池表触发覆盖率（安全合格页分母）≥ 80%；② 正常页替换率 ≤ 2%；③ good 带配对均值 Δ ≥ −0.02 且回归（Δ<−0.10）≤ max(3, 2%)；④ worst/bad 带配对均值 Δ ≥ +0.3（安全合格子集）；⑤ T-PROJ 全集投影落档 + 对照正常页 checkpoint 零漂移（vs m23-m2rung-balanced round-2）。

## 冻结修正记录（实施中确定的口径）

- R2.3 重读解码网格 = 默认 1024（平台仅支持 2048 覆盖，属退化阶梯专用）——原"1024 网格"即默认，无需 override。
- `--layout-regions` 目录形态：编排模式每文档解析 `<root>/<stem>.regions.json`（R1 route A 文件形态的多文档推广）。
- sidecar 渲染空间 = scan 渲染空间（round(pt·dpi/72) + 2880² fit，与 `clamped_bitmap_size`/`fit_pixel_budget` 逐条对齐；PyMuPDF 几何等价）。

## R3 战役结果（2026-11-17 判定）：**B2 五门 NO-GO**

双扫描完成（control/target 各 458 页，target exit 10 = BLK-7 部分成功语义）；导出 → docker 官方评测 ×2（r3-control / r3-target，665 表全配对）→ official 明细以同一工具链重评 landing 刷新（r3-official-refresh，逐位复现 88.4484）→ T-PROJ 投影 → 五门裁定（`reports/analysis/analysis-b2-adjudication.json`）：

| 门 | 冻结口径 | 实测 | 判定 |
|---|---|---|---|
| ① 池表触发覆盖率 | ≥ 80%（安全合格页池表） | **58.6%**（99 池表/触发 58；sidecar 配对口径） | FAIL |
| ② 正常页替换率 | ≤ 2%（568 样本） | **3.17%**（18 表） | FAIL |
| ③ good 带回归防护 | 均值 Δ ≥ −0.02 且回归 ≤ max(3, 2%) | 443 表配对，均值 **−0.0035**，回归 **2** | **PASS**（安全网成立：good 表基本无损） |
| ④ worst/bad 增益 | 均值 Δ ≥ +0.3（安全合格子集配对） | 52 表配对，均值 **−0.0027** | FAIL（替换发生处亦无改善） |
| ⑤ 投影+零漂移 | T-PROJ 落档 + 对照零漂移 | Overall 88.4484 → 88.4178（TEDS 腿 −0.0009）；text 375 页配对 Δ **恰为 0** | **PASS** |

**处置（D-30 矩阵）**：B2 NO-GO → **M4 回挂起态**；R4（B4.3 双配置全量复评）分支条件（B2 GO）不成立，**不执行**。损失有界性如设计所证：失败模式 = 不替换，good 带零实质回归（门③），off 路径零漂移（门⑤ text Δ=0）。D-29 允许一轮校准（规则代次 +1）且必须重跑全部门；门④证据（替换发生处 Δ≈0）表明仅调规则的期望收益低——重开需要新的机制假设（如更重读网格/专用表格解码），按计划属新决策。

## R5 回吐定位（已完成，仅分析）

`analysis-giveback-round2.json`：landing 88.45 精确复现；TEDS 亏损 91.8% 集中于 worst/mid/bad-low 池带（bad-low 52.5% + worst 20.9% + mid 18.4%），good 带 8.2%；CDM 亏损 134.0（93 样本 <0.5）、text 112.5。

## 实施期间冻结口径修正（记录在案）

- result.json `reread` 键在 R2.6 首版被 emitting 两次（身份对象 + 计数器对象），last-wins 解析下身份被遮蔽——已修为单一合并对象（本轮战役文件按旧形态解析，计数器读取兼容两种形态）。
- 门① 分母：计划冻结值 146 来自 R1b 活检测验收；战役实际 sidecar 配对口径为 99（生产 provider 的真实安全集）——裁定器按实测计，两口径均入档。
- T-PROJ 的键空间漂移防护按冻结契约执行：baseline/candidate 明细预连接到 official 键空间（连接数 1781/2352 CDM、1557/1557 text、665/665 TEDS），official 以同工具链刷新复现（逐位一致）。


## 2026-11-18：评审 NO-GO（7 阻断）处置 + R3 v2 重跑

评审报告：`docs/review/next-phase-quality-code-review-2026-11-18.md`（基线 `main@6878855`，即 rebase 后）。七项全部修复并提交：

| 阻断 | 修复 |
|---|---|
| BLK-1 runtime 测试编译失败 | 两处 `RenderOptions { dpi }` 补 `..Default::default()` |
| BLK-2 阅读序被丢弃 | `pairing_verdict` 的阅读序现在贯穿 crop 与 splice（编排按 verdict 重排区域）；回归：检测序≠阅读序 + 逐块内容/裁剪顺序断言 |
| BLK-3 游标仅替换时递增 | 序数游标对每个表块无条件递增；回归：健康首表/身份拦截/compare 拒绝三形态 + 评审最小复现 |
| BLK-4 坏页恒触发未实现 | `attempts` 贯穿门与编排，`always_trigger = attempts >= 2` 仅绕过 F1–F5（配对/身份/比较保留）；回归：单元（恒触发不替换）+ 串行坏页端到端 |
| BLK-5 预算绕过 rung | 预算改由**当前 rung 的 `render_budget`** 决定——ovisocr2 四 rung 全带 (4096,4096)（walk-wide 裁定不变），ovisocr3 全 None 保持 plain；回归：ovisocr3 重试零 budgeted 调用 |
| BLK-6 门①/②多表页重复计数 | 扫描落盘逐表明细（`page-NNNN.reread.json`）；门①按 ordinal 精确归因，门②按唯一文档聚合；回归：部分触发/单替换多表页 |
| BLK-7 零漂移门缺失仍 PASS | fail-closed：`checked == 2×stems && missing == 0`；回归：缺文件/缺整文档/全矩阵 |

**R3 v2 重跑**（修复后二进制，control2/target2 各 458 页）：门① **62.6%**（精确归因，此前 58.6% 为页级高估）< 80% FAIL；门② **2.46%**（唯一文档聚合，此前 3.17% 为重复计数）> 2% FAIL；门③ PASS（443 表 −0.0035/回归 2）；门④ −0.0027 ≪ +0.3 FAIL；门⑤投影已落档（−0.031，text 375 页零漂移再次精确成立）但零漂移腿 **fail-closed FAIL**——2 个空输出页漂移，根因 = main PR #13/#14（空输出进阶梯，rebase 继承，证据：landing attempts=1 空产物 vs control2 attempts=5/2），非 M4 变更（评审修复在 off 路径静态惰性）。

**裁定不变：NO-GO → M4 挂起，R4 不执行。** 产物：`analysis-b2-adjudication-v2.json` / `analysis-b2-tproj-v2.json`（含归因段）。
