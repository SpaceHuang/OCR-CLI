# M4.2 专用表格结构重读（SLANet-plus 臂）开发计划 v1.4

日期：2026-11-22（v1.0 同日评审 NO-GO 5 项阻断，本文为修订版）
分支：feat/next-phase-quality
前置：M4.1 F6 计划（T4 spike FAIL，0/24 采纳）；M4.2 oracle spike 裁决（GO）
评审记录：`docs/review/m4-2-table-structure-reread-plan-2026-11-22-review.md`（v1.0 NO-GO，BLK-1..5）；`-re-review.md`（第二轮，attribution 缺口）；`-re-review-2.md`（第三轮，RBLK-2 generation）；`-re-review-3.md`（第四轮，复核 RBLK-2 未落地）
状态：v1.4 修订完成，待重审

**v1.3 → v1.4 修订摘要（T4 生产实测发现：排序歧义跳过吞掉 8 张最高价值表）**

T1–T3 已实施（`bd2f98f`，710 测试全绿）并完成 T4 生产触发域实测：
OFF 逐字节一致（触发页 16/16 + 全集 md 1651/1651 + 归因抽样 20/20）；ON 端到端
走通（真实 sidecar 子进程、握手、表格线门、attribution、持久化；一次 engine_error
按 D-63③ 兜底保原）。但 **D-56 排序歧义跳过 + 纵向重叠 veto 在生产路径吞掉
8 张已验证可恢复的表**（两个东财 2×2 表格网格页 + 3713 页）——T0 的 det spike
按文件序喂入，从未经过该门；生产可达采纳 7/24 < G-B 门 12/24，T4-ON 实测与
可达性模拟逐页吻合（证据：`C:/t/m23/m42_t4_reachability.py` 输出 + t4-report.json）。

**修订（用户裁决，2026-11-23）：空臂排序歧义不再跳页，改为坐标定序**。裁剪时每
区域的编号与 bbox 像素坐标全程已记录——卡住的只是"不敢在两种解释间拍板"，不是
信息缺失。空页（md 为空）不存在可被打乱的既有内容，D-56"防错序插入污染原文"的
理由不成立：

- **D-56（v1.4 修订）**：empty 臂排序歧义 / 纵向重叠 veto 时**不再整页跳过**，
  改用确定性坐标序：按 x 区间重叠 ≥0.3 切纵向栏带（列优先），栏带按左→右，
  栏带内按上→下；切不出多栏时退化为单栏上→下（与原一致序等价）。顺序决定与
  `f6_ordering_ambiguous=true` 标记一并入 f6_detail/stats（可审计）。raw 臂保持
  原严格跳过不变（按定义 det_N==1，实际不可达，纯防御）。**依据**：空页追加是
  纯新增内容；官方逐表 TEDS 匹配与顺序无关；被吞 8 表 T0 实测 TEDS 0.74–1.00，
  为全体最高价值恢复。**实现后不得再无代次调整排序规则（R2.5/D-42 继续适用）**。
- **T4 补充验收**：4 个跳过页重跑 ON——5f75/6e1c 应各采纳 4 张（md 相对 OFF
  变化）、1417/3713 应逐区域 empty_output 拒绝（md 仍一致）；dca64e05 的
  engine_error 为瞬态（重跑确认）。
- **构建环境教训（入 T6 档案）**：OFF 字节一致性对比必须同 feature 构建
  （`--features vulkan`）——裸 cargo build 得到 CPU 后端二进制，与 GPU 二进制
  对比会产生 logits 漂移假差异。
- G-B 门数值不变（≥12/24）；v1.4 下生产预测回到 15/24（62.5%）。

**v1.2 → v1.3 修订摘要（第三轮 RBLK-2：校准改规则 → 代次必须 +1）**

按 R2.5/D-42 既有契约"校准一轮 = `reread_rules_generation` + 1"，v1.2 执行
D-29 校准（cov_reread 移除）后仍写 generation 2 是违规——同一条 SLANet 开发链上
generation 2 无法表达校准前后两种采纳语义，旧产物会被错误复用。修订：

