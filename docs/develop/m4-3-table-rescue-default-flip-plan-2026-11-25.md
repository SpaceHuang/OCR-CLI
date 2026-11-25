# M4.3 表格救援扩展、引擎修复与默认化执行计划（2026-11-25）

**文档状态：** v0.2（评审响应——BLOCK×6 全部修订 + 非阻断×2；v0.1 初稿见修订记录。承接 `m4-2-table-structure-reread-plan-2026-11-22.md` v1.6——M4.2 已随 PR #17 合入 main，四门 PASS，Overall 91.50）

**范围：** 用户裁定的四项打包：① M4.2 默认开关翻转；② M4.3 部分缺表救援（原开放项 O-5）；⑥ 引擎侧确定性 throw 修复；③ M4 弱表重开 spike（B2 新机制假设重试）。按 **P0 → P1 → P2 → P3** 相位执行，每相位独立门禁，失败即停在相位内。

---

## 1. 事实基线（全部落档可查）

### 1.1 M4.2 合并后现状

- main @ `2d24fb2`（PR #17），Overall **91.50** vs 前基线 90.87（+0.63），table TEDS +0.0183（G-C，`analysis-m42-gc-final-adjudication.json`）。
- `--table-structure-reread` **默认 off**（`src/ocrx-cli/src/main.rs:108`）——91.50 只在显式开 flag 时成立；默认路径用户拿不到该价值。
- **输入链现状（评审 BLOCK-1 证据）**：结构重读 ON 时校验强制要求 `--layout-model` + 已存在的 `--layout-regions` 路径（`main.rs:668-697`）；无 flag 扫描两者均为 `None`（`main.rs:166-194`）。regions 今天只能来自外部预处理脚本（`scripts/omnidocbench-reread-regions-sidecar.py`，route A 文件形态 = R1 冻结决策），普通默认扫描不会自动获得。
- **资产链现状（BLOCK-1 解除基础）**：layout 模型 manifest 已冻结（`assets/manifests/pp-doclayout-v2/`）；regions 脚本产出含 `model_id`/layout revision/`source_pdf_sha256`/页覆盖（`region_provider.rs` 已实现这四项 provenance 校验，M4.2 RBLK-1）；表引擎 sidecar 基建（spawn/握手/身份）现成。
- 引擎 = SLANet-plus Python sidecar（CPU，实测 186.3s/表，RSS ~2.1GB）；引擎身份 `revision = bae6e5f8`，模型 SHA 冻结于 `assets/manifests/table-engine-slanet-plus/bae6e5f8/manifest.json`。
- 失败契约 D-63（冻结）：OFF 不 spawn 字节不变；**ON 启动类故障 = 硬错误**（先于任何页面产物）；运行中单请求故障 = 该区域保原；sidecar 死亡 = 后续全部 engine_down 不重启。

### 1.2 G-B 未改善 key 完全归类（24 key：14 改善 / 10 未改善，0 退化）

| 类别 | key | 处置 | 可救性 |
|---|---|---|---|
| **确定性 pipeline throw**（IndexError，跨运行稳定，产物像素级复现） | dca64e05 r?、1417 r1–r2、3713 r0–r2（共 6 区域） | engine_error → 保原 | **P0 目标：修复后潜在翻转 ≤ 5 个 GT key**（1417[0..2] 3 + 3713[0] 1 + dca64e05[0] 1） |
| 归因门拒绝 / empty_output（真实能力边界：巨型跨列表头等慢性表） | 2017、1349、2211、yanbaor2 ×2（5 key） | 按门保原 | 不在本计划范围（记录在案） |

### 1.3 部分缺表人群（P1 目标，预冻结口径）

来源：`D:/t/f6-ledger-458.json`（M4.1 时代 det/block 对账，458 页全量，**M4.2 之前的基线**——T1 须在当前 gen-4 基线上再冻结）：

- **部分缺表页（block_n ≥ 1 且 det_n > block_n）：17 页 / 135 个未配对 det 区域 / 页上零分 GT 表 20 张**；
- 典型页型：eastmoney 田字格多表页 3 页（det 4–7 / block 1，零分 GT 8 张——即 D-56 排序问题揭示的多表页）；newspaper 2 页（det 93/34，det 噪声为主，靠裁定线门过滤）；教材/研报散页；
- 对照：整页漏（block=0）23 页 / 38 det / 32 零分 GT，M4.2 冻结其中 16 页 / 24 key（形状/质量资格筛除 7 页）。

