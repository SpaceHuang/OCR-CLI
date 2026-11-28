# M4.3 表格救援扩展、引擎修复与默认化执行计划（2026-11-25）

**文档状态：** v0.9（P3 双臂 spike FAIL——弱表线永久关闭。三机制证伪：OvisOCR2 裁剪（B2 v2，Δ≈0）、OvisOCR3 裁剪（本 spike 臂 B，worst/bad 均值 −0.19）、SLANet 裁剪（本 spike 臂 A，−0.27）；改善比例 27–31% 达标但均值深度负值，严重退化 −0.5~−0.77 不受四元组 compare-keep 约束（其非 TEDS 感知）。P1/P2b 产出不受影响（弱表臂从未投产）。v0.8 记录保持。承接 `m4-2-table-structure-reread-plan-2026-11-22.md` v1.6）——P1 四门 PASS（Overall 91.50 → **91.72**，`analysis-m43-t4-four-gates.json`）；G0 GPU 迁移完成（120×，`analysis-m43-g0-gpu-migration.json`）；**P2b-4 验收 PASS**（β≡α 1651/1651、γ≡β md 1651/1651 + Overall 91.72 经默认路径复现、降级/严格/resume 夹具全过，`analysis-m43-p2b4-acceptance.json`）。仅剩 P3 弱表 spike（判据 D-73 冻结）与 spike 表人工复核。v0.5–v0.7 闭环保持。承接 `m4-2-table-structure-reread-plan-2026-11-22.md` v1.6）——用户选项 C：部分缺表臂按实效组合判据 GO（零回归 + 真漏子群 4/9 救援且全部 ≥0.84 + 全集 G-C' 不降为最终条款），进 P1/T4 全集四门；G0 GPU 迁移全完成（120× 加速账本 `analysis-m43-g0-gpu-migration.json`）。v0.6 的 G0/D-79/D-80 保持。承接 `m4-2-table-structure-reread-plan-2026-11-22.md` v1.6）

**范围：** 用户裁定的四项打包：① M4.2 默认开关翻转；② M4.3 部分缺表救援（原开放项 O-5）；⑥ 引擎侧确定性 throw 修复；③ M4 弱表重开 spike（B2 新机制假设重试）。按 **P0 → P1 → P2 → P3** 相位执行（**T-P2.0a 工具部分前置至 P1 T4 之前**，见相位表），每相位独立门禁，失败即停在相位内。

---

## 1. 事实基线（全部落档可查）

### 1.1 M4.2 合并后现状

- main @ `2d24fb2`（PR #17），Overall **91.50** vs 前基线 90.87（+0.63），table TEDS +0.0183（G-C，`analysis-m42-gc-final-adjudication.json`）。
- `--table-structure-reread` **默认 off**（`src/ocrx-cli/src/main.rs:108`）——91.50 只在显式开 flag 时成立；默认路径用户拿不到该价值。
- **输入链现状**：结构重读 ON 时校验强制要求 `--layout-model` + 已存在的 `--layout-regions` 路径（`main.rs:668-697`）；无 flag 扫描两者均为 `None`（`main.rs:166-194`）。regions 今天只能来自外部预处理脚本（`scripts/omnidocbench-reread-regions-sidecar.py`，route A 文件形态 = R1 冻结决策）。
- **route A 读取契约**：provider 按 `<regions-root>/<stem>.regions.json` 目录形态查找（`region_provider.rs:121-146`），并校验 `model_id`/layout revision/`source_pdf_sha256`/页覆盖（M4.2 RBLK-1）——**自动生成的缓存文件名与目录形态必须服从该契约**。
- **资产链现状**：layout 模型 manifest 已冻结（`assets/manifests/pp-doclayout-v2/`）；表引擎 sidecar 基建（spawn/握手/身份）现成。
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
| **P2a（工具前置）** | **T-P2.0a：layout sidecar regions 模式**（P1 战役的 regions 输入工具，先于 P1 T4） | P0（同一 sidecar 代次） | 为 P1 生成冻结 regions（RBLK-3 证据链选项 1） |
| **P1** | M4.3 部分缺表救援（**最终代次 G ∈ {5,6}，T3 裁决确定，D-75/D-78**）；**战役 regions = P2a 自动生成并冻结** | P0 + P2a | 预期 ~20 张零分 GT 的部分可救；TEDS 继续上抬 |
| **G0（引擎 GPU 迁移）** | paddlepaddle-gpu 迁移 + 供应链重冻结 + 真值表 GPU 时代重冻结 + 全门重验（D-79/D-80）——**P1/T4 与 P2b 的前置** | P0/P2a/P1-T1–T3 | 用户场景成本合规（186s/表 CPU 不可上线）；dca64e05 超时类自动消解 |
| **P2b（默认化）** | 默认开：CLI 自动 regions 接线 + 三态语义 + 惰性 spawn + resume 状态机（D-71）；**三方证据链验收（D-78：Run β 显式 ON 全集 run + β≡α 门 + γ≡β 门）** | P1 | 普通默认命令自动获得 M4.2/M4.3 价值 |
| **P3** | M4 弱表重开 spike（SLANet 机制假设） | **P0 + P2b**（对照 = P2 代默认产物，D-73） | 打最弱分量 TEDS 的剩余空间（worst/bad 带池） |

**明确不做（本计划非目标）：** M5 公式区域重读；M6b text-only 区域管线；ONNX 原生重实现（GPU 化已由用户指令提级入范围 = 相位 G0/D-79；ONNX 路线仍留后续立项）；F1–F5/G 触发阈值与 B2 五门的任何重校准（D-29 校准政策不为本计划启用；P1 仅开放一轮校准，范围与代次冻结于 D-75）；重训/微调；评分器与数据集 revision。

---

## 3. 决策记录（承接 D-68）