- `REREAD_RULES_GENERATION` 计划值 = **3**；代次语义记录：
  `2 = M4.1 双公式/VLM F6（未落地的 WIP，运行时不存在）`、
  `3 = M4.2 校准后 SLANet 规则（cov_attempt 单门 + 表格线门）`；
  常量注释同步更新，不沿用"维持 2"的旧结论（D-63）。
- T2/T4 增加 **generation 2/3 双向不复用**回归：generation 2 产物在 generation 3
  运行下不复用，反向亦然；覆盖 旧 flag ON/新 flag OFF（该组合 F6 不触发，行为
  变化无引擎身份字段可挡，只能靠代次）与 SLANet ON 两类路径（D-63/D-64/T2/T4）。
- 非阻断清理（第三轮）：D-59 删除"惰性 spawn"表述——统一为
  "flag ON 即扫描开始时 spawn 并握手（先于任何页面产物）；模型在 worker 内
  惰性加载"。

**v1.1 → v1.2 修订摘要（重审剩余阻断 + 非阻断修正）**

重审剩余阻断：v1.1 恢复了 raw 臂 attribution 双公式门，但 T0 数字（0.544、17/24、
raw 7/11）是**门前**数字，未扣除 attribution 拒绝；raw 子门（≥5/11）在门前证据下
无法判定。已用冻结的 11 份 raw attempt 补算并重算最终 adopted 集合：

| 口径 | 门前（v1.1 数字） | **门后（v1.2 冻结数字）** |
|---|---|---|
| raw 改善 | 7/11 | **6/11（55%，子门 ≥5 ✓）** |
| empty 改善 | 10/13 | **9/13（69%，子门 ≥40% ✓，逐区域口径修正）** |
| 总改善 | 17/24 | **15/24（62.5%，门 ≥12 ✓）** |
| 触发域均值 | 0.544 | **0.513（门 ≥0.40 ✓）** |
| 退化 | 0 | **0 ✓** |

attribution 校准（M4.1 D-54/D-29 预授权的一轮校准）：cov_reread ≥ 0.30 存在结构性
长度偏差——正确重读（如 1898 页 TEDS 0.998）远长于乱码 attempt，比率必然低，
被拒 3 张全部是好表；cov_attempt 在 11 样本上完美分离（好表全部 ≥0.906，唯一
内容错配 0.294）。**SLANet 臂 attribution 冻结为 cov_attempt ≥ 0.50 单门**；
编造风险由表格线门 + 结构门 + T5 stage-A 全量复核承接。校准代价如实记录：
yanbaor2_0aa9（cov_a=0.294，重读 TEDS 0.704）被保守拒绝，损失 1 张改善。

非阻断修正：非触发页 **442**（v1.1 误写 434）；T1 smoke 改为**门后期望**（15 键
adopted + 12 项拒绝逐项枚举）；identity 示例补 **paddlepaddle** 字段。
产物：`analysis-m42-t0-postgate-v1p2.json`（adopted/拒绝逐键）+ 
`analysis-m42-t0-attribution-recompute.json`（双公式原始计算，校准依据）。

**v1.0 → v1.1 修订摘要（对应 BLK 编号）**

| BLK | 修订 |
|---|---|
| BLK-1 | 触发域维持冻结的 16 页；全部有效性门分母改为 24 GT 键（raw 11 / empty 13 分别报告）；40 页 / 57 表降级为外层观察口径，不挂任何门（§1.3、D-65） |
| BLK-2 | T0 生产触发集裁决已完成并冻结（`analysis-m42-det-spike-production-freeze.json`）：24 键均值 0.544 vs 基线 0.000、17/24 改善、0 退化；GO/NO-GO 数值门预注册（D-67，含 post-hoc 披露）；regions 产物路径 + SHA-256 + layout 身份逐页冻结；M4.1"空页无信号"差异归因（§1.5） |
| BLK-3 | 失败策略表冻结：ON 启动类故障一律硬错误（启动先于任何页面产物）；运行中故障逐区域保原计数（D-63/D-64 合并重写为 D-63 失败策略表 + D-64 开关） |
| BLK-4 | 新旧 flag 组合行为表冻结（D-64）；identity 序列化契约 = `skip_serializing_if` OFF 字节不变 + ON 完整引擎身份 + resume 互斥（D-63）；`apply_reread_gate` 接线纳入 T2/T3 验收 |
| BLK-5 | attribution 决策撤回：raw 臂保留 M4.1 D-54 双公式门；新增**表格线预检门**（T0 校准 27/27，伪表 0 线被拒）；T5 stage-A 全部发射表（含无 GT 匹配者）100% 人工复核 + 预注册 NO-GO/收窄处置（D-62 重写） |

