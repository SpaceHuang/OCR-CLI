# M4.3 表格救援扩展、引擎修复与默认化执行计划（2026-11-25）

**文档状态：** v0.1（初稿，待评审。承接 `m4-2-table-structure-reread-plan-2026-11-22.md` v1.6——M4.2 已随 PR #17 合入 main，四门 PASS，Overall 91.50）

**范围：** 用户裁定的四项打包：① M4.2 默认开关翻转；② M4.3 部分缺表救援（原开放项 O-5）；⑥ 引擎侧确定性 throw 修复；③ M4 弱表重开 spike（B2 新机制假设重试）。按 **P0 → P1 → P2 → P3** 相位执行，每相位独立门禁，失败即停在相位内。

---

## 1. 事实基线（全部落档可查）

### 1.1 M4.2 合并后现状

- main @ `2d24fb2`（PR #17），Overall **91.50** vs 前基线 90.87（+0.63），table TEDS +0.0183（G-C，`analysis-m42-gc-final-adjudication.json`）。
- `--table-structure-reread` **默认 off**（`src/ocrx-cli/src/main.rs:108`）——91.50 只在显式开 flag 时成立；默认路径用户拿不到该价值。
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
| **P2** | 默认开 + 三态语义 + 惰性 spawn（D-71） | P1（一次全集验证同时覆盖两代触发域） | 91.50+ 的价值对默认用户生效 |
| **P3** | M4 弱表重开 spike（SLANet 机制假设） | P0（引擎稳定）；与 P1/P2 人群不相交 | 打最弱分量 TEDS 的剩余空间（worst/bad 带 52 表池） |

**明确不做（本计划非目标）：** M5 公式区域重读；M6b text-only 区域管线；SLANet ONNX/GPU 化（P0–P3 全过后按触发域规模再立项）；F1–F5/G 触发阈值与 B2 五门的任何重校准（D-29 校准政策不为本计划启用）；重训/微调；评分器与数据集 revision。

---

## 3. 决策记录（承接 D-68）

| # | 决策 | 依据 |
|---|---|---|
| D-69 | **M4.3 触发域扩展**：F6 触发从"block==0"扩展为"同页 det_N > output_N（含 =0 特例）"；候选 = RBAC 类序数配对的**尾部未配对 det 区域**（配对复用 R2 pairing 参考实现语义）；整页特例沿用 M4.2 形状资格不变 | RBAC ③ 本就定义 detection_N > output_N = miss 病例（不产新块）；M4.2 是其 output==0 极端特例；配对与拼接基建（pairing-check、D-56/D-68 排序）全部现成 |
| D-70 | **M4.3 准入链**（部分缺表无锚文本，cov_attempt 归因公式不适用）：裁定线门（继承 27/27 校准）→ SLANet 重读 → **dedup-vs-emitted 门**（新：归一化单元格文本与同页已发射表重叠超阈值 → 判 duplicate 拒绝）→ spike 阶段 100% 人工复核，生产态 = ①+② | 整页特例的 cov_attempt 依赖 raw_single 整段 attempt 文本，部分缺表页不存在该锚；M4.2 empty 臂先例 = 无锚靠结构门+人工复核；伪表防线（conf 0.984 伪表被裁定线门拒绝的 27/27 校准）继承并加 dedup |
| D-71 | **默认开三态语义**：`--table-structure-reread` 增加**缺省 = best-effort on**——引擎可用性探测（惰性 spawn 至首个触发页，零触发页零开销）失败/握手不匹配 → **整文档降级 off**，summary 告警 + identity 记录 degraded；**显式 `on` 维持 D-63② 严格语义**（启动类故障=硬错误）；显式 `off` 不变 | 默认路径不能因用户环境缺 Python/Paddle 而硬失败（破坏所有现存用户）；显式 opt-in 用户保留 fail-fast 契约；惰性 spawn 使默认开对零触发页零启动开销（否则每次扫描白付 ~10–30s 握手） |
| D-72 | **引擎身份拆分**：`revision` 拆为 `models_revision`（= manifest revision bae6e6f8 系，模型目录 SHA）+ `protocol_revision`（worker 行为代次）。P0 只 bump protocol；manifest 文件与模型 SHA 不动。resume 比对两者 | worker 侧行为修复（无模型变化）不应触发模型 manifest 重冻结；身份链仍 fail-closed（任一不匹配不复用） |
| D-73 | **P3 spike 判据预注册**（弱表 SLANet 重读，compare-keep 即门，无 cov_attempt）：① worst/bad 带配对均值 ΔTEDS ≥ **+0.10** 且改善表占比 ≥ 20%；② 触发的 good 带表 compare-keep 后官方配对 ΔTEDS ≥ −0.02 且显著回归（Δ < −0.10）≤ 2；③ 拒绝/伪表率入档；④ T-PROJ 全集投影落档。**GO → 按 m4-table-reopen R3/B2 五门战役（D-22 冻结门不重校准）出产品裁决；NO-GO → 弱表路径记战略关闭（VLM 与 SLANet 双机制假设均证伪）** | B2 v2 实测 worst/bad Δ≈0 是 VLM 机制证伪，非触发/比较契约问题；SLANet 是 B2 处置原文点名的机制假设；+0.10 spike 线低于产品门 +0.3，留出机制→产品的余量 |
| D-74 | **执行顺序 P0→P1→P2→P3**，每相位独立门禁，失败停相位：P0 修复直接改善 P1 人群页重读成功率；P2 全集验证一次覆盖 P1 新触发域；P3 人群与 P1/P2 不相交可后置 | 相位间最小重跑原则（避免 P2 验证两次全集） |

