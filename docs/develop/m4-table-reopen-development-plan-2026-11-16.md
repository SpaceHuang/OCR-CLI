# M4 表格版重开开发方案（B-LINE-2 部分重开：表格区域重读）

**文档状态：** v1.6（2026-11-17——五轮重审响应：内容身份检查完整契约冻结；v1.5 四轮重审）
**创建日期：** 2026-11-16
**数据依据：** B0' 四+一候选实测（`b0-detect-gate-verdict-final-2026-11-15.md` 及归档 JSON：PP-DocLayoutV2 表格召回 97.3% @ IoU 0.7，744 页冻结子集 0 错误 0 跳过）；B-M2.3 两轮判定（`bm23-milestone-reeval-record-2026-11-15.md`：88.45，净 +0.60，text +1.87pt 兑现）；M4 oracle spike（`omnidocbench-m4m5m3-crop-prompt-spike-2026-11-14.md`：区域效应 2× 纯 token，crop@1024 饱和）
**关联计划：** `omnidocbench-quality-next-phase-development-plan.md`（v1.3——本方案为其 B-LINE-2 的部分重开修订，其余条款继承）
**方案性质：** 独立可执行。范围 = 仅 M4 表格版；M5（公式重读）与 M6b（坏页 text 管线）**维持关闭**（公式五模型 ~60% 平台期，重开条件另附）。

---

## 1. 修订背景与证据链

### 1.1 B0' 联合门拆分依据

原方案 B0' 检测门为联合判据（表格/公式召回同时 ≥90%）。本轮四+一候选实测结果：

| 候选 | 表格门（≥90%） | 公式门（≥90%） |
|---|---|---|
| DocLayout-YOLO DocStructBench（ORT） | 82.6% ✗ | 48.1% ✗ |
| PP-DocLayout-L | 86.0% ✗ | 59.0% ✗ |
| PP-DocLayout_plus-L | 94.6% ✓ | 61.8% ✗ |
| **PP-DocLayoutV2** | **97.3% ✓**（precision 0.83，IoU 0.966） | 59.9% ✗（precision 0.75） |
| Pix2Text MFD 1.5（专用公式检测器，AGPL） | — | 58.5% ✗ |

**结论：** 联合门的公式侧是**基准 GT 公式语义与区域检测范式的系统性错位**（五模型两范式一致平台期，precision 升高 recall 不动 = 缺检出），非模型选型可解；表格侧 V2 以 97.3% 显著过门。据此将 B0' 拆分为**表格门（PASS，重开 M4）/ 公式门（NO-GO，M5/M6b 维持关闭）**，符合方案原重开条款（"重开条件 = 新版面检测能力证据"）。

### 1.2 M2 单位错误教训（本方案投影纪律的来源）

B-M2.3 复盘确认：spike 报告的"+2.0"实为 **text 分量点数**（未除以 3 换算 Overall），导致中继门按 89.5 设置而杠杆真实上限 ~88.5。本轮 M4 重开的全部投影**必须**：
1. 给出分量级算术（每个门的 Δ 落在哪个分量、除几）；
2. 全量复评前不给 Overall 承诺（分量连带预案：M2 教训 = retry-wide 2048 使 CDM/TEDS 回吐 0.47pt，占投影 23%）；
3. B2 五门以 T-PROJ 配对算术为准（D-11），仅报按页均值视为未完成。

---

## 2. 目标与非目标

### 2.1 目标

1. **主目标**：B2 五门全部通过 → M4 表格版转正 → 按 D-30 既有分支 **"B2 GO + B3 NO"** 冻结目标配置 `--table-reread on --layout-model pp-doclayout-v2`，B4.3 双配置复评判定 **目标 ≥ 91**。
2. 分量投影（诚实口径，非承诺；v1.2 按 R1b 安全覆盖修订）：门④人群 = worst 36 + bad-low 97 = **133 表**，其中安全合格覆盖 146/222 池表（worst 22 + bad-low 51 = 73 表，55%）。若合格子集 ΔTEDS +0.3 全兑现且未覆盖部分零变化：ΔTEDS ≈ 73×0.3/665 = 0.033 → Overall ≈ **+1.1 → 89.55**；真实值取决于合格子集上的实际 Δ 分布与未覆盖表格的零变化假设（保守）。**≥91 需要更多**；判定只由 B4.3 实测给出；投影算术与样本数以 T-PROJ 配对结果为准。
3. 每个门携带冻结判据，一级入账一级；失败即按既有降级路径回退（M4 回到"机制 GO / 产品未裁决"挂起态，损失有界）。

### 2.2 非目标