| # | 决策 | 依据 |
|---|---|---|
| D-69 | **M4.3 触发域扩展**：F6 触发从"block==0"扩展为"同页 det_N > output_N（含 =0 特例）"；候选 = RBAC 类序数配对的**尾部未配对 det 区域**（配对复用 R2 pairing 参考实现语义；序数配对下未配对区域恒为尾部，构造性成立）；整页特例沿用 M4.2 形状资格不变 | RBAC ③ 本就定义 detection_N > output_N = miss 病例（不产新块）；M4.2 是其 output==0 极端特例 |
| D-70 | **M4.3 准入链**（部分缺表无锚文本，cov_attempt 归因公式不适用）：裁定线门（继承 27/27 校准）→ SLANet 重读 → **dedup-vs-emitted 门**（完整公式冻结于 D-75）。**生产态自动门 = 裁定线门 + 结构有效性门 + dedup 门（编号如左，无人工环节）；100% 人工复核仅属 T3 spike 裁决，不进生产态** | 整页特例的 cov_attempt 依赖 raw_single 整段 attempt 文本，部分缺表页不存在该锚；伪表防线（conf 0.984 伪表被裁定线门拒绝的 27/27 校准）继承并加 dedup |
| D-71 | **默认开三态语义 + 自动 regions 生成 + resume 状态机**（RBLK-1/2 再修订）：<br>**① 三态**：`--table-structure-reread` 缺省 = best-effort ON；显式 `on` 维持 D-63② 严格语义（启动类故障 = 硬错误）；显式 `off` 不变。<br>**② 自动 regions 生成——时序统一（RBLK-1）**：结构重读活跃（缺省或显式 on）且未给 `--layout-regions` 时，**layout regions 在 resume 分类与页面循环之前整文档生成**（触发谓词依赖 det_N，检测必须先于触发分类——这是能力固有成本，非触发文档同样支付）；**SLANet 表结构引擎保持惰性**：缺省态在首个合格触发页才 spawn/握手；显式 on 的 SLANet spawn 保持 eager（D-63②）。缓存路径服从 route A 契约：regions 根 = `<out>/regions/`，文件 = **`<regions-root>/<stem>.regions.json`**（与 `region_provider.rs:121-146` 一致）；`--layout-regions` 显式提供时优先使用、不生成。<br>**③ 失败降级**：缺省态任何引擎侧不可用（Python 缺失/依赖缺失/SHA 或版本握手不匹配/启动超时/regions 生成失败）→ **整文档降级 off**：summary 告警 + identity 记录；显式 on 保持硬错误。**开销口径如实**：缺省态省的是 **SLANet 启动/握手开销**；layout 检测成本只要重读活跃就存在。<br>**④ resume 状态机（RBLK-2 再修订）**：<br>· 引擎状态 `Unprobed → Available(identity) \| Degraded(reason, from_page)`；显式 on = SLANet eager；缺省 = SLANet lazy。<br>· identity 结构段：`mode ∈ {default, on}`（仅观测口径）+ `availability ∈ {engine{完整引擎身份}, degraded{reason, from_page}, unprobed}`；显式 off 沿用省略（字节兼容不变）。<br>· **resume 分类按语义配置类，不按 mode 字面**：`active`（= 显式 on 或缺省 best-effort，要求 regions 配置 + 代次 + 引擎身份一致且双方实际 Available）与 `off`（显式 off / 降级后内容）两类；**on↔default 互切同属 active，配置一致即复用；active↔off 互斥**（触发页内容必然不同 → Reprocess）。<br>· **历史触发状态持久化**：每页 checkpoint/reread sidecar 新增 `trigger_state{qualified, arm: whole\|partial\|none, base_block_n, shape}`（= **重读前**的资格/基块数/形状摘要）；availability 翻转时以 stored trigger_state 分类，**禁止从救援后 page_json 反推**（新增表会把 det_N > output_N 抹平、raw/empty 形状消失）；stored 无 trigger_state 的旧产物 → 保守 Reprocess（gen 5 代次门本就使 gen-4 产物失效）。<br>· **逐页分类规则**：(i) active↔active（配置一致）→ 非触发页复用；触发页 stored engine{X} vs 当前 Available{Y}：X==Y 复用 / 否则 Reprocess；当前 Unprobed → 此刻执行 SLANet 探测（惰性 spawn 点）再分类；任一 Degraded → 该页 Reprocess。(ii) active↔off：非触发页内容字节相同 → 复用；触发页（以 stored trigger_state 判定）→ Reprocess。**Available 的解析时点**：仅在 trigger_state 判定为合格的页面按需解析；非触发页保持 Unprobed，**不强制 spawn**（否则惰性 spawn 名存实亡）。<br>· **运行中死亡边界（RBLK-2③ 修订）**：缺省态 SLANet 死亡于第 k 页（k 必为触发页，此前可有已采纳表）→ **k 之前已提交页（含 Available 产物）全部保留**，k 起降级 off，identity 记 `degraded{reason, from_page=k}`；resume 时页 <k 按 engine 页分类、页 ≥k 按 off 内容分类。<br>· 可用性翻转安全性：active 内身份一致复用、身份漂移 Reprocess；active↔off 触发页必然 Reprocess、非触发页字节相同复用——任何方向都不会错配。<br>**⑤ 被否选项**："仅提供布局输入时缺省 ON"被否——自动生成链资产齐备、成本有界，选项 1 使"普通默认命令自动找回"的既定价值 hollow；B0' 检测产品化的完整形态（批处理/资产分发/多文档编排）仍留后续里程碑，本计划只做单文档扫描内嵌的最小闭环。 | 触发分类先于触发判定（检测成本不可避免，RBLK-1）；mode 字面匹配与 ON→缺省复用验收矛盾（RBLK-2①）；救援后产物不可反推历史触发（RBLK-2②）；"死亡前均非触发页"假设错误（RBLK-2③） |
| D-72 | **引擎身份拆分**：`revision` 拆为 `models_revision`（= manifest revision `bae6e5f8`，模型目录 SHA）+ `protocol_revision`（worker 行为代次，v1 起值 `proto2`）。P0 只 bump protocol；manifest 文件与模型 SHA 不动。resume 比对两者。**同步更新**：`PROTOCOL.md`、hello fixture、`EngineIdentity`/`mismatches_against_frozen()`、compatibility `StructureRereadIdentity` 序列化形状、result 展示段、代次注释 | worker 侧行为修复（无模型变化）不应触发模型 manifest 重冻结；身份链仍 fail-closed |
| D-73 | **P3 spike 判据（判据与输入产物一一对应）**：spike 只裁决**机制信号**，good 带回归与全集投影移交战役：① **worst/bad 带配对均值 ΔTEDS ≥ +0.10 且改善表占比 ≥ 20%**（分母 = T-P3.1 再冻结的 worst/bad 集合；对照 = **P2 代默认产物**）；② **拒绝/伪表率 + compare-keep 决策日志全量落档**（无门值，只记录）；③ 链与战役**同一条**（见 D-77）。**GO → 按 m4-table-reopen R3/B2 五门战役（D-22 冻结门不重校准）出产品裁决**——战役门②（替换率 ≤2%）与门③（good 带配对均值 ΔTEDS ≥ −0.02 且显著回归 Δ < −0.10 **≤ max(3, 2%)**）即 good 带回归判据，门⑤（T-PROJ + 零漂移）即全集投影判据，输入 = 全集双扫描（control = P2 代默认产物）；**NO-GO → 弱表路径记战略关闭** | spike 与战役判据分离后各自输入产物完备；B2 门③公式恢复 D-22 原文（非阻断清理③） |
| D-74 | **执行顺序 P0 → P2a → P1(T1–T3) → G0 → P1(T4，GPU 证据) → P2b → P3**，每相位独立门禁，失败停相位：P0 修复改善 P1 人群页重读成功率；P2a 为 P1 战役生成冻结 regions（证据链统一，RBLK-3）；P2b 验证消费同一生成器的默认路径；P3 对照 P2 代产物 | 相位间最小重跑 + 证据链单源原则 |
| D-75 | **dedup-vs-emitted 门完整公式（RBLK-4 再修订，冻结）**：<br>**集合构造（唯一算法）**：对候选表与每个已发射表 HTML，取全部 th/td 单元格文本 → **单元格规范化 = 剥标签 → 折叠空白 → 小写（到此为止，不做 bigram）** → 规范化后长度 ≥ 2 字符的单元格字符串计入集合。A = 候选集合，B = 已发射表集合；**\|A\| 即单元格数**。判重 = **\|A∩B\| / \|A∪B\| ≥ 0.60**（Jaccard，对称；对同页任一已发射表命中即判 duplicate 拒绝）。地板：\|A\| < 2 → 候选按退化拒绝；某已发射表 \|B\| < 2 → 该表移出比较域；**比较域为空 → fail-closed 拒绝候选**，独立计数 `dup_undecidable`。**钉死 fixture（手算，五例）**：构造候选与已发射表各一，含完全相同（Jaccard=1 拒）、部分重叠（跨 0.60 边界两侧各一例：0.60 恰好命中拒、0.5<0.60 留）、零重叠（留）、单单元格候选（地板拒）五例，断言交集/并集/判定全部与手算一致。初值 0.60。**校准与代次（冻结）**：**未校准的最终规则 = gen 5**；T3 stage-A 归档 Jaccard 分布，**若发生 Rust 侧 dedup 阈值校准 → 最终规则 = gen 6**（`REREAD_RULES_GENERATION` 5→6 + 同引擎身份下 5↔6 双向不复用回归 + 全门重跑；**不允许以全新输出目录替代代次隔离**）。**一轮校准范围统一 = 仅 Rust 侧 dedup 阈值**；裁定线阈值变更不属于该校准轮，属 worker 行为变更 → bump `protocol_revision` + resume 回归（D-72 链）。 | 集合口径三义性消除（单元格字符串集合，非 bigram 并集/集合的集合）；校准前 spike/checkpoint 与校准后产物必须不同规则身份（M4.2 同型教训，`m4-2 plan:69-79`） |
| D-76 | **混合页插入契约（冻结）**：序数配对下候选恒为尾部 det 区域（D-69），故 **md 与 page_json 共同插入锚点 = 最后一个已发射表块之后**；多候选按 det 序数升序排列（全序，无歧义、无跳过——吸取 D-68 教训：歧义不跳页，确定性排序 + 审计旗标 `f6_partial_insert_anchor=after_last_table`）；**零删除断言**：既有块（正文+表格）逐字节不变、相对顺序不变，仅插入；md 与 page_json 导出顺序一致断言。**局限如实记录（RBLK 轮非阻断修订措辞）**：类序数配对在"模型跳发首表/中部表"页会把已发射块配到错误 det 序数（D-28 v1 冻结契约的既有局限）；dedup 门对此的作用是 **fail-closed 防止重复插入**，**不主张自动纠正配对、不找回被错误配对吞掉的区域**——首表/中部缺失 fixture 的预期 = 不救援，如实落档；插入位置的阅读序完美性不做主张（输出块无坐标，几何定位不可实现） | 固定锚点使 md/page_json 插入确定性可执行且无需几何信息；dedup 承担"不重复"而非"配对纠正" |
| D-77 | **P3 链保留 D-47 内容身份门**：链冻结为 **F1–F5 触发 → 裁剪 → SLANet 重读 → `content_identity_pass`（D-47 双向内容覆盖，既有实现 `reread.rs:1038-1052`）→ 四元组字典序 compare-keep（平局保原）**；spike 与后续产品战役使用同一条链，不允许 spike 简化。不启用"移除 D-47"选项 | P3 处理的是**已发射表**替换（有锚人群）；四元组只证结构更优不证内容同源 |
| D-79 | **引擎 GPU 迁移（用户指令，上线准入；原"阶段 2 可选"提级）**：sidecar 运行时从 `paddlepaddle 3.4.0 (CPU)` 迁移到 **`paddlepaddle-gpu 3.3.1`（cu126，Windows 官方源 wheel；3.4.0 无 Windows GPU 版——如实记录）**；paddleocr 3.7.0 / paddlex 3.7.2 与 8 个模型目录**不变**（models SHA 不动）。manifest pinned 更新 + 记录 CUDA 运行时口径；`protocol_revision` bump（proto3）；`EngineIdentity` 新增 `device` 字段（hello 上报，cpu-era 产物与 gpu-era 互不复用）。**CPU 通道保留为回退**（gpu wheel 亦可跑 device=cpu；device 入身份防止跨时代复用） | 用户场景成本裁决：186s/表 CPU 不可上线（用户 20 表 PDF ≈ 数小时）；RTX 4090 硬件无障碍；dca64e05 的 1099s 超时类在 GPU 下预计自然落回 600s 阈内（用户选项 A 的遗留问题自动消解） |
| D-80 | **证据链顺序（避免全集双跑）**：G0 先于 P1/T4——GPU 迁移 + 重验全部完成后，P1/T4 四门（Run α）与 P2b 三方链**一次性跑在最终引擎上**。P1/T3 的 CPU spike 降级为**机制预演**（触发/门链/dedup 行为验证 + D-75 校准分布初稿）；正式 spike 判据（≥60% 救援率）以 **GPU 时代重跑的 spike** 为准（17 页重跑成本有界） | 引擎身份变更使既有 ON 产物全部失效；先定引擎再跑战役，最小化重跑 |
| D-78 | **P2b 证据链与字节验收矩阵（R2BLK-1/2 + R3BLK-1 修订——三方证据链 + 最终代次 G 贯穿 + 分数分派唯一规则）**：<br>**代次 G（R2BLK-2）**：`G ∈ {5, 6}`，由 P1 T3 裁决确定——未校准 = 5；发生 Rust dedup 校准 = 6（D-75）。**Run α/β/γ 必须使用同一 G**；字段级 diff 断言 `reread_rules_generation == G`；发生校准时 resume 矩阵保留 5↔6 互斥证据；不得以校准前 gen 5 spike 产物充当最终对照。<br>**三方证据链（R2BLK-1 选项 2——P2b 是代码变更而非开关翻转，P1 旧二进制不可能与 P2b 默认 run "同二进制"）**：<br>· **Run α** = P1 相位二进制的显式 ON 全集 run（P2a 冻结 regions，代次 G）——P1 G-B'/G-C' 证据，官方评测 E(α)。<br>· **Run β** = **P2b 最终二进制**的显式 ON 全集 run（同一 P2a 冻结 regions，同一 G）。**门 β≡α**：md/page_json 1651/1651 逐字节一致（证明 P2b 新增的三态 CLI/序列化/状态机代码未改变重读语义）。<br>· **Run γ** = P2b 缺省 best-effort 全集 run（最终二进制，自动生成 regions，同一 G）。**门 γ≡β**：① regions 等价（自动生成 vs P2a 冻结 458/458 逐文件字节一致）；② md/page_json 1651/1651 一致；③ sidecar/result/checkpoint **字段级** diff——除 `mode`（default vs on）与缺省态 `availability` 表达外全部字段相等 + `reread_rules_generation == G`。<br>**分数分派规则（R3BLK-1，冻结，唯一）**：<br>· β≡α 成立 → **S(β) = E(α)**；失败 → 对 β 直接官方评测，**S(β) = E(β)，此后 E(α) 禁止使用**。<br>· γ≡β 三项全部成立 → **S(γ) = S(β)**（β≡α 已失败时即继承 **E(β)**，不得回退 E(α)）；任一失败 → 对 γ 直接官方评测，S(γ) = E(γ)。<br>· **α 与 γ 的等价性只经由 β 传递成立**，不得把 α（旧二进制）产物直接称为与默认 run 的"同二进制对照"。<br>**fallback 质量门（R3BLK-1——直接评测分支同门，冻结；直接评测只是证据来源替换，不是质量要求豁免）**：<br>· 绝对：Overall ≥ 91.50 且 table TEDS 不降（对 M4.2 gen-4 基线，与 P1 G-C' 同口径）；<br>· 相对：对 E(α) 的 Overall Δ ≥ −0.05 且 TEDS Δ ≥ −0.005（评测非确定性噪声容忍——M4.2 期间实测同字节输入曾出现 ~−0.02 Overall 噪声）；<br>· **未过质量门 = P2b FAIL**，不得因"已直接评测"判 PASS。<br>**验收记录字段（落 `analysis-m43-p2b-acceptance.json`）**：`regions_equal`、`beta_alpha_equal`、`gamma_beta_equal`、`eval_source ∈ {E(α), E(β), E(γ)}`、`adopted_scores`——裁决可追溯到实际消费的 run。<br>**对照 B（缺省 + 引擎不可用 vs 旧 OFF）**：仅页面内容产物（md/page_json）与显式 OFF 旧产物逐字节一致；result/checkpoint 改 schema 断言（summary 告警 + availability==degraded{reason}）；显式 OFF 旧字节兼容单独回归。<br>**对照 C（显式 off）**：旧字节兼容不变。 | P2b 新增三态/序列化/状态机代码，P1 证据 run 不可能与其默认 run 同二进制（R2BLK-1）；固定 gen 5 与 D-75 校准后 gen 6 冲突（R2BLK-2）；β≠α 后 γ 绕过失败门继承 E(α) 属分数归因错误，直接评测无门则回退仍可字面 PASS（R3BLK-1） |