---

## 4. 任务分解

### P0：引擎侧确定性 throw 修复 + 身份拆分

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P0.1 | **重枚举 + 根因诊断**：在 gen-4 生产渲染像素上重放 6 区域（dca64e05、1417 r1–r2、3713 r0–r2），worker 开 traceback 捕获完整栈；对照 smoke 时代 9 错误帧清单核对当前确定性集合（T0 probe crop 与生产 crop 清单曾有出入，以生产为准） | 6 区域 IndexError 栈落档，根因定位到 Paddle 管线具体环节（疑：cell-det 空框/方向分类小图/零框 rec 下游索引） |
| T-P0.2 | **worker 侧修复**：按根因加 guard/恢复路径（候选：最小尺寸 pad、跳过方向分类、空框短路返回空结构），恢复输出走同一条裁定线门→重读→归因链；不可恢复则干净 reject（`pipeline_throw` → 保原，语义同今天，零损害） | 6 区域中可恢复者产出 HTML 且过全部既有门 + 人工复核；其余保持保原 |
| T-P0.3 | **协议代次 bump**（D-72 拆分）：`engine_protocol.py` 拆 `models_revision`/`protocol_revision`，Rust `EngineIdentity` + `mismatches_against_frozen()` + compatibility `StructureRereadIdentity` 同步；`REREAD_RULES_GENERATION` **保持 4**（Rust 侧规则未变） | 旧 ON 产物经 protocol_revision 不匹配被拒复用；OFF 路径字节不变（无引擎身份字段） |
| T-P0.4 | **回归验证**：非 throw 的 21 个冻结区域重读 HTML 逐字节一致；OFF 全集字节兼容不受影响（抽样）；smoke 真值表更新重跑 | 21/21 字节一致；真值表 PASS |

### P1：M4.3 部分缺表救援（gen 5）

| 任务 | 内容 | 验收 |
|---|---|---|
| T1 | **人群再冻结**：m42-gc-on 产物（当前基线）× 现有 458 页 regions sidecar 逐页对账（pairing-check），冻结部分缺表页清单 + 未配对区域 + GT 零分 key 映射（预期 ~17 页/20 零分 GT，以实测为准落 `analysis-m43-population-freeze.json`） | 冻结清单含逐页 (det_N, block_N, 候选区域, GT key)；与 1.3 预冻结口径差异解释落档 |
| T2 | **触发/配对/插入契约实现**（D-69/D-70，TDD）：触发分支（det>block≥1）；尾部未配对候选抽取；dedup-vs-emitted 门；插入持久化（page_json 按 det 序位置插入、md 插入点冻结、零删除契约沿用 D-54）；计数器扩展 `f6_partial_*` 系；`REREAD_RULES_GENERATION` 4→5 | 单测：部分缺/序数错位（重读区域实为已发射表→dedup 拒）/伪表（裁定线拒）/多表插入序 4 类 fixture；非目标正文逐字节不变；md/page_json 一致 |
| T3 | **16→N 页 spike**（冻结人群）：重读运行 → 官方逐表 TEDS → **采纳表 100% 人工复核**（对照页图，人群小可全查） | **判据（预注册，全分母）**：① GT 可配对零分 key 救援率（TEDS ≥ 0.5）/ 零分 key 总数 ≥ 60%；② 采纳表人工复核伪表 = 0；③ text 分量不低于基线；④ 已发射表逐字节不变。未达 → D-29 一轮校准可用（仅 dedup/裁定线阈值），校准后重跑全判据 |
| T4 | **全集四门**（同 M4.2 形态）：G-A' OFF 字节不变；G-B' 冻结 key 改善/退化；G-C' **Overall ≥ 91.50 且 TEDS 不降**（vs M4.2 后基线）；G-D' 成本入档（186s/表 × 新增 crop 数） | 四门全 PASS 才转正；失败按 D-30 矩阵精神降级（能力不进目标命令） |