- M5/M6b 不重开（公式区域系）；M4 的区域提供者即使检出公式区域，**不产出公式重读**（范围扩大禁令沿用原方案 D-26）。
- 全局默认参数不动（token 1024 / 渲染预算 2880² / prompt 主契约）；表格重读裁剪 token = 1024（D-21 冻结）。
- 不动 M2 已落地形态（retry-wide 光栅 × walk-wide 2048；其 CDM/TEDS 回吐定位为并行有界实验 R5，不阻塞本方案）。
- 缺表救援（detection_N > output_N）仍不在范围（O-5）。

---

## 3. 任务分解

### 3.1 R0：门拆分修订说明（本文档）+ B0' 记录更新

落点：本文档 + `b0-detect-gate-verdict-final` 追加修订段。
验收：表格门/公式门拆分依据与五模型数据入档；D-30 矩阵第一行改注（区域系拆分：表格门过 → M4 可单独推进；公式门不过 → M5/M6b 维持关闭）。

### 3.2 R1：运行时路线裁决（O-1 落地）

**问题**：扫描管线（Rust）如何在页级拿到 V2 的表格区域？

| 路线 | 形态 | 优势 | 劣势 |
|---|---|---|---|
| A. paddle sidecar | Python 子进程常驻，页位图进/区域 JSON 出（stdin/stdout 或文件） | 复用已验证的探测代码路径（坐标交换修正已固化）；零转换风险 | 发行依赖 paddle runtime（~600MB） |
| B. ONNX 转换 + ORT | paddle2onnx → ORT C++ 推理 | 发行干净（ORT ~20MB） | V2 转换可行性未知（PIR 导出、8 列输出自定义 decode 需 C++ 重写） |

**裁决判据（实测）**：页级推理延迟（候选 A 已有数据 ~2.5s/页 CPU）× 集成成本 × 转换风险。预注册裁决线：路线 B 转换成功且 ORT 推理与 paddle 输出**规范化后完整区域序列一致**（class、index_in_class、bbox、conf 逐项一致；仅比框 IoU 不够——评审 BLK-1）→ 选 B；否则选 A，发行形态（可选依赖 vs 内置）随 R3 结果再定。
落点：spike 分支（scratch）+ 裁决记录落 `docs/develop/`。
验收：裁决记录含双路线实测数据与选择依据；**区域规范化契约**随裁决一并冻结（见 R1b）。

**R1b：区域规范化契约冻结（评审 RBLK-1 解除条件；v1.2 按实测数据重写）**
- 类映射：模型 class 21（table）→ `RegionClass::Table`；其余类丢弃（本方案范围外）。
- 置信度过滤：conf ≥ 0.25（与门评估口径一致，冻结值）。
- **阅读序 = 双解释一致规则（v1.1 的 `(y,x)` 字典序已被完美框实验证伪并废止）**：
  1. 行解释：y-overlap 带聚类（重叠 ≥30% 归带），带按 y 排序、带内按 x 排序；带 y 范围两两不相交才算"行干净"。
  2. 列解释：x-overlap 列聚类（对称定义）；列按 x 排序、列内按 y 排序；x 范围两两不相交才算"列干净"。
  3. **页面合格条件 = 行、列两个解释都干净且产生相同顺序**（加连续对 y-overlap ≥30% 否决）。否则页面标记 `ordering_ambiguous`，**运行时跳过该页重读**（保留原输出，永不重读）。
- **验证（B0a 完美框下界，工具 `scripts/omnidocbench-region-ordinal-check.py`，产物 `analysis-b0-ordinal-agreement.json`——分桶口径与评审一致）**：117 个多表页 → **合格（重读）95 页序数全部正确**（合格页序数正确率 100%）、**歧义跳过 22 页**（运行时永不重读）、**unsafe（重读但序错）= 0**。v1.1 的 `(y,x)` 规则实测 85.5% 一致（14.53% 错序，评审复测一致），rowgroup 88.9%——均废止。
- **整页跳过规则（积分制，可执行参考实现）**：重读合格性要求 `检测表格数 == 输出表格块数` **且本页未发生去重** 且双解释一致——漏检/中间多检/去重孔洞都会使孔洞后的序数整体错位，任何一项违反即整页跳过（保留原输出）。规则以可执行 Python 参考实现冻结（`scripts/omnidocbench-region-pairing-check.py` 的 `pairing_verdict`，R2.4 按此移植 Rust）。
- **真实 V2 输出验收（替代 v1.3 的合成扰动——评审三轮裁定合成模拟无效，实锤成立）**：`analysis-b0-pairing-safety.json`（744/744 页，0 错误）——**配对成功 699 页身份错配 = 0**；漏检 6 / 多检 20 / 去重 3 / 排序歧义 16 / 计数不匹配 26 全部整页跳过（0 漏网）。三态（miss/multi/dedup）由真实检测行为自然涌现，不再合成。
- **预注册硬门（v1.5 最终形态；v1.4 的门②把裁剪质量指标混入配对身份——范畴错误，本轮拆分）**：
  - **① 完美框门**：unsafe = 0 且合格页序数正确率 = 100%（`analysis-b0-ordinal-agreement.json`）；
  - **② 配对身份门（真实 V2 输出，防错表替换的硬门）**：配对成功页上，生成位置 k 的检测框最佳 IoU 匹配的 GT 表序数必须 == k（**不含 IoU 下限**——身份是序数语义，IoU 是另一失效模式）；实测 **699 配对页身份错配 = 0**；
  - **②b 裁剪质量（机制防护项，非离线门）**：身份正确但框重叠 < 0.7 的页（实测 2/744 = 0.27%，超集框含题注/边带）——替换时新增**内容身份检查**（R2.4：重读表与原块文本重叠 ≥ 冻结阈值才进入 compare-keep，否则保留原表）+ 冻结 compare-keep + B2 门③实测回归封顶；两页显式入档 `crop_quality_below_0_7`；
  - **③ 变更纪律**：任何规则或验收器修改必须重过全部门；
  - **hard_gate.pass = 门① AND 门②**（AND 在 `omnidocbench-region-pairing-check.py` 中实际实现——读取完美框产物并求合）。