非阻断清理：conf 传递链补齐（`DetectedRegion` 加字段 + fixture，T3）；延迟口径统一为实测 186.3s/表（删除 v1.0"秒级"矛盾）；D-60 措辞明确"VLM F6 臂 = T1 移除工作树 WIP、运行时不存在"。

---

## 1. 背景与证据链

### 1.1 问题

OvisOCR3 基线（Overall 90.87）在零表格块页上把整表读成裸文本或不发射。
冻结触发域（16 页）内 24 张 GT 表基线 TEDS **全部为 0**（均值 0.000）。

### 1.2 M4.1 的结论与遗留

VLM 重读臂 spike FAIL（0/24 采纳，能力边界）。其触发/门/持久化基建
（发射形态前置资格、结构有效性门、page-NNNN.reread.json）保留，引擎换
SLANet-plus。**M4.1 的 VLM F6 臂是从未落地的分支工作树 WIP，M4.2 的 T1 将其
移除**——运行时不存在 VLM F6，"不再新增"与"运行时没有"两个表述同时成立。
代次契约沿用：M4.1 WIP 的 generation=2 不落地，M4.2 落地即 generation=3
（见 D-63）。

### 1.3 两个口径的 spike（各自冻结，不得混用）

**外层观察口径（oracle，40 页 / 57 GT 表，GT 真值框裁剪）**：均值 0.604、≥0.5 占
64.9%、有产出子集 0.765/82.2%。仅用于能力上界，**不挂任何门**（v1.0 错误已纠正）。

**生产触发域口径（T0-b，16 触发页 / 24 GT 键 / 27 det 区域，真实检测框）**
（下表为**门前**数字；含 attribution 的门后数字见文首 v1.2 修订表）：

| 口径 | n | 均值 | ≥0.5 | 改善(+0.05) | 退化 | 空产出 |
|---|---|---|---|---|---|---|
| 生产触发域 | 24 | **0.544** | 15/24 (62.5%) | **17/24 (70.8%)** | **0** | 7 键 |
| — raw 臂 | 11 | 0.435 | — | 7/11 | 0 | 4 |
| — empty 臂 | 13 | 0.636 | — | 10/13 | 0 | 3 |
| 基线 | 24 | 0.000 | 0 | — | — | — |

det 折扣归因：oracle 0.604 → det 0.511（40 页口径）；本触发域内 6 张 oracle 近满
分的表在 det 框下归零（教材页检测框切割表体）→ 结构门兜底保原，零损害。
区域→GT 配对：27 区域中 24 与 GT 键 IoU≥0.5 配对，3 个额外区域（§1.4 人工复核）。

### 1.4 T0 全量人工复核（BLK-5 要求，已完成）

27 区域裁剪图逐张对照预测 HTML（复核产物：`D:/t/m42-t0-crops/`，
manifest + 17 个非空候选 HTML）：

- **17 个非空候选 = 16 PASS / 1 FAIL**。16 个 GT 配对候选全部内容忠实于裁剪图
  （东财财报四表 ×2 页、诗文练习表、hex/ASCII 表、考古计数表、银行期限表、
  二元特征矩阵（90° 旋转）、心理学中介表（三线）、银行个股表、医美产业链表
  （90° 旋转），与逐表 TEDS 0.55–1.0 相互印证）。