### 1.4 弱表带分布（P3 目标，B2 v2 战役口径，`analysis-b2-adjudication-v2.json`）

- worst/mid/bad 池（safe-eligible 页）：**99 表**（绝对池 222 表）；VLM 重读时代触发覆盖 62.6%（门① FAIL）；
- good 带配对 443 表：均值 ΔTEDS −0.0035 / 显著回归 2（门③ PASS——compare-keep 保护有效）；
- worst/bad 带配对 52 表：均值 ΔTEDS **−0.0027 ≈ 0**（门④ FAIL——VLM 对同一弱表重读无增益）；
- B2 v2 裁定 NO-GO 处置原文：重开需要"新的机制假设（如更重读网格/**专用表格解码**）"——SLANet-plus 即该假设。

---

## 2. 相位总览与执行顺序

| 相位 | 内容 | 依赖 | 独立价值 |
|---|---|---|---|
| **P0** | 引擎侧 throw 修复 + 引擎身份拆分（D-72） | 无 | 翻转 ≤ 5 个 GT key；修掉的 throw 可能同型出现于 P1 人群页 |
| **P1** | M4.3 部分缺表救援（gen 5） | P0（人群页重读成功率不受 throw 影响） | 预期 ~20 张零分 GT 的部分可救；TEDS 继续上抬 |
| **P2** | 默认开：**自动 regions 生成 + 三态语义 + 惰性 spawn + resume 状态机**（D-71/D-78） | P1（一次全集验证同时覆盖两代触发域） | 普通默认命令自动获得 M4.2/M4.3 价值（评审 BLOCK-1 选项 2） |
| **P3** | M4 弱表重开 spike（SLANet 机制假设） | P0（引擎稳定）；与 P1/P2 人群不相交 | 打最弱分量 TEDS 的剩余空间（worst/bad 带池） |

**明确不做（本计划非目标）：** M5 公式区域重读；M6b text-only 区域管线；SLANet/版面检测的 ONNX/GPU 化（P0–P3 全过后按触发域规模再立项）；F1–F5/G 触发阈值与 B2 五门的任何重校准（D-29 校准政策不为本计划启用；P1 仅开放 dedup/裁定线阈值的一轮校准，见 D-75）；重训/微调；评分器与数据集 revision。

---

## 3. 决策记录（承接 D-68）