- **覆盖代价（诚实核算，B0b 冻结语料；可复现产物由工具自动生成于 analysis JSON 的 `b0b_pool_coverage` 段）**：worst 22/36（61.1%）、mid 73/89（82.0%）、bad-low 51/97（52.6%）、good-en 107/120、good-zh 278/297、good-other 25/26；**裁决池（worst+mid+bad-low）合计 146/222 = 65.77%**。
- A/B 路线一致性 = 规范化后完整区域序列逐项比对（class、ordinal、bbox、conf；bbox 逐框 IoU ≥ 0.99、conf |Δ| ≤ 1e-3 数值容差）。### 3.2b R2.0：布局模型 manifest 与资产解析链（复用现有契约；评审 RBLK-2 解除条件，先于 R2 各子任务）

- **磁盘 manifest（沿用现有磁盘格式，`assets/manifests/ovisocr2/<revision>/community-abiray.json` 的字段风格）**：`assets/manifests/pp-doclayout-v2/<revision>/layout-paddlex.json` = `{schema_version: 1, model: "pp-doclayout-v2", source: {repository: "PaddlePaddle/PP-DocLayoutV2", revision: <冻结>, license: <以源仓库声明为准>, source_url: "https://hf-mirror.com/PaddlePaddle/PP-DocLayoutV2"}, assets: [{file: "inference.json", sha256, size_bytes, role: "layout-graph"}, {file: "inference.pdiparams", sha256, size_bytes, role: "layout-weights"}], runtime: {target_size: 800, pixel_scale: 255, conf: 0.25, class_table: 21, route: <A|B 按 R1>}}`。预处理参数由 manifest 提供，不允许就地硬编码。
- **运行时契约转换**：解析器落 `src/ocrx-models`（新 `LayoutModelManifest`，字段风格对齐 `ocrx-inference-api/src/asset.rs` 的 `ModelManifest`/`AssetDescriptor`——资产条目含 **relative_path**（缓存相对单段路径）+ **source_url**（HTTPS 钉扎，指向 hf-mirror 的 resolve URL）+ sha256/byte_len/role/required/scope，及 `provenance{provider,url}`；`license_id` 取磁盘 manifest 的 source.license）。注册/解析入口 = ocrx-models 的布局模型注册表（id `pp-doclayout-v2` + revision → 磁盘 manifest → 运行时结构），与 ovisocr2 的 id+revision 解析同构。
- **空缓存获取链（验收路径）**：空缓存 → 读磁盘 manifest → 逐资产按 source_url 下载 → sha256 校验 → 按 relative_path 缓存填充 → id+revision 解析 → 非 fake 检测冒烟（真实页 → 区域输出）。
- **降级语义**：缺失/哈希不符/运行时不可用 → 空区域 + warning 进 checkpoint warnings（扫描不失败）；但目标配置的 B2/R4 战役必须以"获取链全程成功"为前提。
- **验收**：从空缓存（或全新环境）走完获取链并完成非 fake 冒烟；有效配置与资产哈希进 checkpoint/result；降级语义单测。

### 3.3 R2：B1 管线接线（TDD）

核心数据结构（序数配对 / splice_m4m5 / F1–F5 / TableShape 解析）与比较算术已实现；本修订已按评审 BLK-3 修复 `keep_original` 第四项（行×列数）未参与决策的契约偏差（填充率持平后由第四项裁决，三类分支单测覆盖），R2 为纯工程接线：