- **唯一 FAIL = 无 GT 配对的额外区域**（jiaocaineedrop_1417 r0，conf 0.984）：
  裁剪图是"甲/乙两组散落分数列表"，无表格线、GT 不认为它是表；SLANet 把它
  量化成 4×36 的结构合法伪表。**conf 无法拦截（0.984 高于多数真表）**。
- **表格线预检门校准**：cv2 形态学线检测（横线 kernel = 45% 宽、纵线 = 45% 高，
  ≥2 条判定有线）在 27 张裁剪上 **27/27 判对**：伪表 0 线被拒；16 个有产出的真表
  全部有线；2 个无线的真表本就空产出（预检跳过零损失）；2 个有线的 EXTRA 空区域
  推理返回空、由空产出兜底。门放在 sidecar 侧（cv2 随 paddleocr 依赖已有），
  推理前快检，伪区域节省 186s/个。

### 1.5 M4.1"空页区域信号丢失"差异归因（BLK-2 要求）

m41 spike 归因 JSON 自记：4 个 empty 页 "sidecar regions.json empty **in this
run**"。实证：`D:/t/r3/m41-run1/` 目录 find 无任何 `*.regions.json`——T4 spike
用旧二进制扫描，当时 regions sidecar 写入链路损坏（后于 F6 持久化修复中修掉）。
T0 消费的 `D:/t/r3/regions/`（459 文件）来自修复后的 r3-ovisocr3 战役扫描
（layout pp-doclayout-v2@b7366822），16 触发页全部有区域信号。**是工具链 bug
差异，不是模型行为差异**；本次消费产物已逐文件 SHA-256 冻结（§3 T0 产物）。

### 1.6 架构约束（集成面调查结论）

产品推理 = 纯 Rust + llama.cpp MTMD（GGUF）；SLANet-plus 是 Paddle 三模型管线
（结构 + 内嵌 PP-OCRv4 单元格检测/识别），无法接入现有后端 → Python sidecar。
`FileRegionProvider` 已只产出 table 类区域（conf 字段需补传递链，见 T3）。

---

## 2. 目标 / 非目标

### 目标

G1：16 触发页 24 张 GT 表由 SLANet-plus 重读，触发域 table TEDS 均值 0.000 →
   ≥0.40（T5 实测判定），全集 Overall 不降。
G2：OFF 产物逐字节不变；**ON + 启动类故障 = 硬错误无产物**（不存在"ON 但退化
   产物"状态）；ON + 运行中故障 = 逐区域保原 + 计数，扫描正常完成。
G3：供应链冻结：模型 SHA + 三 Python 包版本 + sidecar 握手身份进 manifest/
   identity 链；本次消费的 regions 产物已逐文件冻结（T0 产物）。
G4：逐表延迟（实测 186.3s/表 CPU）与 sidecar RSS 实测入档。

### 非目标

- 公式 F6（M4.1 D-57 维持）；混合页维持排除口径。
- **触发域扩大**：维持冻结 16 页。f6_excluded（6 页）等其余 33 张坏表留在原地，
  列为 M4.3 候选——按评审解除条件，扩大触发域必须重新冻结目标清单 + 重做该人群
  oracle/det spike + 补持久化安全契约，本计划不做。
- SLANet ONNX 原生重实现（阶段 2 可选）；conf/多检调优（phase 1 只记录）。

---

## 3. 决策记录（D-59 起）