| # | 决策 | 依据 |
|---|---|---|
| D-69 | **M4.3 触发域扩展**：F6 触发从"block==0"扩展为"同页 det_N > output_N（含 =0 特例）"；候选 = RBAC 类序数配对的**尾部未配对 det 区域**（配对复用 R2 pairing 参考实现语义；序数配对下未配对区域恒为尾部，构造性成立）；整页特例沿用 M4.2 形状资格不变 | RBAC ③ 本就定义 detection_N > output_N = miss 病例（不产新块）；M4.2 是其 output==0 极端特例 |
| D-70 | **M4.3 准入链**（部分缺表无锚文本，cov_attempt 归因公式不适用）：裁定线门（继承 27/27 校准）→ SLANet 重读 → **dedup-vs-emitted 门**（完整公式冻结于 D-75）→ spike 阶段 100% 人工复核，生产态 = ①+② | 整页特例的 cov_attempt 依赖 raw_single 整段 attempt 文本，部分缺表页不存在该锚；M4.2 empty 臂先例 = 无锚靠结构门+人工复核；伪表防线（conf 0.984 伪表被裁定线门拒绝的 27/27 校准）继承并加 dedup |
| D-71 | **默认开三态语义 + 自动 regions 生成 + resume 状态机**（评审 BLOCK-1/3 修订）：<br>**① 三态**：`--table-structure-reread` 缺省 = best-effort ON；显式 `on` 维持 D-63② 严格语义（启动类故障 = 硬错误）；显式 `off` 不变。<br>**② 自动 regions 生成**：结构重读活跃（缺省或显式 on）且未给 `--layout-regions` 时，扫描在页面循环前经 **layout sidecar regions 模式**（`sidecar/table-engine` 同一 Python 环境，扩展 regions 生成入口；模型资产 = 已冻结的 `assets/manifests/pp-doclayout-v2/`）对整文档生成 regions 文件至确定性缓存路径 `<out>/regions/<stem>.json`，随后走既有 route A 文件管线与全部 provenance 校验（model_id/revision/source_pdf_sha256/页覆盖）；`--layout-regions` 显式提供时优先使用、不生成。<br>**③ 失败降级**：缺省态任何引擎侧不可用（Python 缺失/依赖缺失/SHA 或版本握手不匹配/启动超时/regions 生成失败）→ **整文档降级 off**：summary 告警 + identity 记录；显式 on 保持硬错误。<br>**④ 惰性 spawn 状态机**（冻结）：<br>· 引擎状态 `Unprobed → Available(identity) \| Degraded(reason)`；显式 on = eager（状态在页面循环前解析，D-63② 不变）；缺省 = lazy（**首个触发页**（含 resume 比对到达的首个触发页）才 spawn/握手/生成 regions）。<br>· identity 新增结构段 `availability ∈ {engine{完整引擎身份}, degraded{reason, from_page}, unprobed}`；显式 off 沿用省略（字节兼容不变）。<br>· **resume 分类规则（逐页，冻结）**：(i) 显式 off ↔ stored absent → 全复用；(ii) mode 或 regions 配置不匹配 → 整文档 Reprocess；(iii) 同活跃 mode 下：**非触发页**在任何 availability 下内容相同 → 配置匹配即复用；**触发页**（谓词由 regions + 当前基块数确定性可算）→ stored engine{X} vs 当前 Available{Y}：X==Y 复用 / 否则 Reprocess；当前 Unprobed → 此刻执行 spawn 探测（即惰性 spawn 点）再按前句分类；当前/stored 任一 Degraded → Reprocess 该页。<br>· 缺省态运行中 engine 死亡（D-63④ 类）→ 不重启，其余页降级 off，identity 记 `degraded{from_page}`；该页之前已提交页均为非触发页（按定义），内容不受影响。<br>· 可用性翻转安全性：unavailable→available 与 available→unavailable 互切时，触发页内容必然不同 → 按规则 (iii) Reprocess；非触发页字节相同 → 复用，不会错配。<br>**⑤ 被否选项**："仅提供布局输入时缺省 ON"（评审选项 1）被否——regions 生成链（脚本+manifest+校验）资产齐备，自动化成本有界（约 2–3 工程日），而选项 1 使"普通默认命令自动找回"的既定价值 hollow；B0' 检测产品化的完整形态（批处理/资产分发/多文档编排）仍留后续里程碑，本计划只做单文档扫描内嵌的最小闭环。 | 默认路径不能因用户环境缺 Python/Paddle 而硬失败（破坏所有现存用户）；显式 opt-in 用户保留 fail-fast 契约；惰性 spawn 使零触发文档零引擎开销（否则每次扫描白付 ~10–30s 握手）；identity 之前置握手被移除后，engine 身份在首个触发页前不存在——resume 分类以配置段先行、engine 段延迟解析（BLOCK-3 状态机） |
| D-72 | **引擎身份拆分**：`revision` 拆为 `models_revision`（= manifest revision `bae6e5f8`，模型目录 SHA）+ `protocol_revision`（worker 行为代次，v1 起值 `proto2`）。P0 只 bump protocol；manifest 文件与模型 SHA 不动。resume 比对两者。**同步更新**：`PROTOCOL.md`、hello fixture、`EngineIdentity`/`mismatches_against_frozen()`、compatibility `StructureRereadIdentity` 序列化形状、result 展示段、代次注释 | worker 侧行为修复（无模型变化）不应触发模型 manifest 重冻结；身份链仍 fail-closed（任一不匹配不复用） |
| D-73 | **P3 spike 判据（评审 BLOCK-6 修订——判据与输入产物一一对应）**：spike 只裁决**机制信号**，good 带回归与全集投影移交战役（战役 = 已冻结的 B2 五门，天然包含）：① **worst/bad 带配对均值 ΔTEDS ≥ +0.10 且改善表占比 ≥ 20%**（分母 = T-P3.1 再冻结的 worst/bad 集合；对照 = P2 代默认产物，候选 = 同产物+弱表替换）；② **拒绝/伪表率 + compare-keep 决策日志全量落档**（无门值，只记录）；③ 链与战役**同一条**（见 D-77）。**GO → 按 m4-table-reopen R3/B2 五门战役（D-22 冻结门不重校准）出产品裁决**——战役门②（替换率 ≤2%）与门③（good 带 Δ ≥ −0.02）即 good 带回归判据，门⑤（T-PROJ + 零漂移）即全集投影判据，输入 = 全集双扫描（control = P2 代默认产物，candidate = 同产物 + 弱表臂）；**NO-GO → 弱表路径记战略关闭（VLM 与 SLANet 双机制假设均证伪）** | B2 v2 实测 worst/bad Δ≈0 是 VLM 机制证伪，非触发/比较契约问题；spike 与战役判据分离后各自输入产物完备（BLOCK-6 修订选项 2）；+0.10 spike 线低于产品门 +0.3，留出机制→产品的余量 |
| D-74 | **执行顺序 P0→P1→P2→P3**，每相位独立门禁，失败停相位：P0 修复直接改善 P1 人群页重读成功率；P2 全集验证一次覆盖 P1 新触发域；P3 人群与 P1/P2 不相交可后置 | 相位间最小重跑原则（避免 P2 验证两次全集） |
| D-75 | **dedup-vs-emitted 门完整公式（评审 BLOCK-4 修订，冻结）**：规范化 = **D-47 v1**（剥标签→折叠空白→小写→字符 bigram）；比较域 = 候选表 HTML 的**非平凡单元格集合** A（单元格文本规范化后长度 ≥ 2 字符计入）vs 同页**每一个已发射表块**的集合 B；判重 = **\|A∩B\| / \|A∪B\| ≥ 0.60**（Jaccard，对称，对任一已发射表命中即判 duplicate 拒绝）；地板：\|A\| < 2 → 候选按退化拒绝（既有结构有效性门大概率已拒）；某已发射表不可解析（\|B\| < 2）→ 该表移出比较域；**比较域为空（无可比已发射表）→ fail-closed 拒绝候选**，独立计数 `dup_undecidable`；初值 0.60；**校准政策**：T3 spike stage-A 归档分布，至多一轮校准（D-29 政策），且**仅限 Rust 侧 dedup 阈值**（变更走 gen 5 失效链）；**裁定线阈值若需变更属 worker 行为变化 → 必须同步 bump `protocol_revision` + resume 回归**（BLOCK-4 指出的代次缺口） | 部分缺表臂 dedup 是防"已发表格重复插入"的关键内容归属门，不能以占位符进入实现；D-47 v1 规范化是已落档先例（M4.1 T3 实现过）；方向/地板/不可判定处置全部显式冻结 |
| D-76 | **混合页插入契约（评审 BLOCK-4 修订，冻结）**：序数配对下候选恒为尾部 det 区域（D-69），故 **md 与 page_json 共同插入锚点 = 最后一个已发射表块之后**；多候选按 det 序数升序排列（全序，无歧义、无跳过——吸取 D-68 教训：歧义不跳页，确定性排序 + 审计旗标 `f6_partial_insert_anchor=after_last_table`）；**零删除断言**：既有块（正文+表格）逐字节不变、相对顺序不变，仅插入；md 与 page_json 导出顺序一致断言。**已知局限如实记录**：类序数配对在"模型跳发首表"页会把已发射块配到错误 det 序数（D-28 v1 冻结契约的既有局限），机制靠 dedup 自校正（重读该区域 → 与已发射表 Jaccard 命中 → 拒绝）；插入位置的阅读序完美性不做主张（输出块无坐标，几何定位不可实现） | 固定锚点使 md/page_json 插入确定性可执行且无需几何信息；dedup 门承担内容正确性，锚点只承担位置确定性 |
| D-77 | **P3 链保留 D-47 内容身份门（评审 BLOCK-5 修订）**：链冻结为 **F1–F5 触发 → 裁剪 → SLANet 重读 → `content_identity_pass`（D-47 双向内容覆盖，既有实现 `reread.rs:1038-1052`）→ 四元组字典序 compare-keep（平局保原）**；spike 与后续产品战役使用同一条链，不允许 spike 简化。不启用"移除 D-47"选项 | P3 处理的是**已发射表**替换（有锚人群），与 F6 raw/empty 无锚人群不同构；四元组只证结构更优不证内容同源，省略 D-47 会让结构更复杂的异表替换原表——这是 M4 链既有安全门，不存在移除理由 |
| D-78 | **P2 字节验收矩阵（评审 BLOCK-2 修订，按产物类分列）**：<br>**对照基线 = 同一 P0+P1 二进制、同一 assets/regions、新运行的 gen 5 显式 ON 全集产物**（该 run 同时是 P1 G-C' 的证据 run，一 run 两用）。<br>· **对照 A（缺省 best-effort ON vs 显式 ON，引擎可用）**：md/page_json **1651/1651 逐字节一致**（评测输入完备性，官方评测直接复用该 run 分数）；reread sidecar 逐字节一致；result/checkpoint **字段级**断言——除 `table_structure_reread.mode`（default vs on）与缺省态 `availability` 表达外全部字段相等（程序化逐字段 diff，非字节 diff）。<br>· **对照 B（缺省 + 引擎不可用 vs 旧 OFF）**：**仅页面内容产物**（md/page_json）与显式 OFF 旧产物逐字节一致；result/checkpoint **不做字节相等断言**，改为 schema 断言（summary 告警字段存在 + availability==degraded{reason}）+ 显式 OFF 的旧字节兼容单独回归（M4.2 G-A 证据继续有效）。<br>· **对照 C（显式 off）**：旧字节兼容不变。<br>· P1 分数复用范围 = 对照 A 成立时的该 run 官方评测结果；P1 的 G-C' 门本身对照 M4.2 gen-4 基线（91.50） | 与 gen-4 旧产物比字节在 P1 有增量的前提下自相矛盾（BLOCK-2）；降级告警/身份字段必然改变 result/checkpoint 字节，验收必须限定到页面内容产物；字段级 diff 使 mode 差异可见且其余字段受断言保护 |