| 子任务 | 内容 | 落点 | 验收 |
|---|---|---|---|
| R2.1 | 区域提供者端口：`RegionProvider` trait（`regions_for_page(page) -> Vec<DetectedRegion>`），paddle sidecar 实现者 + fake 实现；检测失败/超时 → 空区域（降级 = 保留原输出，不失败） | `src/ocrx-scan/src/reread/` | fake 单测（正常/失败/超时三路径） |
| R2.2 | 裁剪器：从 RenderedPage 按区域 bbox 裁剪（D-21：重读请求 1024 token；裁剪边长随 bbox 比例适配 1024 网格；crop 复用 `ocrx-pdf::crop_bbox` 语义） | `reread/` | 裁剪几何单测（含越界/退化 bbox） |
| R2.3 | 表格重读变体请求：table-HTML 重读 prompt（新契约 `table-reread-v1`，PROMPT_ID/VERSION 入身份）+ 1024 token + 纯贪心 | `ocr/` prompt + artifacts | 请求形状单测（prompt 键/生成参数/1024 网格） |
| R2.4 | 触发—对比—拼接编排：正常页 F1–F5 触发（默认 off）→ 裁剪重读 → **内容身份检查（v1.6 完整契约冻结，评审五轮解除条件）**——见 D-47→ 比较保留（严格更优才换）→ `splice_m4m5`；坏页（attempts ≥ 2 提交的退化页）恒触发（沿用原 B1 条款）。比较保留函数的第四项（行×列数）分支已按评审 BLK-3 修复并补三类分支单测（`compare_keep_fourth_element_decides_on_fill_tie`） | `reread/` + `window.rs` 后处理位 | 编排单测：触发/不触发/平局留原表/替换/区域缺失全保原 + 内容身份检查单测（双覆盖≥阈值→进比较/任一<阈值→保原/任一侧文本低于 8 bigram 下限→保原〔不可判定 fail-closed〕） |
| R2.5 | CLI 开关：`--table-reread on`（默认 off）+ `--layout-model pp-doclayout-v2`；生效值与模型 id+资产哈希进身份（B4.2 字段已就绪）。**执行身份扩展（评审 BLK-5 解除条件）**：`OcrIdentity`/`ScanOcrSide` 新增 `reread_prompt_version: String`（`table-reread-v1`）与 `reread_rules_generation: u32`（触发/比较/规范化规则的代次，校准一轮 = generation+1），纳入 `scan_side_matches`；resume 回归：prompt 切换 / 校准代次 +1 / 规范化版本切换三者均不复用旧产物；result 执行身份同步 | `ocrx-cli` + `ocrx-output/src/compatibility.rs` | 开关解析单测 + 身份字段断言 + 三项 resume 不复用回归 |
| R2.6 | 金集防线：开关 off 时管线零调用；on 时正常页替换率统计进 result（`tables_reread_total/replaced_total`） | `window.rs` + result schema | off 路径字节不变单测 + 计数器单测 |
| R2.7 | **持久化闭环（评审 BLK-4 解除条件）**：单一后处理产物函数——解析原 `PageDocument` → `splice_m4m5` 原位替换 → 同步生成新 `page_json` 与**定点 Markdown 更新**（仅替换目标表的 HTML 片段，非表格文本/公式/图片/空白逐字节不变；不做整页重格式化）→ 同一 `PageArtifactResult` 进 checkpoint/commit | `reread/` + `ocr/artifacts.rs` | 端到端验收四条：① on 但未触发 → 产物逐字节不变；② 替换后 md/json 一致且 diff 恰为目标表；③ 导出器读到新表（export-omnidocbench-predictions.py 输入侧验证）；④ checkpoint 输出哈希对应新 Markdown；非目标内容逐字节不变 |

门禁：fmt + clippy 0 + workspace 测试绿 + 金集 30 页（开关 off）零差异。

### 3.4 R3：B2 五门裁决（评测战役）

- **协议**：B0b 表格语料（worst 36 / mid 89 / good-en 120 / good-zh 297 / good-other 26 / bad-low 97）+ 冻结人群清单，对照（`--table-reread off`）与目标（`on`）双扫描同批页配对；官方同粒度逐表 TEDS；T-PROJ 全集投影。
- **五门（D-22 沿用 + 门①按 D-46 修订）**：① worst/mid/bad 池**安全合格页**触发覆盖率 ≥ 80%（分母修订 = R1b 安全合格页上的池表 146 张；绝对池覆盖 65.8% 作为投影输入如实报告，不作门）；② 正常页最终替换率 ≤ 2%（分母 = 对照配置正常页表格样本总数）；③ good 带（en+zh+other）逐表配对 ΔTEDS 均值 ≥ −0.02 且回归（Δ < −0.10）≤ 3 表或 2% 取大；④ worst/bad 带均值 ΔTEDS ≥ +0.3（按安全合格子集配对口径）；⑤ T-PROJ 全集投影落档 + 对照配置正常页 checkpoint 零漂移。
- **校准政策（D-29 沿用）**：触发/比较为 v1 冻结值，允许至多一轮校准（新值落修订记录并全量复跑），评分过程中不得调整定义。
- 落点：评测战役脚本 + `docs/develop/` 裁决记录。
- 不过门的处理：M4 回到"机制 GO / 产品未裁决"挂起；本方案按 2.1.3 降级收口。