| 编号 | 决策 | 冻结依据 / 可执行性 |
|---|---|---|
| D-59 | 集成面 = Python sidecar（阶段 1）：**flag ON 即扫描开始时 spawn 长驻
worker 并握手（先于任何页面产物；模型在 worker 内惰性加载）**，stdin/stdout JSONL。
paddleocr==3.7.0 / paddlex==3.7.2 / paddlepaddle 钉扎。协议消息：hello
（身份）/ ping / recognize {png, bbox, page, region_ordinal} → {html} \| {html:
null, reject: no_ruling_lines} \| {html: "", reject: empty_output} \| 错误帧 | spike 以同栈验证；协议 fixture 冻结；ON 时启动即验证（BLK-3） |
| D-60 | 触发复用 M4.1 D-53 发射形态前置资格（block_N==0 + 形态检查；empty /
raw_single 且 det_N==1 触发；f6_excluded 6 页不触发不解码）；引擎 =
SLANet-plus sidecar；**VLM F6 臂不存在于运行时**（T1 移除工作树 WIP） | 触发语义与冻结清单同源；16 页/24 键口径全程统一（BLK-1） |
| D-61 | 裁剪源 = regions sidecar table 类 det 区域（本次消费产物 = §1.5 冻结的
r3 regions，SHA-256 逐页入档）；IoU≥0.9 去重（D-56）后 27 区域；conf 进
`DetectedRegion` 传递链并逐区域入 f6_detail；phase 1 不做 conf 阈值过滤
（T0 证明 conf 不能区分伪表：0.984） | 27/27 配对与复核产物齐全；provider 已存在仅补 conf 字段 |
| D-62 | **内容安全门（v1.2 校准定稿）**：全部区域 = 结构有效性门 + **表格线预检门**
（推理前执行：h≥2 或 v≥2，kernel 45%宽/高；T0 校准 27/27，伪表被拒）+ 空产出
兜底 + **逐区域判定**（多 GT 页不得用整页拼接判定）。raw 臂 attribution =
**cov_attempt ≥ 0.50 单门（M4.1 D-54/D-29 预授权的一轮校准）**：cov_reread ≥0.30
子句以实测证据移除——它有结构性长度偏差，拒绝好表（1898 页 TEDS 0.998、ijop
0.899、69c 0.550 均因重读远长于乱码 attempt 被拒）；cov_attempt 在 11 个 raw
样本上完美分离（好表全部 ≥0.906，唯一内容错配 0.294）。编造风险由表格线门 +
结构门 + T5 stage-A 全量复核承接。**每区域处置入 f6_detail 与
page-NNNN.reread.json**（adopted / rejected{structural,attribution,empty_output,
no_ruling_lines,engine_error,engine_down} / skipped_shape）。**T5 stage-A：ON
生产代码跑 16 页，全部发射表（含无 GT 匹配者）100% 人工复核；预注册处置：
发现任何内容错误 → 整体 NO-GO 或收窄（empty 臂移出 v1，杠杆降至 raw 6/11），
收窄决策上报用户**。**实现后不得再无代次地调整 attribution 公式或阈值——任何
此类调整必须 generation+1 并重跑 T5 全部约束性门（R2.5/D-42）** | T0 门后
15/24 键 adopted；复核 16 PASS/1 FAIL（FAIL 已被表格线门拦截）；校准依据
（11 样本双公式原始值）全量归档
`analysis-m42-t0-attribution-recompute.json`；预注册处置消除"选择性通过"空间 |
| D-63 | **失败策略表（冻结，唯一处置）**：<br>① OFF：不 spawn，产物逐字节不变<br>
② ON 启动类故障（python 缺失/依赖缺失/版本或 SHA 握手不匹配/启动超时）：
**硬错误退出，无页面产物、无 checkpoint 更新**（启动与握手先于页面循环）<br>
③ ON 运行中单请求故障（超时/崩溃/协议错误）：该区域 f6_rejected{engine_error}
保原，扫描继续，summary 汇总告警<br>④ ON 运行中 sidecar 进程死亡：后续区域全部
f6_rejected{engine_down}（phase 1 不自动重启），扫描正常完成，identity 记录计数<br>
**identity 序列化契约**：新增字段 `#[serde(skip_serializing_if = "Option::is_none")]`
（OFF = None = 序列化省略 = checkpoint/result 逐字节不变）；ON 携带完整引擎身份
{engine, revision, paddleocr, paddlex, paddlepaddle, models_sha, flag}；resume 门：
**`REREAD_RULES_GENERATION = 3`** + 引擎身份不匹配不复用产物。**代次语义（冻结，
同步更新常量注释）**：`2 = M4.1 双公式/VLM F6`（未落地 WIP，运行时不存在）、
`3 = M4.2 校准后 SLANet 规则`（cov_attempt 单门 + 表格线门）——依 R2.5/D-42
"校准一轮 = generation + 1"（第三轮 RBLK-2）；generation 2 产物在 generation 3
运行下不复用，反向亦然 | 四类故障唯一处置消除 D-63/D-64 冲突（BLK-3）；
`skip_serializing_if` 方案可用既有 OFF 产物 diff 测试机械验证（BLK-4）；
代次回归进 T2/T4（RBLK-2 解除） |
| D-64 | **flag 组合行为表（冻结）**：<br>\| 旧 --table-reread \| 新
--table-structure-reread \| 行为 \|<br>\|---\|---\|---\|<br>\| off \| off \|
现状基线，逐字节不变 \|<br>\| on \| off \| 现有 F1–F5 VLM 重读（已发射表），
F6 不触发 \|<br>\| off \| on \| 仅 F6 SLANet 臂；**校验同旧规则：必须携带
--layout-model/--layout-regions** \|<br>\| on \| on \| F1–F5 VLM + F6 SLANet
（人群不相交：已发射 vs 零块） \|<br>**代次注记**：旧 ON/新 OFF 组合的
"F6 不触发"本身是相对 generation 2 配置的行为变化，且该组合没有新引擎身份字段
可挡旧 F6 产物 resume——隔离只能靠 generation 3 门（T2/T4 验收覆盖）<br>
新 flag 经 `apply_reread_gate` 同一 arming 点接线（arm 条件 = 各自 flag）；
端到端接线验收进 T2/T3 | 每个组合的行为唯一可测（BLK-4）；layout 参数校验对称；
旧 ON/新 OFF 的代次隔离有明确机制与测试（RBLK-2） |
| D-65 | **回归门（预注册，口径 = 24 键触发域 + 全集两层）**：<br>**G-A** OFF
逐字节：458 页双扫描（新 flag 缺省 vs pre-M4.2 基线）全部产物（md/page_json/
checkpoint/result/reread sidecar）逐字节相等<br>**G-B** 触发域 ON：24 键中
≥12/24（50%）TEDS 较基线（全 0）提升 ≥0.05 且 24 键零退化；raw /11 与 empty
/13 分别报告（各 ≥40% 改善）；**非触发 442 页逐字节不变**<br>**G-C** 全集 ON：
458 页 Overall ≥ 90.87；table TEDS 均值严格提升；非触发页 text edit / formula
CDM 逐键相等；**16 触发页 text/CDM 不劣化**（v1.0"应逐键相等"措辞修正：
触发页 md 合法变化）<br>**G-D** 预算：186.3s/表已实测入档；sidecar RSS 实测入档
（无门槛值） | G-B 分母 = 触发域 24 键，数学可达（BLK-1 解除）；
T0 门后已示 15/24/0 退化（v1.2），门留 12/24 余量 |
| D-66 | CI 与可测性：Rust 单测走协议 fixture（mock 进程/内存 fake），不依赖
paddle（macOS CI 照常）；sidecar Python 测试 `@pytest.mark.paddle` 本机执行、
CI 显式报告 skipped 数；表格线门 fixture = T0 冻结裁剪图 2 张（伪表 + 有线真表）
+ 期望判定 | macOS CI 钉扎先例；skipped≠pass |
| D-67 | **T0 生产触发域 GO/NO-GO 门（预注册）**：均值 ≥0.40 ∧ 改善 ≥12/24 ∧
raw ≥5/11 ∧ empty ≥40% ∧ 退化 =0 ∧ 全量人工复核无内容错误（按 D-62 处置）→
GO 进 T1；任一不满足 → 停止集成。<br>**v1.2 门后复核（含 attribution）**：
0.513≥0.40 ✓、15≥12 ✓、raw 6/11 ✓、empty 9/13 ✓、退化 0 ✓、复核 16 PASS/1 FAIL
（FAIL 被表格线门拦截）✓ → **GO**。<br>**披露**：本门冻结时门前聚合数字已知
（0.544/17/0），attribution 与门后数字为重审要求补算（亦属后知）；故该门只作为
T0→T1 的停止条件复核；**约束性门是 T5 G-B/G-C**（实施后实测，不可选择性地跳过）。
若评审要求更强的 T0 独立性，替代方案 = 重跑 spike（成本 ~1.4h CPU）后按本门判定 | 数值门 + 停止条件成对出现（BLK-2 解除）；门后证据完整
（重审剩余阻断解除）；post-hoc 性质如实披露，不冒充预注册 |
| D-68 | **空臂排序确定化（v1.4，用户裁决 2026-11-23）**：empty 臂遇排序歧义
或纵向重叠 veto 时不再整页跳过，改用确定性坐标序——按 x 区间重叠 ≥0.3 切纵向
栏带，栏带左→右、栏带内上→下（多栏退化情形 = 单栏上→下，与原一致序等价）；
`f6_ordering_ambiguous=true` 与顺序决定入 stats/f6_detail。raw 臂原严格跳过
不变（纯防御，det_N==1 按定义单区域）。排序规则调整同样受代次契约约束 | T0 的
det spike 未经过排序门（文件序喂入）是 T0 设计盲区，v1.4 如实修正；被吞 8 表
T0 实测 0.74–1.00；空页追加无既有内容可打乱，D-56 跳过理由不成立 |