---

## 4. 任务分解

### P0：引擎侧确定性 throw 修复 + 身份拆分

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P0.1 | **重枚举 + 根因诊断**：在 gen-4 生产渲染像素上重放 6 区域（dca64e05、1417 r1–r2、3713 r0–r2），worker 开 traceback 捕获完整栈；对照 smoke 时代 9 错误帧清单核对当前确定性集合（T0 probe crop 与生产 crop 清单曾有出入，以生产为准） | 6 区域 IndexError 栈落档，根因定位到 Paddle 管线具体环节（疑：cell-det 空框/方向分类小图/零框 rec 下游索引） |
| T-P0.2 | **worker 侧修复**：按根因加 guard/恢复路径（候选：最小尺寸 pad、跳过方向分类、空框短路返回空结构），恢复输出走同一条裁定线门→重读→归因链；不可恢复则干净 reject（`pipeline_throw` → 保原，语义同今天，零损害） | 6 区域中可恢复者产出 HTML 且过全部既有门 + 人工复核；其余保持保原 |
| T-P0.3 | **协议代次 bump**（D-72 拆分）：`engine_protocol.py` 拆 `models_revision`/`protocol_revision`；**同步更新 `PROTOCOL.md`、hello fixture、Rust `EngineIdentity` + `mismatches_against_frozen()`、compatibility `StructureRereadIdentity` 序列化形状、result 展示段、代次注释**；`REREAD_RULES_GENERATION` **保持 4**（Rust 侧规则未变） | 旧 ON 产物经 protocol_revision 不匹配被拒复用；OFF 路径字节不变（无引擎身份字段）；hello fixture 单测更新 |
| T-P0.4 | **回归验证**：非 throw 的 21 个冻结区域重读 HTML 逐字节一致；OFF 全集字节兼容不受影响（抽样）；smoke 真值表更新重跑 | 21/21 字节一致；真值表 PASS |