---

## 4. 任务分解

### P0：引擎侧确定性 throw 修复 + 身份拆分

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P0.1 | **重枚举 + 根因诊断**：在 gen-4 生产渲染像素上重放 6 区域（dca64e05、1417 r1–r2、3713 r0–r2），worker 开 traceback 捕获完整栈；对照 smoke 时代 9 错误帧清单核对当前确定性集合（以生产为准） | 6 区域 IndexError 栈落档，根因定位到 Paddle 管线具体环节 |
| T-P0.2 | **worker 侧修复**：按根因加 guard/恢复路径（候选：最小尺寸 pad、跳过方向分类、空框短路返回空结构），恢复输出走同一条裁定线门→重读→归因链；不可恢复则干净 reject（`pipeline_throw` → 保原，语义同今天，零损害） | 6 区域中可恢复者产出 HTML 且过全部既有门 + 人工复核；其余保持保原 |
| T-P0.3 | **协议代次 bump**（D-72 拆分）：`engine_protocol.py` 拆 `models_revision`/`protocol_revision`；**同步更新 `PROTOCOL.md`、hello fixture、Rust `EngineIdentity` + `mismatches_against_frozen()`、compatibility `StructureRereadIdentity` 序列化形状、result 展示段、代次注释**；`REREAD_RULES_GENERATION` **保持 4**（Rust 侧规则未变） | 旧 ON 产物经 protocol_revision 不匹配被拒复用；OFF 路径字节不变；hello fixture 单测更新 |
| T-P0.4 | **回归验证**：非 throw 的 21 个冻结区域重读 HTML 逐字节一致；OFF 全集字节兼容不受影响（抽样）；smoke 真值表更新重跑 | 21/21 字节一致；真值表 PASS |