### 3.5 R4：B4.3 双配置复评（B2 GO 后）

- 对照 = 新默认（M2 形态）；目标 = 对照 + `--table-reread on --layout-model pp-doclayout-v2`。
- 判定：目标配置 Overall ≥ 91（D-30 既有分支值）；对照配置判正常页零漂移；金集门（30 页冻结子集，D-15 口径）照常执行。
- 产物命名：轮次隔离（B4.3-R2 前缀），防覆盖教训。

### 3.6 R5（并行有界实验；基线冻结规则见下）：CDM/TEDS 回吐定位

round-2 vs landing 逐键 diff 定位 ~80 个 attempt-2 逃逸页的 CDM/TEDS 回吐集中点（en TEDS 0.6495→0.6324 为主嫌疑）；产出回吐-页面映射表。

**基线冻结规则（评审 BLK-6 解除条件，采纳第 1 项）**：
1. **R5 在 R4 完成前仅允许分析**（diff/映射表/定位结论）；任何行为修复不得进入 R3/R4 的对照或目标配置。
2. 若 R5 的行为修复必须在 R4 前合入：冻结新默认配置与相关人群/基线，**在该新配置上重跑 R3 全部 B2 门**，通过后才能进入 R4。不得只替换 R4 对照而沿用旧 B2 GO。

---

## 4. 行为基线扩展（继承 + 新增）

继承原方案 §2 全部条款，新增：

1. **区域提供者降级语义**：检测不可用/失败/超时 → 空区域 → 管线全保原输出（不失败、不阻塞扫描主路径）。
2. **开关默认 off**：`--table-reread` 未开启时，R2.4 编排零调用（单测锁定字节不变）。
3. **替换三重保险**：F1–F5 触发（可疑才读）→ 比较保留（严格更优才换）→ 替换率 ≤2% 门（B2 门②）；金集 30 页 + 正常页 checkpoint 零漂移兜底。
4. **重读 token 冻结**：裁剪重读 = 1024（D-21）；重读请求纯贪心。

## 5. 决策记录