### P1：M4.3 部分缺表救援（gen 5）

| 任务 | 内容 | 验收 |
|---|---|---|
| T1 | **人群再冻结**：m42-gc-on 产物（当前基线）× 现有 458 页 regions sidecar 逐页对账（pairing-check），冻结部分缺表页清单 + 未配对区域 + GT 零分 key 映射（预期 ~17 页/20 零分 GT，以实测为准落 `analysis-m43-population-freeze.json`） | 冻结清单含逐页 (det_N, block_N, 候选区域, GT key)；与 1.3 预冻结口径差异解释落档 |
| T2 | **触发/配对/插入/去重契约实现**（D-69/D-70/D-75/D-76，TDD）：触发分支（det>block≥1）；尾部未配对候选抽取；dedup-vs-emitted 门（D-75 完整公式：Jaccard ≥ 0.60、地板 2 单元格、空比较域 fail-closed、`dup_undecidable` 计数）；插入持久化（D-76 固定锚点 = 最后已发射表块之后、det 序数全序、md/page_json 镜像、零删除）；计数器扩展 `f6_partial_*` 系；`REREAD_RULES_GENERATION` 4→5 | 单测 fixtures（BLOCK-4 清单）：首表缺失（序数错位 + dedup 自校正）/中部缺失/尾部缺失/检测序与输出序分叉/重复候选/多已发射表比较/空比较域 fail-closed/无正文页插入；既有表格与非目标正文逐字节不变；md/page_json 导出顺序一致 |
| T3 | **N 页 spike**（冻结人群）：重读运行 → 官方逐表 TEDS → stage-A 归档（dedup Jaccard 分布 + 裁定线分布 → D-75 一轮校准的样本口径）→ **采纳表 100% 人工复核**（对照页图） | **判据（预注册，全分母）**：① GT 可配对零分 key 救援率（TEDS ≥ 0.5）/ 零分 key 总数 ≥ 60%；② 采纳表人工复核伪表 = 0；③ text 分量不低于基线；④ 已发射表逐字节不变。校准仅限 dedup/裁定线阈值且按 D-75 代次规则执行，校准后重跑全判据 |
| T4 | **全集四门**：G-A' OFF 字节不变；G-B' 冻结 key 改善/退化；G-C' **Overall ≥ 91.50 且 TEDS 不降**（vs M4.2 后基线）；G-D' 成本入档（186s/表 × 新增 crop 数）。**G-C' 的证据 run = gen 5 显式 ON 全集 run（同 run 服务 P2 对照 A 基线，D-78）** | 四门全 PASS 才转正；失败按 D-30 矩阵精神降级（能力不进目标命令） |