### P2a（工具前置）：layout sidecar regions 模式

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P2.0a | **sidecar regions 生成模式**（D-71②，P1 战役输入工具）：`sidecar/table-engine` 扩展 regions 入口（输入 pdf → 输出 `<stem>.regions.json`，schema 同 route A 文件 + manifest 握手）；对 458 页全集生成并**冻结**（逐文件 sha 入档 `analysis-m43-regions-freeze/`） | 生成文件过 RBLK-1 全部 provenance 校验；458 页成功率 + 逐页耗时落档；冻结清单落档 |

### P1：M4.3 部分缺表救援（最终代次 G ∈ {5,6}）

| 任务 | 内容 | 验收 |
|---|---|---|
| T1 | **人群再冻结**：m42-gc-on 产物（当前基线）× **P2a 冻结 regions** 逐页对账（pairing-check），冻结部分缺表页清单 + 未配对区域 + GT 零分 key 映射（预期 ~17 页/20 零分 GT，以实测为准落 `analysis-m43-population-freeze.json`）；**附 regions 来源对比**：P2a 冻结 regions vs 既有 r3 sidecar 逐页 det_N 差异表落档（触发域重叠率入档——两源不一致时解释并冻结处置） | 冻结清单含逐页 (det_N, block_N, 候选区域, GT key)；与 1.3 预冻结口径差异 + regions 源差异解释落档 |
| T2 | **触发/配对/插入/去重契约实现**（D-69/D-70/D-75/D-76，TDD）：触发分支（det>block≥1）；尾部未配对候选抽取；dedup-vs-emitted 门（D-75 唯一算法 + 手算 fixture：Jaccard ≥0.60 边界两侧、零重叠、地板拒、空比较域 fail-closed、`dup_undecidable` 计数）；插入持久化（D-76 固定锚点、det 序数全序、md/page_json 镜像、零删除）；**`trigger_state{qualified, arm, base_block_n, shape}` 持久化**（D-71④，重读前摘要入 checkpoint/sidecar）；计数器扩展 `f6_partial_*` 系；`REREAD_RULES_GENERATION` 4→5（校准后按 D-75 升 6） | 单测 fixtures：首表缺失（**预期不救援**，落档不重复插入断言）/中部缺失（同）/尾部缺失/检测序与输出序分叉/重复候选（dedup 拒）/多已发射表比较/空比较域 fail-closed/无正文页插入 + **D-75 手算 fixture**；既有表格与非目标正文逐字节不变；md/page_json 导出顺序一致 |
| T3 | **N 页 spike**（冻结人群）：重读运行 → 官方逐表 TEDS → stage-A 归档（dedup Jaccard 分布 + 裁定线分布）→ **采纳表 100% 人工复核**（对照页图） | **判据（预注册，全分母）**：① GT 可配对零分 key 救援率（TEDS ≥ 0.5）/ 零分 key 总数 ≥ 60%；② 采纳表人工复核伪表 = 0；③ text 分量不低于基线；④ 已发射表逐字节不变。**若校准（仅 Rust dedup 阈值）→ gen 6 + 5↔6 双向不复用回归 + 重跑全判据（D-75）** |
| T4 | **全集四门**：G-A' OFF 字节不变；G-B' 冻结 key 改善/退化；G-C' **Overall ≥ 91.50 且 TEDS 不降**（vs M4.2 后基线）；G-D' 成本入档。**战役 regions = P2a 冻结 regions；G-C' 证据 run = Run α（代次 `G`，T3 裁决后确定），服务 D-78 三方链** | 四门全 PASS 才转正；失败按 D-30 矩阵精神降级 |