---

## 4. 任务分解（TDD）

| 任务 | 内容 | 验收 |
|---|---|---|
| T0 | ✅ 已完成（2026-11-22）：生产触发域裁决 + 全量人工复核 + 表格线门校准 +
regions 产物冻结 + m41 差异归因 + **attribution 补算与门后重算（v1.2）**。产物：
`analysis-m42-det-spike-production-freeze.json`（逐页 SHA-256 + 区域清单 + 24 键
逐表）、`analysis-m42-t0-attribution-recompute.json`（双公式原始值）、
`analysis-m42-t0-postgate-v1p2.json`（adopted 15 键 + 拒绝逐键）、
`D:/t/m42-t0-crops/`（27 裁剪 + manifest + 17 候选 HTML）、§1.4 复核记录 |
**D-67 门复核（门后口径）**：均值 0.513≥0.40 ✓、改善 15≥12 ✓、raw 6/11≥5 ✓、
empty 9/13≥40% ✓、退化 0 ✓、复核 16 PASS/1 FAIL（FAIL 已由表格线门拦截）✓ → GO |
| T1 | sidecar worker（`sidecar/table-engine/`）：JSONL 协议（hello/ping/
recognize/错误帧）、**表格线预检门**（cv2，D-62 阈值）、管线惰性加载、异常映射；
`requirements.txt` 钉扎三包 | 协议 fixture 单测（paddle 标记）；
表格线门 fixture = T0 冻结伪表 + 有线真表 2 张判定断言；端到端 smoke：
**门后期望** = 16 页 27 区域批量调用，15 键 adopted HTML 与 T0 预测逐字节一致，
12 项拒绝逐项枚举比对（empty_output 10（含 2 个 EXTRA 空区域）/
no_ruling_lines 1（EXTRA 伪表）/ attribution_cov_attempt 1） |
| T2 | Rust 客户端 + CLI：spawn/握手/超时/僵尸回收（D-63 四类处置表）；
`--table-structure-reread` 全链（ScanOptions → `apply_reread_gate` arming →
OcrIdentity → compatibility）；**identity 序列化契约**（D-63 skip 方案）；
`REREAD_RULES_GENERATION = 3` 常量 + 注释更新（2/3 语义，D-63）；
D-64 组合表各臂行为 | OFF checkpoint/result 字节相等测试（新增字段被省略的
diff 断言）；ON/OFF 不互 resume；引擎哈希不同不复用；**generation 2/3 双向
不复用（gen2 checkpoint 在 gen3 运行不复用、反向亦然；两类路径各 1 条：
旧 flag ON/新 flag OFF、SLANet ON）**；ON+启动故障 → 硬错误退出码断言；
组合表 4 臂各 1 条端到端单测 |
| T3 | 引擎接入 `orchestrate_f6`：区域裁剪（PNG）→ 表格线门（sidecar 侧）→
结构门 → attribution 门（raw 臂）→ raw_single 标记替换 / empty 追加 →
`f6_detail`（engine、conf、reject_reason 枚举）→ reread sidecar；
**conf 传递链**：sidecar 解析 → `DetectedRegion.conf` → f6_detail | 既有 F6 单测改造为 SLANet fake 全绿；新增：多区域排序（D-56；
v1.4 空臂确定化为 D-68）、
空产出保原、表格线门跳过（no_ruling_lines 计数）、sidecar 运行中死亡 →
后续区域 engine_down、零采纳产物逐字节不变守恒；conf 链 fixture 断言 |
| T4 | 段落级回归：16 页全集 ON/OFF 本地对比。**v1.4 补充**：D-68 实施后
4 个排序跳过页（5f75/6e1c/1417/3713）重跑 ON——5f75/6e1c 各采纳 4 张且 md
相对 OFF 变化；1417/3713 逐区域 empty_output 拒绝且 md 仍一致；dca64e05
engine_error 瞬态重跑确认 | 非触发页逐字节不变；触发页 md/page_json/reread
sidecar 三产物一致；checkpoint 哈希随新产物更新；ON 后 resume 不复用 OFF
产物（反之亦然）；**generation 回归：旧 flag ON/新 flag OFF（generation 2
语义）与 SLANet ON（generation 3）的 checkpoint 互相不复用，双向各 1 条
实测**；✅ 已完成（2026-11-23）：OFF 16/16 与 r3 基线逐字节一致 + 全集
1651/1651 md 一致 + 归因抽样 20/20；ON 首轮 6 采纳/1 engine_error/8 排序
丢失/1 拒绝，与可达性模拟逐页吻合 |
| T5 | 基准门（D-65 G-A→G-D 顺序）+ **stage-A 人工复核**：458 OFF 双扫描 →
16 页 ON → stage-A 全部发射表 100% 人工复核（含无 GT 匹配者）→ 458 ON | 四门判定表 + 判定 JSON；stage-A 预注册处置（D-62）：
任何内容错误 → NO-GO 或收窄上报；G-C 失败即整体 NO-GO |
| T6 | 档案：manifest/供应链冻结记录、RSS 实测、判定产物、v1.x 修订史；
大白话总结交付 | 文档 + 归档完整；评审残留清零 |