### P2：默认开 = 自动 regions 生成 + 三态 + 惰性 spawn + resume 状态机

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P2.0 | **自动 regions 生成（D-71②）**：layout sidecar regions 模式（`sidecar/table-engine` 扩展入口：输入 pdf → 输出 regions JSON，schema 同 route A 文件 + manifest 握手）；CLI 接线（缺省/显式 on 且无 `--layout-regions` → 页面循环前生成至 `<out>/regions/<stem>.json` → 既有 provenance 校验 → route A 管线；多文档逐文档解析）；**检测成本实测入档**（458 页一次） | 生成文件过 RBLK-1 全部校验；458 页生成成功率 + 逐页耗时落档；`--layout-regions` 显式提供时不生成 |
| T-P2.1 | **三态 + 惰性 spawn + 状态机实现（D-71③④，TDD）**：三态解析；lazy spawn（首个触发页，含 resume 比对到达的触发页）；availability 三态记录与序列化（含 `degraded{reason, from_page}`）；降级路径（缺省态启动/握手/生成失败 → 整文档 off + summary 告警 + identity；显式 on 硬错误不变）；D-63④ 缺省态变体（运行中死亡 → 从 from_page 起降级） | 单测：三态 × 引擎可用/不可用矩阵；状态机全转移（Unprobed→Available/Degraded、运行中死亡）；availability 翻转（可用↔不可用）的 resume 分类规则 (iii) 逐条断言；显式 on 启动失败硬错误回归 |
| T-P2.2 | **验收矩阵（D-78 + BLOCK-1③ + BLOCK-3③）**：<br>· **对照 A**：gen 5 显式 ON 全集 run（= P1 G-C' 证据 run）vs 缺省 best-effort run：md/page_json 1651/1651 字节一致 + sidecar 一致 + result/checkpoint 字段级 diff（仅 mode/availability 差异）。<br>· **对照 B**：引擎不可用夹具（无 Python/坏模型 SHA 各一）缺省扫描完成、md/page_json == 显式 OFF 旧产物逐字节、result/checkpoint schema 断言（告警 + degraded）。<br>· **对照 C**：显式 off 旧字节兼容回归。<br>· **CLI 编排矩阵（真实扫描，非解析单测）**：无 flag+无 regions+引擎可用（自动生成生效，断言 `f6_triggered > 0` 于种子触发文档与产物表格数）/无 flag+引擎缺失（降级）/无 flag+显式 regions 文件（用文件不生成）/显式 on+自动（失败=硬错误）/显式 on+regions 文件（M4.2 既有行为）/缺 layout 资产（缺省降级 vs 显式硬错误）。<br>· **真实 checkpoint resume 回归**：同环境 ON→缺省全复用（零 Reprocess）；可用→不可用（触发页 Reprocess 为 OFF 内容、非触发页复用）；不可用→可用（反向）；degraded from_page 边界。 | 全矩阵 PASS 落档；README/runbook 更新：**如实表述**——普通默认命令在 Python 引擎环境就绪时自动获得表格找回；引擎不可用时自动降级（告警可见）；自动 regions 的逐页检测成本数字引用 T-P2.0 实测 |