| # | 决策 | 理由与放弃的备选 |
|---|---|---|
| D-33 | B0' 联合门拆分为表格门/公式门；表格门 PASS（V2 97.3%）→ M4 单独重开，公式门 NO-GO → M5/M6b 维持关闭 | 五模型实测：公式 ~60% 平台期为 GT 语义×检测范式系统性错位（precision 升 recall 不动），选型不可解；表格侧 V2 97.3% 显著过门，符合原重开条款"新版面检测能力证据"。备选"维持联合门全关"被否——把已过门的表格能力与未过门的公式捆绑关闭，违背门禁制度的分级入账原则 |
| D-34 | M4 重开范围 = 仅表格类区域；区域提供者即使检出公式区域也不产出重读 | 范围扩大禁令沿用 D-26；公式重读无合格区域来源（MFD/版面模型均 ~60%），强行纳入违反"失败能力不得进入目标配置" |
| D-35 | R1 运行时路线以实测裁决（预注册：B 路线需转换成功 + **规范化后完整区域序列一致**——class/ordinal/bbox/conf 逐项，bbox 逐框 IoU ≥ 0.99、conf \|Δ\| ≤ 1e-3） | 路线 B 发行干净但转换风险未知；路线 A 集成成本低但发行重。不以假设裁决，以数据裁决 |
| D-36 | 重读 prompt 独立契约 `table-reread-v1`（不复用主 prompt） | 重读输入是表格裁剪图、输出是纯表格 HTML——输入输出形态与整页 markdown 解码完全不同；独立 PROMPT_ID/VERSION 入身份（B4.2 字段） |
| D-37 | M4 触发双轨：正常页 F1–F5（默认 off）/ 坏页恒触发 | 沿用原 B1 条款；坏页人群的阶梯已耗尽，重读是其唯一补救通道 |
| D-38 | 投影纪律：全部投影给分量级算术，全量复评前不做 Overall 承诺 | M2 单位错误教训（text 点数误标 Overall 点数，中继门错设 1.05 分）；分量连带（CDM/TEDS 回吐）必须入预案 |
| D-39 | 类内阅读序冻结为 `(y 中心, x 中心)` 字典序 + IoU≥0.9 去重后掩码（不重编号）；B0a 多表页序数一致率/错配率落档为 B2 归因注记 | RBAC 序数配对的语义键必须可复现生成；检测器原始行序是置信度序（非阅读序），不能直接当序数。备选"检测器自带阅读序字段"不存在（RT-DETR 输出无序号） |
| D-40 | 布局模型资产走 manifest 冻结（id/revision/sha256/预处理参数/运行时配置），缺失或哈希不符降级为空区域 + warning | `--layout-model` 必须在干净环境可独立执行；仅存身份字段不提供模型文件。备选"机器本地路径直用"违反可复现性 |
| D-41 | 持久化闭环 = 定点 Markdown 替换（仅目标表 HTML 片段）+ page_json 同步生成，不做整页重格式化 | 整页通用序列化会扰动非表格内容（引入 B2 未覆盖的回归）；定点替换使 diff 恰为目标表，验收可逐字节断言 |
| D-42 | 重读执行身份新增 `reread_prompt_version` + `reread_rules_generation`（校准代次），纳入 scan_side_matches 与 result | prompt/规则变化后 resume 若不校验将混用旧算法产物（违反 D-29/D-36）；显式字段优于复合版本字段（可审计） |
| D-43 | R5 基线冻结：R4 前仅分析；行为修复前置合入必须重跑 R3 全部 B2 门 | B2 后改对照基线而不重跑裁决，将无法隔离 M4 边际效果（评审 BLK-6） |
| D-47 | **内容身份检查完整契约（v1.6 冻结；替代 v1.5 的"重叠 ≥ 冻结阈值"占位）**：① **规范化** = 剥离 HTML 标签 → 折叠全部空白 → 小写 → 字符 bigram 集合（中文友好，冻结 v1）；② **公式 = 双向覆盖率**：cov_a = \|grams(原块)∩grams(重读)\|/\|grams(原块)\|（原块被重读覆盖——碎片守卫），cov_b = 交集/\|grams(重读)\|（重读被原块覆盖——异表守卫）；③ **阈值 T1=0.40 / T2=0.25**（calibration 实测：合法分布 cov_a p5=0.615/cov_b p5=0.263，异表分布两者 p95≤0.036——T1/T2 下合法通过 238/248=96.0%、异表通过 7/244=2.9%，偏向拦截：被拦合法页保原零风险，漏过异表仍有 compare-keep + B2 门③双层后盾；标定脚本与产物 `analysis-reread-identity-calibration.json` 可复现）；④ **边界行为（全部 fail-closed）**：任一侧规范化后 unique bigram < 8 → 不可判定 → 保原（空表救援排除在 v1 外，与 O-5 口径一致）；通过 → 进入 compare-keep（不直接替换）；任一覆盖率 < 阈值 → 保原；⑤ **身份归属**：T1/T2/规范化版本/公式版本全部纳入 `reread_rules_generation`（R2.5），变更 = generation+1 → resume 不复用；校准轮次受 D-29"至多一轮"约束 | 阈值必须有数据支撑而非占位（评审五轮正确要求）；双向覆盖率分别拦截碎片（cov_a 低）与异表（cov_b 低）——单一 dice 会漏拦截碎片（子集 dice 偏高）；偏向拦截的取舍由失效代价不对称决定（保原 = 零风险，漏放 = 槽位污染且有后盾） |
| D-44 | 阅读序 = 双解释一致规则 + **积分制整页跳过**（可执行参考实现 `pairing_verdict`，R2.4 移植 Rust）；预注册硬门 = **①完美框（unsafe 0 / 合格页序数 100%）AND ②配对身份（真实 V2 输出 699 配对页错配 0；跳过 45 页 = 计数不匹配 26〔漏检 6 + 多检 20〕+ 排序歧义 16 + 去重 3）**；裁剪质量（2 页身份正确低重叠框，超集框含题注）为独立失效模式，由替换时内容身份检查（R2.4）+ compare-keep + B2 门③防护，不入配对门 | v1.1 的 `(y,x)` 字典序被完美框实验证伪（14.53% 错序）；几何规则无法消解编辑阅读序分歧，唯一安全形态 = 不确定即不重读；序数孔洞必须整页跳过。合成三态模拟被评审裁定无效并实锤——替换为真实输出验收后，验收器自身的 gt_ordinal 标注 bug 被立即暴露：验收器必须过自己的门。**IoU ≥ 0.7 混入配对身份门是 v1.4 范畴错误（IoU 度量裁剪质量），评审按冻结文本正确驳回"残留"口径——拆分而非改口径** |
| D-45 | 布局模型 manifest 复用现有契约：磁盘层沿用 ovisocr2 manifest 字段风格（schema_version/model/source{repository,revision,license,source_url}/assets[{file,sha256,size_bytes,role}]）；运行时层按 `AssetDescriptor`（role/required/scope/**relative_path**/sha256/byte_len/**source_url** HTTPS 钉扎）+ `provenance`；获取链 = 空缓存 → 解析 manifest → 逐资产 source_url 下载 → sha256 校验 → relative_path 缓存填充 → id 解析 → 非 fake 检测冒烟 | 评审 RBLK-2：自造格式与磁盘/运行时契约均不符且无获取路径。现有 `AssetDescriptor` 已含 relative_path + source_url 与 HTTPS 校验，复用而非新造 |
| D-46 | 门①分母修订：触发覆盖率 ≥80% 改在**安全合格页池表**（146 张）上计；绝对池覆盖 65.8% 如实报告为投影输入 | 安全机制（D-44）的结构性代价 = 歧义页不重读；若按绝对池计 65.8% < 80% 则门①结构性地不可能过——门必须度量"机制在能安全作用的范围内是否有效"，绝对覆盖作为投影折损入账（worst/bad 合格覆盖 55% 已折入 §2.1 投影） |

## 6. 风险

| 风险 | 缓解 |
|---|---|
| B2 五门不过（覆盖率/替换率/回归门任一失败） | M4 回挂起态，损失有界；D-29 允许一轮校准；触发三重保险使失败模式 = 不替换（无回退风险） |
| paddle sidecar 发行成本 | R1 预注册 ORT 转换判据；若 A 路线入选且 B2 过门，发行形态（可选依赖/引导安装）随 R4 后再定 |
| V2 区域质量在退化页失准 | 区域与 OCR 解耦（D-20 沿用）；失败降级 = 保留原输出；bad 带本身在 B2 门④的人群里实测 |
| 替换劣化 good 带表格 | 比较保留严格更优 + 替换率 ≤2% 门 + 金集兜底；good 带逐表回归封顶（门③） |
| 裁剪质量残留（真实输出实测 2/744 页身份正确但框重叠 0.64–0.69，超集框含题注） | **替换时内容身份检查**（R2.4，重读与原块重叠不足即保原）+ 冻结 compare-keep + B2 门③量化实测影响；显式入档非静默 |
| ≥91 目标不达 | 判定只由 B4.3 实测给出；B2 过门即 M4 转正入账（目标配置判定），未达 91 按方案 §1.2 出达成性报告 |
| 全局 2048 网格重试（M2 形态）与表格重读 1024 裁剪交互 | 重读是独立请求（1024 冻结），不与整页解码网格叠加；B2 配对实测兜底 |

## 7. 修订记录

**v1.5（2026-11-17）**——四轮重审响应（剩余 1 项：真实输出 unsafe_total = 2 不满足冻结 IoU ≥ 0.7 门）：
- **算术修正**：跳过页 = **45**（计数不匹配 26〔内含漏检 6 + 多检 20〕+ 排序歧义 16 + 去重 3）；v1.4 的"71"为重复相加，订正。
- **AND 门实际实现**：`omnidocbench-region-pairing-check.py` 读取完美框产物并求合（`perfect_box_gate_pass` 字段），hard_gate.pass = 配对身份门 AND 完美框门——不再只是"声称"。
- **门②拆分（范畴修正）**：IoU ≥ 0.7 度量裁剪质量而非配对身份，v1.4 将其混入身份门属范畴错误；②拆为**配对身份门**（序数 == k，无 IoU 下限；699 配对页 0 错配——防错表替换的硬门）与**裁剪质量项**（2 页超集框含题注，显式记录于 `crop_quality_below_0_7`）。裁剪质量由**机制**防护而非残留口径：R2.4 新增替换时**内容身份检查**（重读表与原块文本重叠 ≥ 冻结阈值才进 compare-keep，否则保原）——直接拦截错表/残表内容进入槽位；B2 门③实测回归封顶。评审若不认可该拆分，备选方案 = 规则跳过全部"低重叠风险页"，但免 GT 的运行时信号不存在，该备选不可实现——拆分是唯一可闭环路径。
- **非阻断**：v1.4 文档头部版本已于本轮同步。

**v1.4（2026-11-17）**——三轮重审响应（三态验证有效性收口）：
- **合成三态模拟废除**（评审裁定其无效且实锤成立：未执行冻结跳过条件、dedup/miss 同操作、异常页"碰巧正确"计安全、hard_gate 未纳入）。替换为**真实 V2 检测输出验收**：新工具 `omnidocbench-region-pairing-check.py` 内建积分制跳过规则的可执行参考实现（`pairing_verdict`，R2.4 按此移植），744 页全量实测——配对 699 页身份错配 0，71 页异常全部整页跳过，hard_gate 纳入全部状态。
- **验收器自身 bug 被真实输出验收暴露**：gt_ordinal 误用全区域下标（含公式区），单表页正确配对被错标为 unsafe（12 页假阳性）——修复标注后重跑收敛至真实残留 2 页（身份正确、低重叠裁剪框，compare-keep + 门③防护，风险表显式入档）。教训入 D-44：验收器必须过自己的门；合成扰动永远抓不到验收器 bug。
- **完美框 harness 清理**：假模拟代码移除，hard_gate 标注 scope（完美框下界 ONLY，真实输出安全由 pairing-check 把门并 AND 本门）。
- 非阻断：文档头部同步 v1.4。

**v1.3（2026-11-16）**——二轮重审响应（RBLK-1/RBLK-2 收口）：
- **RBLK-1（收口）**：分桶口径修正（合格 95 页全对 / 22 跳过 / 0 unsafe，与评审一致——v1.2 的"103 合格"混淆了歧义但恰好序对的页）；新增**积分制整页跳过规则**（检测数==输出块数 ∧ 无去重 ∧ 双解释一致，任一违反整页跳过）并入 D-44；**真实检测三态（漏检/中间多检/去重）模拟内建于验收工具**（unsafe 总计 0），硬门从"另测"改为内建门；覆盖产物可复现（`analysis-b0-ordinal-agreement.json` 内建 `b0b_pool_coverage` 段：146/222 = 65.77%）。
- **RBLK-2（收口）**：R2.0 重写——废除与现有契约冲突的自造 JSON；磁盘 manifest 沿用 ovisocr2 字段风格（含 source.source_url），运行时层按 `AssetDescriptor`（relative_path + HTTPS source_url 钉扎）+ `provenance`/`license_id` 转换；注册/解析入口 = ocrx-models 布局模型注册表（id+revision 同构 ovisocr2）；验收 = 空缓存获取链全程 + 非 fake 冒烟（D-45 落地细则）。
- **非阻断**：R1 正文与 D-35 已于 v1.2 同步（序列级一致 + 数值容差）；本版无新增口径变化。

**v1.2（2026-11-16）**——重审响应（RBLK-1/RBLK-2）：
- **RBLK-1**：`(y,x)` 字典序废止（完美框实验 14.53% 错序）；阅读序改为**双解释一致规则**（行/列几何解释都干净且同序才合格，歧义页运行时跳过重读）；实测 B0a 完美框 0 unsafe / 103 合格全对 / 22 跳过；预注册硬门 = unsafe 0 + 合格页序数正确率 100%（D-44）；覆盖代价如实核算（裁决池 65.8%），门①分母修订为安全合格池表（D-46），§2.1 投影同步修订（+1.1 → 89.55 保守口径）。
- **RBLK-2**：R2.0 重写为复用现有契约——磁盘层沿用 ovisocr2 manifest 字段风格、运行时层按 `AssetDescriptor`（relative_path/source_url HTTPS 钉扎）+ `provenance`；补空缓存获取链（解析 → 下载 → sha256 校验 → 缓存填充 → id 解析 → 非 fake 冒烟）作为验收（D-45）。
- **非阻断清理**：文档头部同步 v1.2；D-35 判据同步为序列级一致 + bbox/conf 数值容差。

**v1.1（2026-11-16）**——评审响应（BLOCK×6 全部处置 + 非阻断口径修正）：
- **BLK-1**：新增 R1b 区域规范化契约（类映射/置信度过滤/`(y,x)` 中心序生成/IoU 掩码去重/B0a 多表页序数一致率落档）；R1 A/B 一致性改为规范化后完整序列逐项比对（D-39）。
- **BLK-2**：新增 R2.0 布局模型 manifest 与资产解析链（id/revision/sha256/预处理参数/sidecar 启动配置；缺失降级语义；非 fake 冒烟验收）（D-40）。
- **BLK-3**：`keep_original` 第四项（行×列数）契约偏差**已修复并补三类分支单测**（`compare_keep_fourth_element_decides_on_fill_tie`）；§3.3 引言前提描述同步修正。
- **BLK-4**：新增 R2.7 持久化闭环子任务（定点 Markdown 替换 + page_json 同步 + 四条端到端验收：未触发字节不变 / diff 恰为目标表 / 导出器可读 / 哈希对应）（D-41）。
- **BLK-5**：R2.5 扩展执行身份——`reread_prompt_version` + `reread_rules_generation`（校准代次）纳入 `OcrIdentity`/`ScanOcrSide`/`scan_side_matches` 与 result；三项 resume 不复用回归（D-42）。
- **BLK-6**：R5 基线冻结规则——R4 前仅分析；行为修复前置合入必须重跑 R3 全部 B2 门（D-43）。
- **非阻断口径修正**：§2.1 样本数 86 → **133 表**（worst 36 + bad-low 97）；门④全集算术修正为 ≈ +2.005 Overall（下限含义），原 +0.3~0.9 区间废止。

**v1.0（2026-11-16）**——初稿：B0' 门拆分依据（五模型实测表）、M2 单位错误教训入投影纪律、R0–R5 任务分解（R2 六子任务 TDD 验收）、D-33–D-38 决策记录、风险表。