### P2：默认开 + 三态语义 + 惰性 spawn

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P2.1 | **三态实现**（D-71）：CLI flag 三态解析（缺省/on/off）；惰性 spawn（首个触发页前探测）；降级路径（探测失败 → 整文档 off + summary 告警字段 + identity `degraded` 标记）；显式 on 维持 eager + 硬错误 | 单测：三态矩阵 × 可用/不可用引擎 fixture；降级产物 == 现 OFF 产物逐字节；显式 on 启动失败硬错误回归 |
| T-P2.2 | **全集验证**：默认（无 flag）全集 run vs m42-gc-on（显式 ON）逐字节一致（md 1651/1651 + 归因抽样）→ 评测分数直接复用 91.50+P1 增量；无 Python 环境夹具扫描完成且降级正确；resume：可用性变化 → identity 不匹配 → Reprocess；金集代次内零差异 | 1651/1651；降级夹具 PASS；README/runbook 默认命令更新（Q7.1 载体） |

### P3：M4 弱表重开 spike

| 任务 | 内容 | 验收 |
|---|---|---|
| T-P3.1 | **人群冻结**：B0b 冻结带 key 清单（worst/mid/bad 池 safe-eligible 99 表）从 B2 战役产物提取落档；crop 重渲染（4096² 内，同 M4 链） | `analysis-m43b-weak-table-freeze.json`；99 表逐表 key + band |
| T-P3.2 | **spike 运行**：F1–F5 触发（v1 冻结不动）→ 裁剪 → SLANet 重读 → 四元组字典序 compare-keep（平局保原）→ 官方配对评测 | 跑通 99 表；compare-keep 决策日志（换/留 + 理由）落档 |
| T-P3.3 | **裁决**：按 D-73 四判据出 GO/NO-GO；GO → 立即按 m4-table-reopen R3/B2 五门战役（D-22 门冻结，含覆盖率 ≥80%/替换率 ≤2%/投影+零漂移）出产品裁决；NO-GO → 战略关闭落档 | 裁决记录 + 投影落 `docs/review/` 评审 |

---

## 5. 风险

| 风险 | 缓解 |
|---|---|
| P0 根因在 Paddle 内部不可修 | 损失上限 = 5 个 GT key 维持 0（今天已零损害）；降级为干净 reject 语义不变 |
| P1 伪表（newspaper 93-det 页 det 噪声） | 裁定线门（27/27 校准继承）+ dedup 门 + spike 100% 人工复核；生产态依赖前两者，newspaper 页单独复核加严 |
| P1 序数错位导致重读已发射区域 | dedup-vs-emitted 门兜底（duplicate 拒绝），浪费成本可接受（186s/误配区域） |
| P1 text 分量回归（插入表块挤占） | T3 判据③预注册；M4.2 同型判据曾 PASS（garbled 文本退出 text 计分，持平或改善） |
| P2 跨环境字节不稳定（同一输入，有/无引擎环境产物不同） | 口径如实落档：字节稳定性以**同环境**定义；identity 记录 availability，跨可用性 resume → Reprocess |
| P2 惰性 spawn 改变 D-63② 语义 | 显式 on 保持 eager+硬错误不变；只有缺省态惰性化；契约差异在 D-71 写明 |
| P3 good 带回归重演（B2 门③曾 PASS 但替换率超限） | compare-keep 平局保原 + D-73 判据②；五门战役时门②（替换率 ≤2%）原样复核 |
| P3 spike 通过但五门战役再 NO-GO | 预算有界（spike 2 日 + 战役 2–3 日）；NO-GO 即双机制证伪，弱表线长期关闭，收益账清零（D-30 精神） |

## 6. 成本估算

| 相位 | 工程 | 算力 |
|---|---|---|
| P0 | 1–1.5 日（诊断 0.5 + 修复/身份拆分 0.5–1） | 16 页触发域重跑 + smoke（<0.5h） |
| P1 | 4–5 日（T1 0.5 / T2 1.5–2 / T3 1 / T4 1–1.5） | spike：裁定线过滤后预估 30–60 crop × 186s ≈ 2–3h；全集双扫描 + 评测 ≈ 数小时 |
| P2 | 1.5–2 日 | 全集 default run（评测复用字节一致性，不重评）≈ 数小时 |
| P3 | spike 2 日（+GO 时战役 2–3 日） | 99 表 × 186s ≈ 5h CPU |

## 7. 修订记录

- **v0.1（2026-11-25）**：初稿。四相位结构、D-69–D-74、事实基线取自 f6-ledger-458 / analysis-b2-adjudication-v2 / analysis-m42-det-spike-production-freeze / G-B adjudication。