### P3：M4 弱表重开 spike

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P3.1 | **人群再冻结**：B0b 冻结带 key 清单从 B2 战役产物提取，**在 P2 代基线产物上重算 band 归属**（M4.2/M4.3 可能已救走部分弱表），落 worst/mid/bad 集合与 worst/bad 子集分母；crop 重渲染（4096² 内，同 M4 链） | `analysis-m43b-weak-table-freeze.json`：逐表 key + band + 与 B2 v2 口径的差异解释；worst/bad 分母明确（B2 v2 时代 52，再冻结后以实测为准） |
| T-P3.2 | **spike 运行（D-77 完整链）**：F1–F5 触发（v1 冻结不动）→ 裁剪 → SLANet 重读 → **D-47 content_identity_pass** → 四元组字典序 compare-keep（平局保原）→ 官方配对评测（worst/bad 子集） | 跑通冻结集合；compare-keep 决策日志（换/留 + 理由）+ D-47 拦截记录 + 拒绝/伪表率全量落档 |
| T-P3.3 | **裁决（D-73 判据 ↔ 输入产物一一对应）**：① ΔTEDS ≥ +0.10 且 ≥20% 改善——输入 = T-P3.1 冻结清单 + T-P3.2 配对评测（分母 = 再冻结 worst/bad 集合）；② 拒绝/伪表率——输入 = T-P3.2 日志；③ 链一致性——输入 = 实现检查。GO → **战役（另列预算）**：全集双扫描（control = P2 代默认产物 / candidate = +弱表臂）→ B2 五门（D-22 冻结：覆盖率 ≥80% / 替换率 ≤2% / good 带 Δ ≥ −0.02 且回归 ≤2 / worst-bad Δ ≥ +0.3 / T-PROJ+零漂移）→ 产品裁决；NO-GO → 战略关闭落档 | 裁决记录 + 各判据输入产物路径落 `docs/review/` 评审；战役为独立预算项，spike GO 不自动开支 |

---

## 5. 风险

| 风险 | 缓解 |
|---|---|
| P0 根因在 Paddle 内部不可修 | 损失上限 = 5 个 GT key 维持 0（今天已零损害）；降级为干净 reject 语义不变 |
| P1 伪表（newspaper 93-det 页 det 噪声） | 裁定线门（27/27 校准继承）+ dedup 门 + spike 100% 人工复核；生产态依赖前两者，newspaper 页单独复核加严 |
| P1 序数错位导致重读已发射区域 | dedup-vs-emitted 门兜底（duplicate 拒绝），浪费成本可接受（186s/误配区域）；错位插入自校正机制见 D-76 局限记录 |
| P1 text 分量回归（插入表块挤占） | T3 判据③预注册；M4.2 同型判据曾 PASS |
| P2 自动 regions 的逐页检测成本进入默认路径 | 成本实测入档（T-P2.0）并在 README 如实标注；非触发文档同样支付检测成本（触发判定需要 det 区域）——这是能力的固有价格，ONNX/GPU 化留后续立项 |
| P2 跨环境字节不稳定（同一输入，有/无引擎环境产物不同） | 口径如实落档：字节稳定性以**同环境**定义；identity 记录 availability，跨可用性 resume 按状态机 Reprocess（BLOCK-3 矩阵回归覆盖） |
| P2 惰性 spawn 改变 D-63② 语义 | 显式 on 保持 eager+硬错误不变；只有缺省态惰性化；状态机与 resume 规则冻结于 D-71④ 并有真实 checkpoint 回归 |
| P3 good 带回归重演 | compare-keep 平局保原 + D-47 内容身份门（D-77）+ 战役门②③原样复核 |
| P3 spike 通过但五门战役再 NO-GO | 预算有界（spike 2 日 + 战役 2–3 日）；NO-GO 即双机制证伪，弱表线长期关闭，收益账清零（D-30 精神） |