### G0：引擎 GPU 迁移（上线准入，D-79/D-80）

| 任务 | 内容 | 验收 |
|---|---|---|
| T-G0.1 | **隔离验证**：独立 venv 装 `paddlepaddle-gpu==3.3.1`（cu126）+ paddleocr 3.7.0 + paddlex 3.7.2；`paddle.utils.run_check` GPU 可用性；27 区域 truth-table 在 GPU 下重跑（真值表期望 = GPU 时代重冻结） | GPU 推理成功；`dca64e05 r0`（1099s CPU）GPU 实测耗时落档（预期 <600s）；HTML 与 CPU 时代逐字节差异账本落档（结构性稳定 / 字节级漂移如实记录） |
| T-G0.2 | **供应链重冻结**（D-79）：worker device 切换（gpu；env 可覆写回 cpu）+ `protocol_revision` bump（proto3）+ `EngineIdentity.device` 字段（hello + compatibility identity）+ manifest pinned 更新（paddlepaddle-gpu 3.3.1 + CUDA 口径）+ requirements.txt 更新 + PROTOCOL.md 同步 | hello 携带 proto3 + device=gpu；cpu-era/gpu-era 产物互不复用（resume 回归）；REREAD_RULES_GENERATION 不变（Rust 规则未动） |
| T-G0.3 | **门禁重验**：smoke 真值表（GPU 期望 = T-G0.1 的重冻结结果）+ OFF 字节兼容抽样 + fmt/clippy + 全单测 | 全绿落档 |
| T-G0.4 | **性能账本（G-D' 更新）**：GPU 逐区域耗时分布 + 引擎 RSS；T3 spike 17 页 GPU 重跑（正式 spike 判据①②③④以此为准） | 成本入档；spike 判据①（≥60%）以 GPU 数值裁决 |
| T-G0.5 | **正式 spike 裁决**：stage-A/B 以 GPU 产物重出 → 判据①②③④全判 → GO 才进 P1/T4 | 裁决记录落 `docs/review/` 评审 |

