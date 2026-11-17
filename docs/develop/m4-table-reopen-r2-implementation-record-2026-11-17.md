# M4 表格版重开 — R1/R2 实施记录（2026-11-17）

状态：**R2.0–R2.7 全部完成（TDD，门禁全绿）**；R3 评测战役执行中（双扫描后台）。
计划文档：`m4-table-reopen-development-plan-2026-11-16.md`（v1.6，六轮评审收敛）。

## 提交链

| 任务 | 提交 | 内容 |
|---|---|---|
| R2.5 身份字段 | 9311af9 | `reread_prompt_version`/`reread_rules_generation` 进 `OcrIdentity`/`ScanOcrSide`/`scan_side_matches`；三条 resume 不复用回归（prompt 切换/校准代次+1/规范化版本切换=代次+1 契约测试）；serde 默认向后兼容 |
| R2.0 manifest 链 | 0f4a5b7 | 磁盘 manifest（真实冻结值：revision `b7366822…`、graph `91457e49…`/1592566B、weights `45404a84…`/212170944B、apache-2.0、runtime{800,255,0.25,21,route A}——当日从 hf-mirror 实取并逐字节验证）；运行时 `LayoutModelManifest`（复用 AssetDescriptor 契约）+ 注册表 + 获取链（钉扎 URL 下载→sha256+尺寸验证→原子提交；算改缓存=硬错误）；`sha256_file` 短长度测试补强（顺带发现测试竞态，实现本身无缺陷） |
| R2.5b CLI 开关 | 2552fc1 | `--table-reread on|off` + `--layout-model`（冻结 v1 仅 pp-doclayout-v2）+ `--layout-regions`（文件或目录）；完整性校验前置；生效值经 manifest 链解析，资产哈希=冻结规则 sha256(manifest 序 role:sha256 行)；`ScanOcrConfig::with_reread` 单一映射（存储身份与预比较侧同源）；result.json `reread` 对象同步；顺手补回有文档无解析的 `--column-split-retry` |
| R2.6 金集防线 | 9d34f00 | `run_pages_with_session` 挂 `Option<&mut dyn RegionProvider>`；`apply_reread_gate` 唯一武装点（四个 Committed 站点）；off=构造前返回（零调用）；`apply_table_reread` 单一后处理产物函数（page_json 解析→编排→splice→page_json 同步→定点 markdown 替换，嵌套感知 k-th `<table>` 片段，缺失降级 warning）；生产解码器 `SessionTableDecoder`（同会话 + 冻结 TABLE_REREAD_V1 + 默认 1024 网格）；计数器链 `PageArtifactResult`→`JobSummary`→result.json pages |
| R2.7 持久化闭环验收 | 940da9d | ①武装未触发=产物逐字节不变（含零解码调用）；②替换 diff 恰为目标表（md 片段交换 + json 块级比对）；③导出器输入侧实测（reread 表被导出）；④checkpoint 哈希=新 markdown 的 artifact_sha256 |
| R3 准备 | e58893c 等 | regions sidecar（458/458 页，冻结预处理逐字节对齐 B0' 探针，含 mkldnn 禁用修复）；pairing-check regions-dir 模式+逐页判定+跨空间重缩放；B2 五门裁定器+合成夹具单测；provider 目录形态（编排模式按 stem 解析） |

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

## 未决

- R3 双扫描（control 458 页 / target 458 页）后台执行中；完成后自动链：导出 → docker 官方评测 ×2 → T-PROJ 投影 → 五门裁定（`analysis-b2-adjudication.json`）。
- B2 GO → R4（B4.3 双配置全量复评，目标 ≥91 分支）；B2 NO-GO → M4 回挂起态（D-29 允许一轮校准，校准需重跑 R3 全部门）。
- R5（CDM/TEDS 回吐定位）按 D-43 仅分析，可与 R3 扫描并行推进。