## 6. 成本估算

| 相位 | 工程 | 算力 |
|---|---|---|
| P0 | 1–1.5 日（诊断 0.5 + 修复/身份拆分 0.5–1） | 16 页触发域重跑 + smoke（<0.5h） |
| P1 | 5–6 日（T1 0.5 / T2 2（含 dedup+插入契约单测面扩大）/ T3 1 / T4 1–1.5） | spike：裁定线过滤后预估 30–60 crop × 186s ≈ 2–3h；全集双扫描 + 评测 ≈ 数小时 |
| P2 | 3–4 日（T-P2.0 自动 regions 1–1.5 / T-P2.1 状态机 1–1.5 / T-P2.2 矩阵 1） | 458 页 regions 自动生成一次（成本实测入档）；全集 default run（评测复用字节一致性，不重评）≈ 数小时 |
| P3 | spike 2 日（+GO 时战役 2–3 日，战役含全集双扫描另列） | spike ≈ 99 表 × 186s ≈ 5h CPU；战役 ≈ 全集双扫描 + 评测 |

## 7. 修订记录

- **v0.2（2026-11-25，评审响应——BLOCK×6 + 非阻断×2）**：① **BLOCK-1** → D-71 重写：新增自动 regions 生成（layout sidecar regions 模式 + 已冻结 pp-doclayout-v2 manifest + 既有 provenance 校验），普通默认命令自动生效；被否选项（仅提供布局输入时缺省 ON）如实记录；CLI 编排矩阵入 T-P2.2。② **BLOCK-2** → 新增 D-78 字节验收矩阵：对照基线改为同二进制 gen 5 显式 ON 新 run（与 P1 G-C' 一 run 两用）；降级字节门限定到页面内容产物，result/checkpoint 改 schema 断言。③ **BLOCK-3** → D-71④ 冻结惰性 spawn 状态机（Unprobed/Available/Degraded + availability 段 + 逐页 resume 分类规则 + 可用性翻转安全性论证）；真实 checkpoint resume 回归入 T-P2.2。④ **BLOCK-4** → 新增 D-75（dedup 完整公式：D-47 v1 规范化、Jaccard ≥0.60、地板、空比较域 fail-closed、校准代次规则——裁定线变更必须 bump protocol_revision）与 D-76（混合页插入：固定锚点 = 最后已发射表块之后、det 序数全序、零删除、序数错位自校正局限如实记录）；T2 fixtures 按 BLOCK-4 清单扩充。⑤ **BLOCK-5** → 新增 D-77：P3 链保留 D-47 content_identity_pass，spike 与战役同链。⑥ **BLOCK-6** → D-73 修订：spike 判据缩为机制信号（worst/bad ΔTEDS + 拒绝率落档），good 带回归与 T-PROJ 明确移交 B2 五门战役（门②③⑤），各判据输入产物路径与分母落 T-P3.3。⑦ 非阻断：D-72 修正 `bae6e6f8`→`bae6e5f8` 笔误；T-P0.3 显式列入 PROTOCOL.md/hello fixture/result 展示/代次注释。⑧ 成本重估：P1 5–6 日、P2 3–4 日。
- **v0.1（2026-11-25）**：初稿。四相位结构、D-69–D-74、事实基线取自 f6-ledger-458 / analysis-b2-adjudication-v2 / analysis-m42-det-spike-production-freeze / G-B adjudication。