### P2b：默认化 = CLI 自动 regions 接线 + 三态 + 惰性 spawn + resume 状态机

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P2.0b | **CLI 接线（D-71②）**：缺省/显式 on 且无 `--layout-regions` → resume 分类与页面循环前整文档生成 regions 至 `<out>/regions/<stem>.regions.json` → 既有 provenance 校验 → route A 管线；多文档逐文档解析；`--layout-regions` 显式提供时不生成 | 与 T-P2.0a 同生成器同输出：夹具文档断言生成的文件与 P2a 冻结产物逐字节一致（确定性复现） |
| T-P2.1 | **三态 + 惰性 spawn + 状态机实现（D-71③④，TDD）**：三态解析；layout regions 前置生成；SLANet lazy spawn（首个合格触发页，含 resume 比对到达的触发页）；availability 三态记录与序列化（含 `degraded{reason, from_page}`）；`trigger_state` 持久化读取；降级路径（缺省态启动/握手/生成失败 → 整文档 off + summary 告警 + identity；显式 on 硬错误不变）；缺省态运行中死亡 → 前页保留 + from_page 起降级 | 单测：三态 × 引擎可用/不可用矩阵；状态机全转移；**trigger_state 四类判别用例**（零块不合格/empty·raw 合格/部分缺表/零采纳但已触发）；死亡于"≥1 张已采纳之后"的边界；availability 翻转分类规则逐条断言；显式 on 启动失败硬错误回归 |
| T-P2.2 | **验收矩阵（D-78 三方链 + BLOCK-1③ + BLOCK-3③）**：<br>· **Run β（P2b 最终二进制显式 ON 全集 run，同 P2a 冻结 regions、同代次 G）**：门 β≡α——md/page_json 1651/1651 逐字节一致 → **S(β) = E(α)**；不一致 → 对 β 直接官方评测，**S(β) = E(β)，E(α) 此后禁用**；S(β) 须过 D-78 fallback 质量门（绝对 + 相对），未过 = P2b FAIL。<br>· **Run γ（缺省 best-effort 全集 run，自动 regions）**：门 γ≡β——① regions 等价 458/458 逐文件字节一致；② md/page_json 1651/1651 一致；③ sidecar/result/checkpoint 字段级 diff（仅 mode/availability 差异 + `reread_rules_generation == G`）。三项全过 → **S(γ) = S(β)**（β≡α 已失败时即继承 E(β)，不回退 E(α)）；任一失败 → 对 γ 直接官方评测，S(γ) = E(γ)，同样须过 fallback 质量门。<br>· **验收记录** `analysis-m43-p2b-acceptance.json`：`regions_equal` / `beta_alpha_equal` / `gamma_beta_equal` / `eval_source` / `adopted_scores` 全部落档（裁决可追溯到实际消费的 run）。<br>· **对照 B**：引擎不可用夹具（无 Python/坏模型 SHA 各一）缺省扫描完成、md/page_json == 显式 OFF 旧产物逐字节、result/checkpoint schema 断言。<br>· **对照 C**：显式 off 旧字节兼容回归。<br>· **CLI 编排矩阵（真实扫描）**：无 flag+无 regions+引擎可用（自动生成生效，断言种子触发文档 `f6_triggered > 0` 与产物表格数）/无 flag+引擎缺失（降级）/无 flag+显式 regions 文件（用文件不生成）/显式 on+自动（失败=硬错误）/显式 on+regions 文件（M4.2 既有行为）/缺 layout 资产（缺省降级 vs 显式硬错误）/**零触发文档**（regions 已生成、SLANet 未 spawn、availability==unprobed）。<br>· **真实 checkpoint resume 回归**：同环境 ON→缺省 与 缺省→ON 双向全复用（零 Reprocess）；可用→不可用（stored trigger_state 判定触发页 Reprocess、非触发页复用；旧产物无 trigger_state → 保守 Reprocess）；不可用→可用（反向）；死亡后 from_page 边界（含 ≥1 已采纳场景）；**发生校准时同引擎身份下 5↔6 双向不复用证据（D-75）**。 | 全矩阵 PASS 落档（含 β/γ 两 run 产物、门结果与采纳分数；fallback 评测未过质量门 = P2b FAIL）；README/runbook 更新：普通默认命令在 Python 引擎环境就绪时自动获得表格找回；引擎不可用时自动降级（告警可见）；layout 检测成本为重读活跃时的固有成本（逐页数字引用 T-P2.0a 实测） |

### P3：M4 弱表重开 spike

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P3.1 | **人群再冻结**：B0b 冻结带 key 清单从 B2 战役产物提取，**在 P2 代基线产物上重算 band 归属**，落 worst/mid/bad 集合与 worst/bad 子集分母；crop 重渲染（4096² 内，同 M4 链） | `analysis-m43b-weak-table-freeze.json`：逐表 key + band + 与 B2 v2 口径差异解释 |
| T-P3.2 | **spike 运行（D-77 完整链）**：F1–F5 触发（v1 冻结不动）→ 裁剪 → SLANet 重读 → **D-47 content_identity_pass** → 四元组字典序 compare-keep（平局保原）→ 官方配对评测（worst/bad 子集） | 跑通冻结集合；compare-keep 决策日志 + D-47 拦截记录 + 拒绝/伪表率全量落档 |
| T-P3.3 | **裁决（D-73 判据 ↔ 输入产物一一对应）**：① ΔTEDS ≥ +0.10 且 ≥20% 改善——输入 = T-P3.1 冻结清单 + T-P3.2 配对评测（分母 = 再冻结 worst/bad 集合，对照 = P2 代默认产物）；② 拒绝/伪表率——输入 = T-P3.2 日志；③ 链一致性——输入 = 实现检查。GO → **战役（另列预算）**：全集双扫描（control = P2 代默认产物 / candidate = +弱表臂）→ B2 五门（D-22 冻结原文：覆盖率 ≥80% / 替换率 ≤2% / **good 带配对均值 ΔTEDS ≥ −0.02 且显著回归 Δ < −0.10 ≤ max(3, 2%)** / worst-bad Δ ≥ +0.3 / T-PROJ+零漂移）→ 产品裁决；NO-GO → 战略关闭落档 | 裁决记录 + 各判据输入产物路径落 `docs/review/` 评审；战役为独立预算项 |

---

## 5. 风险

| 风险 | 缓解 |
|---|---|
| P0 根因在 Paddle 内部不可修 | 损失上限 = 5 个 GT key 维持 0（今天已零损害）；降级为干净 reject 语义不变 |
| P1 伪表（newspaper 93-det 页 det 噪声） | 裁定线门（27/27 校准继承）+ dedup 门 + spike 100% 人工复核；生产态 = 裁定线 + 结构有效性 + dedup 三门（D-70），newspaper 页复核加严 |
| P1 序数错位（模型跳发首/中部表） | **如实预期：不救援被错误配对吞掉的区域**（dedup 仅 fail-closed 防重复插入，不纠正配对，D-76）；损失入 T3 判据①分母如实计 |
| P1 text 分量回归（插入表块挤占） | T3 判据③预注册；M4.2 同型判据曾 PASS |
| P2a/P1 自动 regions 与既有 r3 regions 质量不一致 | T1 regions 来源对比表落档（逐页 det_N 差异 + 触发域重叠率）；不一致即解释并冻结处置，不带病进入战役 |
| P2 跨环境 regions 生成不确定（字节不等） | D-78 门 γ≡β ① 失败 → 对 γ 直接评测 + fallback 质量门（未过 = P2b FAIL）；不以显式 regions 对照替代默认路径质量验收 |
| P2 fallback 直接评测分数回退（β/γ 与 α 不同源时） | fallback 质量门双保险：绝对（Overall ≥ 91.50 且 TEDS 不降）+ 相对（对 E(α) Δ ≥ −0.05 / −0.005，噪声容忍口径冻结于 D-78）；未过 = P2b FAIL，不得以"已评测"豁免 |
| P2 跨环境字节不稳定（同一输入，有/无引擎环境产物不同） | 口径如实落档：字节稳定性以**同环境**定义；identity 记录 availability + trigger_state，跨可用性按状态机 Reprocess（真实 checkpoint 回归覆盖） |
| P2 惰性 spawn 改变 D-63② 语义 | 显式 on 保持 SLANet eager+硬错误不变；layout regions 生成前置对两种形态一致（D-71②时序统一）；状态机与 resume 规则冻结并有真实 checkpoint 回归 |
| P3 good 带回归重演 | compare-keep 平局保原 + D-47 内容身份门（D-77）+ 战役门②③（D-22 原文公式）复核 |
| P3 spike 通过但五门战役再 NO-GO | 预算有界（spike 2 日 + 战役 2–3 日）；NO-GO 即双机制证伪，弱表线长期关闭，收益账清零（D-30 精神） |

## 6. 成本估算

| 相位 | 工程 | 算力 |
|---|---|---|
| P0 | 1–1.5 日（诊断 0.5 + 修复/身份拆分 0.5–1） | 16 页触发域重跑 + smoke（<0.5h） |
| P2a | 1–1.5 日（sidecar regions 模式 + 冻结） | 458 页检测一次（逐页耗时实测入档） |
| P1 | 5–6 日（T1 0.5（含 regions 源对比）/ T2 2 / T3 1 / T4 1–1.5） | spike：裁定线过滤后预估 30–60 crop × 186s ≈ 2–3h；全集双扫描 + 评测 ≈ 数小时 |
| G0 | 1.5–2 日（隔离验证 0.5 + 重冻结/身份 0.5 + 门禁重验 0.5–1） | 27 区域 truth-table GPU 重跑（预计 <30 分钟引擎时间）+ 17 页 spike GPU 重跑 |
| P2b | 2.5–3 日（T-P2.0b 接线 0.5 / T-P2.1 状态机 1–1.5 / T-P2.2 矩阵 1） | **Run β 显式 ON 全集 run（三方链中继）+ Run γ 缺省全集 run**（β≡α 与 γ≡β 门成立时评测复用 E(α)，不重评；任一门失败则对应 run 直接官方评测）≈ 数小时×2 |
| P3 | spike 2 日（+GO 时战役 2–3 日，另列） | spike ≈ 99 表 × 186s ≈ 5h CPU；战役 ≈ 全集双扫描 + 评测 |

## 7. 修订记录

- **v0.10（2026-11-27，代码评审响应——BLOCK×2 + 非阻断×4 全部修复）**：**BLOCK-1** → test_protocol.py 升 proto3/device/3.3.1（16/16 绿）+ engine_protocol.py `EXPECTED_DEVICE`/`EXPECTED_PADDLEPADDLE = 3.3.1`（Python 守卫对齐 Rust/requirements）。**BLOCK-2** → 双修复：commit slot 钉扎补写 `reread_rules_generation = 5`（与 identity 同步入 checkpoint）；resume 路径惰性引擎升级为 eager（分类前解析精确身份，spawn 失败仍走 best-effort 降级）。端到端实证：checkpoint gen=5 + identity 钉扎 + resume 1.8s 复用。**非阻断×4** → regions 路径存在性校验恢复（显式 on 手滑路径报错）、部分臂 adopted candidates 加入 D-75 比较域、multi 同名 stem 防护、String::as_str closure。**环境发现** → GPU OCR-rec 空白级跨 run 方差（密集 crop "Note3"/"Note 3"）——smoke 断言归一化；CUDNN 9.9/9.5 错配入 G0 账本。**D-71④ 口径如实记录**：spawned 文档 on↔default 复用已实现；unprobed 文档跨模式保守重处理（惰性分类前无身份可匹配——lazy-probe-at-classify 为后续细化项）。
- **v0.9（2026-11-27，P3 双臂 spike 裁决）**（2026-11-27，P3 双臂 spike 裁决）**：**FAIL——弱表线永久关闭**。双臂实测（GPU 引擎，D-47+四元组链）：Arm A（SLANet）worst/bad 18 对均值 **−0.27**；Arm B（OvisOCR3 F1–F5）26 对 **−0.19**；best-of-both 37 对 −0.25。改善比例 27–31% 达标但被严重退化抵消（−0.5~−0.77：结构合理的重读替换了可接受表但内容排序错误——四元组 compare-keep 非 TEDS 感知，无法约束）。三机制证伪闭环：ovisocr2 裁剪（Δ≈0）+ ovisocr3 裁剪（−0.19）+ slanet 裁剪（−0.27）。**交互警告**：armB 使 P1 已救页（eastmoney_7f55）退化 −0.73——两臂永不共启。用户指令（救援探针常设化）保持：探针机制留存，每次接入新模型复跑一遍作例行体检，但弱表自动替换线不再开启。工件：`analysis-m43-p3-dual-arm-verdict.json`。
- **v0.8（2026-11-27，执行记录更新）**（2026-11-27，执行记录更新）**：P0/P2a/P1/G0/P2b 全部执行完成——P1 四门 PASS（91.50→91.72）；P2b-4 验收 PASS（β≡α 1651/1651、γ≡β md 1651/1651 + Overall 91.72 默认路径复现、regions era-drift 如实记录含 GPU-era 确定性证明、降级/严格/resume 夹具全过）。P3 待执行。事故记录：checkout 曾静默切回 main（已恢复，24 提交完好，main 复位 origin）。
- **v0.7（2026-11-26，T3 裁定落档（2026-11-26，T3 裁定落档——用户选项 C）**：P1/T3 spike 在冻结判据下 ① FAIL（4/20 = 20% < 60%；分解：真漏子群东财 4/9 = 44% 全部 ≥0.84 高分带，另 11 张零分 key 为弱表人群非缺表），②③④ PASS（17 采纳 / text 零劣化 / 已发射表零删除）。**用户裁定选项 C**：放弃 60% 单门，改实效组合判据——零回归（已证）+ 真漏子群 4/9 救援且全部 ≥0.84 + 全集 G-C' Overall/TEDS 不降（T4 最终条款）。T1 人群定义缺陷如实记录（零分 key 混入弱表人群）。部分缺表臂随 T4 过门投产；GPU 数值与 CPU 预演完全一致。
- **v0.6（2026-11-26，用户指令修订）**（2026-11-26，用户指令修订）**：**GPU 迁移提级为上线准入**——用户裁决"不上 GPU 不能上线（1000+ 秒用户场景不可接受），上线前必须改 paddlepaddle-gpu 并测完"。新增相位 **G0**（隔离验证 / 供应链重冻结 / 门禁重验 / 性能账本 / 正式 spike 裁决）与决策 **D-79**（供应链口径：paddlepaddle-gpu 3.3.1 cu126——3.4.0 无 Windows GPU wheel 如实记录；models/paddleocr/paddlex 不变；proto3 + device 入身份；CPU 通道保留为回退）与 **D-80**（G0 先于 P1/T4，避免全集双跑；CPU spike 降级为机制预演）。非目标清单相应收窄（ONNX 路线仍留后续）。执行顺序更新为 P0 → P2a → P1(T1–T3) → G0 → P1(T4) → P2b → P3。
- **v0.5（2026-11-25，三轮重审响应——R3BLK×1）**（2026-11-25，三轮重审响应——R3BLK×1）**：D-78/T-P2.2 分数分派改为唯一规则——β≡α 成立 → S(β)=E(α)，失败 → S(β)=E(β) 且 E(α) 禁用；γ≡β 三项全过 → S(γ)=S(β)（β≡α 已失败时继承 E(β)，不回退 E(α)），任一失败 → S(γ)=E(γ)；fallback 质量门冻结（绝对 Overall ≥ 91.50 且 TEDS 不降 + 相对对 E(α) Δ ≥ −0.05/−0.005 噪声容忍），直接评测未过门 = P2b FAIL；验收记录字段（regions_equal / beta_alpha_equal / gamma_beta_equal / eval_source / adopted_scores）落 `analysis-m43-p2b-acceptance.json`。风险表补 fallback 回退行。
- **v0.4（2026-11-25，二轮重审响应——R2BLK×2 + 非阻断×2）**：① **R2BLK-1** → D-78 改三方证据链（评审选项 2）：Run α = P1 二进制显式 ON 证据 run（评测 E(α)）；Run β = **P2b 最终二进制显式 ON 全集 run**（门 β≡α：md/page_json 1651/1651 一致 → 复用 E(α)，不一致则直接评测）；Run γ = 缺省 best-effort run（门 γ≡β：regions 等价 458/458 + 内容一致 + 字段级 diff → 分数 = E(α)）；α 与 γ 的等价性只经 β 传递，不得称"同二进制对照"；相位表 P2b、T-P2.2、成本（+Run β 一次全集扫描）同步。② **R2BLK-2** → 最终代次 `G ∈ {5,6}`（T3 裁决确定：未校准=5、校准=6）贯穿 D-78/T4/T-P2.2——α/β/γ 同一 G、字段级 diff 断言 generation==G、resume 矩阵保留 5↔6 互斥证据、禁止以校准前 gen 5 spike 产物充当最终对照。③ 非阻断×2：D-75 fixture 改"五例"；D-71④ 注明 Available 仅在 trigger_state 合格页按需解析、非触发页保持 Unprobed 不强制 spawn。
- **v0.3（2026-11-25，重审响应——RBLK×4 + 非阻断×4）**：① **RBLK-1** → D-71② 时序统一：layout regions 一律在 resume 分类与页面循环前整文档生成（触发谓词依赖 det_N，检测成本为能力固有），仅 SLANet 引擎惰性（缺省首个合格触发页 spawn；显式 on eager）；缓存路径改为 route A 契约 `<regions-root>/<stem>.regions.json`；"零开销"措辞改为"零 SLANet 启动/握手开销"；零触发文档验收状态入 T-P2.2。② **RBLK-2** → D-71④ 三处重写：resume 配置类改为语义分类（on↔default 同属 active，配置一致即复用；active↔off 互斥），T-P2.2 补 ON↔缺省双向回归；新增 `trigger_state{qualified, arm, base_block_n, shape}` 重读前摘要持久化，禁止从救援后 page_json 反推，旧产物保守 Reprocess；删除"死亡前均非触发页"错误假设，死亡边界改为"前页（含 Available 产物）保留 + from_page 起降级"，边界测试入 T-P2.1/T-P2.2。③ **RBLK-3** → 证据链选项 1 冻结（D-78）：sidecar regions 模式前置为 P1 T4 输入工具（新相位 P2a / 任务 T-P2.0a），P1 战役与 P2 对照 A 消费同一次冻结 regions；对照 A 增加 regions 等价门（458/458 逐文件字节一致），字节不等时该环境按选项 3 全集重评；T1 增加 regions 来源对比表。④ **RBLK-4** → D-75 集合构造唯一算法冻结（单元格规范化到小写为止、A/B = 单元格字符串集合、|A| = 单元格数、不做 bigram）+ 手算 fixture（0.60 边界两侧/零重叠/地板拒）；代次语义冻结（未校准 = gen 5；Rust dedup 校准 → gen 6 + 5↔6 双向不复用 + 全门重跑；裁定线校准另 bump protocol_revision）；D-75/T3 校准范围统一为仅 Rust dedup 阈值。⑤ 非阻断×4：D-76 "自校正"改为"fail-closed 防重复插入、不主张配对纠正"，首/中部 fixture 预期 = 不救援；D-70 生产态门显式编号（裁定线 + 结构有效性 + dedup，人工复核仅属 spike）；T-P3.3 恢复 B2 门③原文公式 ≤ max(3, 2%)；相位表 P3 补 P2 依赖（对照 = P2 代默认产物）。⑥ 成本重估：新增 P2a 1–1.5 日，P2b 调整为 2.5–3 日。
- **v0.2（2026-11-25，评审响应——BLOCK×6 + 非阻断×2）**：BLOCK-1 → D-71 自动 regions 生成（评审选项 2）+ CLI 编排矩阵；BLOCK-2 → D-78 字节验收矩阵（同二进制 gen 5 对照、降级门限定页面内容产物）；BLOCK-3 → D-71④ 惰性 spawn 状态机 + 真实 checkpoint 回归；BLOCK-4 → D-75 dedup 公式 + D-76 插入契约 + fixtures 扩充；BLOCK-5 → D-77 P3 链保留 D-47；BLOCK-6 → D-73 spike/战役判据分离；非阻断：bae6e6f8→bae6e5f8 笔误、T-P0.3 同步清单。
- **v0.1（2026-11-25）**：初稿。四相位结构、D-69–D-74、事实基线取自 f6-ledger-458 / analysis-b2-adjudication-v2 / analysis-m42-det-spike-production-freeze / G-B adjudication。