依赖：T0 ✅ → T1 ∥ T2（协议 fixture 先冻结）→ T3 → T4 → T5 → T6。

---

## 5. 风险

| 风险 | 缓解 |
|---|---|
| sidecar 依赖重（paddle 磁盘 GB 级） | opt-in flag 缺省不装不启（D-64 组合表）；
G-D 实测透明；阶段 2 ONNX 保留 |
| 逐表延迟 186.3s（全集 ON 触发域 27 区域 ≈ 1.4h） | 只在 16/458 页生效；单进程
批量（模型加载一次）；表格线门前置拦截伪区域 |
| paddleocr/paddlex 升级破坏行为 | 三包钉扎 + SHA manifest + 握手 fail-closed
（D-63 ②硬错误） |
| **无 GT 配对区域产出伪表**（T0 实锤 1 例） | 表格线预检门（27/27 校准，
伪表 0 线）；T5 stage-A 全量复核；预注册 NO-GO/收窄处置（D-62） |
| 表格线门误杀无边框真表（罕见） | 校准集内 0 误杀有产是真表；误杀 = 拒绝保原
（零损害，仅损失潜在改善）；f6_detail 记录 skipped_ruling 供后续校准 |
| raw_single 标记替换适配 SLANet html | 持久化形状引擎无关（M4.1 T2 已单测）；
T3 覆盖 html 含换行/属性场景 |
| 触发页 md 变化对 text/CDM 的外溢 | G-C 专门条款：16 触发页 text/CDM 不劣化
（逐键判定） |

---

## 6. 评审流程

v1.4 → 重审（唯一新增裁决点：D-68 空臂排序确定化——用户方向已确认 2026-11-23，
本文冻结其具体规则与验收）→ PASS 后实施 D-68 + T4 补充验收 → T5（G-B/G-C 全集
+ stage-A 人工复核）为最终 GO/NO-GO。
