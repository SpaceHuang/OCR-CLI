# OCR-CLI 复读退化检测与恢复修复方案

**文档状态：** 修订 v2.3——v2.2 复审 PASS 维持（2026-10-31），复审 MAJOR-1（D5.0 门槛区间缺口）与 3 项 MINOR 已并入。此前：v1 阻断已在 v2 修订、重审注意事项已在 v2.1 并入、2026-10-31 扩范围新增 §3.7 输出预算对齐（D5）。修订记录见 §8
**创建日期：** 2026-10-30（v1）/ 2026-10-30（v2、v2.1）/ 2026-10-31（v2.2 扩范围、v2.3 复审修订）
**依据：** `docs/requirement/requirements-v0.1.md` §7.3/§7.4/§7.5、`docs/develop/technical-design-v0.1.md` §7.1
**事故输入：** 2026-10-30《义务教育教科书·数学九年级下册》PDF 第 104 页（7.1 正切，例 3/例 4）扫描发生确定性复读退化，循环内容替换了页面后半部分真实内容；人工重扫确认在相同输入下 100% 复现，OCR 工具链自身无恢复路径。
**方案性质：** 本文独立给出问题定位、契约变更、检测器规格、恢复流程、修改清单与验收方法；实施时不需要借助其他设计文档才能理解本方案。

---

## 1. 事故证据与现状缺口

### 1.1 事故证据

输出目录：`D:\ChinaTextbook\初中\数学\苏科版-江苏凤凰科学技术出版社\Markdown\九年级\义务教育教科书·数学九年级下册\`

| 产物 | 事实 |
|---|---|
| `page-0104.json`（25,226 B，04:23:32） | `termination: "max_tokens"`，`warnings: ["truncated"]`；内容在“例 3 (2) 依次按键”处以行内片段 `依次按键 $\tan^{1}F$ 2 2 1 8` 为周期循环约 300 次直至 4096 token 预算耗尽；后半页真实内容（(2) 的解答、例 4、练习、习题 7.1）从未被生成 |
| `page-0104.checkpoint` | `complete=true`、`truncated=true`、`warnings=truncated`；无任何复读标记 |
| `result.json` | 整本 `status: partial`、`pages.truncated: 1`；页 104 诊断 `status: partial`、`warnings: ['truncated']`、`error_code: null` |
| `document.md`（后改名 `<PDF名>.md`） | 截断页内容被无标记合并进终稿；当前目录中 04:28:52 的 `page-0104.md` 为人工替换后的干净内容，`.json`/checkpoint 仍是坏产物——两套产物从此不一致 |

### 1.2 现有处理链路

```text
ocrx-runtime::LlamaMtmdRuntime::generate_with_progress        纯贪心 argmax 解码
  └─ ocrx-ovisocr2::write_page_artifacts_with_status_cancelled
       ├─ adapt_response → parse_page_with_termination
       │    ├─ remove_bbox_image_placeholders      先剥离 bbox 标签
       │    ├─ remove_confident_tail_repetition    仅“最后两行相同”时删一行
       │    └─ truncated = (finish_reason == "max_tokens")
       ├─ 裁剪 bbox PNG、写 page-NNNN.md / .json          ← .md 来自未裁剪的
       │                                                      response.raw_output
  └─ ocrx-output::ScanService::run_with_checkpoints_heartbeat
       ├─ checkpoint { complete: true, truncated } 照常提交，completed += 1
       └─ failed==0 && completed==全量 → finalize_document（无标记合并）
  └─ ocrx-cli::run_scan → status partial / exit 10（无 suggestion）
```

### 1.3 缺口清单

| # | 缺口 | 位置 |
|---|---|---|
| G1 | 生成循环无任何环上防护：temperature 0、无重复惩罚、无 n-gram 阻断、无生成中检测，进环后 100% 跑满 token 预算，且相同输入确定性复现 | `src/ocrx-runtime/src/llama_mtmd.rs:198-290` |
| G2 | 复读检测只覆盖“最后两行逐字节相同”并只删一行；行内片段循环（本事故形态）不触发，`repetition_removed` 警告缺失；bbox 标签循环在剥离后完全不可见。不满足需求 §7.4“必须处理或检测输出末尾周期性重复” | `src/ocrx-ovisocr2/src/lib.rs:372-388`、`lib.rs:252-253` |
| G3 | 截断/退化页按 `complete=true` 提交并静默合并进 `document.md`，真实内容被替换而终稿无任何标记；被清理的内容也没有任何保留路径，违反需求 §7.4“自动清理不得静默删除大量正常文本。发生不确定清理时应保留原始页面输出，并在最终报告中产生 warning”；只有翻查 result.json/checkpoint 才能发现异常。违反需求 §7.3“截断页不能作为完全成功处理”的产物层语义 | `src/ocrx-output/src/lib.rs:678-696`、`lib.rs:730-747`、`lib.rs:1281-1290` |
| G4 | 无自恢复路径：`is_reusable` 不看 `truncated`，`--resume` 对截断页走 `SkipCommitted` 永不重跑；全量重扫确定性复现（G1）；无逐页扰动重试机制 | `src/ocrx-output/src/lib.rs:1353-1382`、`src/ocrx-cli/src/main.rs:2171-2240` |
| G5 | exit 10 时终端无任何 suggestion（仅 exit 130 有），用户从 stdout 只能看到 `truncated: 1`，且现有唯一建议手段 `--resume` 对本场景无效（G4） | `src/ocrx-cli/src/main.rs:2939-2955` |
| G6 | 适配器对未知 `finish_reason` 默认映射为 `Termination::Eos`（完全成功语义），新增终止原因时会静默放大为成功 | `src/ocrx-ovisocr2/src/lib.rs:267-271` |
| G7 | 输出预算契约失效：默认 4096 仅为需求 §7.3"默认 16384"的 1/4；且适配器把 >4096 的 `--max-output-tokens` **静默钳回 4096**（`request()` 的 `min` 钳位），CLI 校验（≥128 与上下文容量）不拦截——用户提高预算完全无效，方案的"提高预算重扫"恢复路径实际不存在。另：上下文默认 16384 也装不下"1024 图像 + 16384 输出 + 1024 提示" | `src/ocrx-ovisocr2/src/lib.rs:10/196-202`、`src/ocrx-cli/src/main.rs:1022-1040`、`main.rs:1030/2120` |

---

## 2. 目标、非目标与行为基线

### 2.1 目标

1. 正常页面输出**逐字节不变**：检测器对现有 30 页参考金集（`ovisocr2-draft-v1`）零触发，全部 page.json 金样本不变。
2. 复读循环被尽早终止：进入环后不再跑满 token 预算，环前已生成内容完整保留。
3. 行级与行内两种周期形态、以及 bbox 标签循环，都被检测、**清理并标记**；清理结果必须同时驱动 `page-NNNN.md` 与 page.json（单源，见 §3.2.2），退化页终稿中不得残留循环正文。
4. 退化/截断页在终稿 `document.md` 中带显式可见标记，不再静默替换真实内容；被清理的原始输出按需求 §7.4 落盘保留。
5. `--resume` 能自动重试退化页（带渲染扰动，打破确定性），重试成功后重合并并摘除标记；全部重试失败则保留标记并以 partial 收尾（三连退化是预期内的常态结果，不是缺陷，见 §3.5）。
6. exit 10 时给出可操作的建议文本与受影响页码列表。
7. 输出预算符合需求 §7.3（v2.2 扩范围）：默认 16384、适配器上限同步 16384、超限**显式拒绝**而非静默钳位；配套上下文默认提升，使默认配置能容纳默认预算。

### 2.2 非目标

- 不改变温度 0 贪心解码与 OCR 提示词；不引入 repetition penalty / n-gram 阻断等会改变正常页输出的采样手段（行为基线 2.1.1 决定了只能做“检测后中止”，不能做“采样干预”）。
- 不实现“从既有前缀续写”的部分重生成（runtime 无该能力，llama.cpp MTMD 接口不支持安全回退）。
- 不新增退出码；partial 仍为 10。
- 不做区域裁剪二次 OCR、不做跨页上下文重扫。
- 不修改评分器与真值集。

### 2.3 行为基线

- 对金集 30 页与仓库内全部现有 fixture：page.json、document.md、result.json 中除本文明确新增的字段外逐字节不变；新增字段在这些页面上取“无退化”缺省值（机械比对规则见 §5.3）。
- `PageDocument::SCHEMA_VERSION` 保持 1；`result.json` 的 `schema_version` 保持 3；checkpoint 保持 v5。所有变更均为加法（见 §3.3 兼容性矩阵）。
- 退出码表（`ErrorKind`）不变。
- 裁剪操作必须幂等：对已裁剪文本再次裁剪**不新增任何删除、不新增任何警告**（第二次调用的检测标志自然为 false，§5.1 单测覆盖），保证“入口裁剪一次 + 独立路径二次进入”不会二次删内容。

---

## 3. 方案设计

### 3.1 分层总览

```text
D2 运行时层：token 环检测 → 早停，finish_reason = "repetition_loop"
D1 适配器层：统一退化裁剪管线（原始输出裁剪 → 同时驱动 .md 与 page.json）
             + 终止原因映射与 degenerated 不变式
D3 输出层：  原始输出保留（raw 边车）→ 退化页提交带标记 → document.md 可见标注
             → resume 扰动重试 → 重合并
D4 文档层：  需求/技术设计/Skill 文本对齐 + 金集验收
```

依赖关系：D1 的检测器先落地并冻结阈值（含金集零触发验收），D2 复用同一组阈值常量；D3 依赖 D1 的警告契约与单源管线；D4 随 D1-D3 收尾。

### 3.2 检测器规格

#### 3.2.1 运行时 token 环检测（D2，`llama_mtmd.rs`）

解码循环内维护最近 **K=256** 个 token 的环形窗口。每生成一个 token，在窗口内寻找最小周期 p ∈ [1, 64]，使得窗口末尾恰好由该周期完整重复 r 次构成（尾部允许被下一个不完整周期截断）。命中以下条件时判定进环并早停：

| 周期 p | 触发重复次数 r | 所需窗口长度上界 p·r |
|---|---|---|
| 1（单 token 循环） | ≥ 16 | 16 |
| 2–8 | ≥ 8 | 64 |
| 9–64 | ≥ 4 | 256 |

K=256 与最重档位 p=64、r=4 恰好相容（v1 的 K=128 使 p∈(32,64] 成为死档，v2 修正）。

- 触发后立即返回 `Ok(InferenceResponse { raw_output, finish_reason: "repetition_loop" })`，环前内容保留；**不是** `Err`。
- 每-token 开销为逐候选周期 p 比较窗口元素，最坏约 Σp ≈ 2,080 次比较（p ≤ 64），无堆分配，相对 llama 解码每-token 成本可忽略；金集耗时回归允许 ±3%。
- 阈值与 K 以上表为初始冻结值，实现为 `ocrx-runtime` 内常量；调整必须先通过 §5.3 金集零触发验收。
- 已知假阳性风险：连续标点（省略号展开为多个“。”token）、数学中的重复数字串。p=1 的 r≥16 即为此设；金集验收是兜底门禁，若金集出现触发，上调对应档位阈值而不是加白名单。
- 已知漏检边界：p>64 的长周期环不被 runtime 检测，由适配器层裁剪（§3.2.2，行内周期上限 512 字符）与 max_tokens/truncated 标记兜底。

#### 3.2.2 统一退化裁剪管线（D1，`ocrx-ovisocr2`）——修复 v1-BLK-1

**裁剪结果必须同时驱动全部下游产物。** v1 只替换 parse 路径导致 `.md`/`document.md` 仍含循环全文、且与 page.json 分叉；v2 将裁剪上移到产物入口，形成单源管线：

```text
response.raw_output
  → normalize_line_endings
  → degenerate_tail_crop（一次，且仅此一处裁剪）
       ├─ 裁剪后文本 ─→ replace_bbox_images → page-NNNN.md（及最终 document.md）
       ├─ 裁剪后文本 ─→ remove_bbox_image_placeholders → parse_structured_blocks → page.json
       ├─ 裁剪后文本 ─→ extract_bboxes → visual regions
       └─ crop 元数据（repetition_removed, degenerated）→ 警告与 checkpoint
```

实现落点：`write_page_artifacts_with_status_cancelled` 在调用 `adapt_response` 前执行一次 `degenerate_tail_crop(normalized_raw)`；`adapt_response` 与 `replace_bbox_images`、`extract_bboxes` 均以裁剪后文本为输入。`replace_bbox_images`/`extract_bboxes` 函数本身不改（§3.6 仅指函数体不动，输入改变）。公共入口 `parse_page`（测试/独立调用方）保留内部裁剪以维持独立语义；裁剪幂等性（§2.3）保证路径重叠时结果一致。

**警告的元数据来源**：`adapt_response` 以（裁剪后文本，入口 crop 元数据）为输入。入口裁剪之后，`parse_page_with_termination` 对已裁剪文本的内部再检测是 no-op（§2.3），自身算不出 `repetition_removed`——page.json 的 `repetition_removed`/`degenerated` 警告**以入口元数据并入为准**，不由 parse 内部重算。该接线由 §5.1 生产路径测试验证。

**检测形态**（对 normalize 后的原始输出、在剥离 bbox 标签之前执行）：

1. **行级周期**：设末尾连续行序列为 L1..Ln，存在 p ≥ 1 使 L(n-p+1..n) 与 L(n-2p+1..n-p) 逐行相同且该块整块重复 ≥ 2 次 → 判定环，将尾部裁剪到保留一个完整周期块，环前最后一个不完整周期块一并裁掉。**置信度守卫：参与匹配的每一行 trim 后 ≥ 8 字节**（与现行为 `last.len() < 8` 守卫一致；缺失该守卫会使现有测试 `parser_removes_only_high_confidence_tail_repetition` 的 `"short\nshort"` 用例翻转，v1-BLK-3）。含空行/短行的多行环因此不受行级规则覆盖，由 runtime token 检测（§3.2.1）或 `truncated` 标记兜底。
2. **行内周期**：取最后一行（或未以换行结尾的末段）字符序列，存在周期 p ∈ [8, 512] 字符使末尾由该周期完整重复 ≥ 3 次构成 → 判定环，裁剪到保留一个周期。p 下限 8 字符避免中文标点单字符循环误删（与 §3.2.1 的 token 档位独立，因为字符≠token）。

**警告判据（可操作定义）**：

- `degenerated = true`：命中任一形态（无论是否实际删减），**或** `termination == Termination::RepetitionLoop`（不变式，见 §3.2.3）。
- `repetition_removed = true`：当且仅当 `remove_bbox_image_placeholders(裁剪前) != remove_bbox_image_placeholders(裁剪后)`——即被裁掉的片段在剥离 bbox 标签后仍含非空白内容。环内容恰为 bbox 标签序列时（剥离后为空白），`repetition_removed = false` 而 `degenerated = true`。该判据使“只置位不删减”与“删减”不再依赖对环内容形状的猜测。
- 原“最后两行相同删一行”行为成为行级周期 p=1、r=2、行 ≥8 字节的特例，现有单测语义保持。
- 末端不完整周期（max_tokens 截断处）按不完整块处理，不参与周期匹配但随环一起裁剪。

#### 3.2.3 终止原因映射与 degenerated 不变式（D1，修复 v1-BLK-2 与 G6）

```rust
match finish_reason {
    "max_tokens" => Termination::MaxTokens,
    "stop" | "stop_token" => Termination::StopToken,
    "repetition_loop" => Termination::RepetitionLoop,   // 新增
    "eos" => Termination::Eos,
    _ => Termination::Invalid,                          // 原 `_ => Eos` 收紧
}
```

`Termination` 新增变体 `RepetitionLoop`（`as_str() = "repetition_loop"`）。`_ => Invalid` 使未知终止原因不再被放大为成功；现有测试使用的 `"eos"/"stop"/"stop_token"/"max_tokens"` 均不受影响（`"cancelled"` 在进入映射前已被 `lib.rs:90-93` 短路为 Err）。

**不变式（显式规则，承载于适配器共享路径，非约定）：**

> `termination == Termination::RepetitionLoop` ⇒ `degenerated = true`，无论文本检测是否命中。

动机：runtime 的 token 环早停（如 p=1、r≥16 的连续“。”）在文本上可能不呈现为行级/行内可检形态；若无此规则，页面将得到 `repetition_loop` 终止却零警告 → exit 0、无标记、resume 不重处理，复刻静默成功。该规则与 §3.2.2 的文本判据取“或”，在产物入口统一求值，纳入 D1.3 单测与 §5.2 场景 F。

### 3.3 契约变更与兼容性矩阵

| 契约 | 变更 | 兼容性 |
|---|---|---|
| `Termination`（core） | 新增 `RepetitionLoop`；`from_json` 同步接受 `"repetition_loop"` | page.json 由新二进制写入后，旧二进制读取会报 `InvalidJson`。page.json 是内部产物（用户契约是 document.md/result.json），接受此不对称；在本文记录为已知限制 |
| `PageWarning`（core） | 新增 `Degenerated`（`"degenerated"`）；`from_json` 同步接受 | 同上 |
| 警告置位规则 | `degenerated` = 文本检测命中 **或** `termination == RepetitionLoop`（§3.2.3 不变式）；`truncated` 仅表示 max_tokens，可与 `degenerated` 共存 | 解析层不约束组合 |
| checkpoint v5 | 新增加法字段：`degenerated=<bool>`（省缺读为 false）、`attempts=<n>`（最近一次处理运行的尝试次数，1..=3，Reprocess 时重置）、`raw=<文件名\|none>`（原始输出边车指针）。v5 解析器把未知行收入 map 且不读取，新字段对旧二进制安全 | 双向兼容 |
| checkpoint parameters（compatibility） | `tokens=` 由 `None` 改为记录**生效预算**（如 `tokens=16384`） | 新旧 checkpoint 必然失配 → 旧输出目录 `--resume` 全量 Reprocess，属预期迁移行为（D5.3、D-12） |
| 原始输出保留（需求 §7.4） | `repetition_removed || degenerated` 的页面原子写入边车 `page-NNNN.raw`（pre-crop 的 normalize 后原始输出）。落点为边车文件而非 checkpoint 字段本体：raw 可达单页输出预算上限（v2.2 后为 16384 token，约 40–64 KB），hex 编码入 checkpoint 会使每个坏页 checkpoint 膨胀一倍以上且可读性崩坏；D4.1 将 §7.4“保留原始页面输出到 checkpoint”修订为“保留原始页面输出（checkpoint 记录指针）”。边车不参与 markdown 收集、不受 `--keep-pages` 影响、随输出目录持久保留 | 新文件类型；`collect_page_markdowns`/`recover_pending_ocr_queue` 均按精确文件名或 `.checkpoint` 后缀匹配，无冲突 |
| `JobSummary`（output） | 新增 `degenerated_pages: u32`；CLI 四处 `failed_pages == 0 && truncated_pages == 0` 判定改为同时要求 `degenerated_pages == 0`（`main.rs:2716/2802/2858/2950`） | 内部结构体 |
| result.json（schema 3） | `pages` 对象加法新增 `"degenerated": N`；页诊断沿用现有 `warnings` 数组承载 `degenerated` | 加法字段，Python/JSON 消费者兼容；不做 schema bump |
| `document.md` | 退化/截断页在页内容前插入一行可见标记（§3.4），页正文为**裁剪后**文本（§3.2.2 单源） | 新行为，金集页面全部无标记故输出不变 |
| `InferenceResponse.finish_reason` | 新增值 `"repetition_loop"`；runtime fake 同步提供该取值 | 内部 trait，跟随本仓库一起改 |

### 3.4 落盘与终稿标记（D3，修复 G3）

1. `commit_page` 时 checkpoint 写入 `degenerated`、`attempts`、`raw` 指针；`PageCheckpoint` 结构体与 `write_checkpoint`/`read_checkpoint` 同步加字段。
2. `diagnostic()`（`lib.rs:1384-1391`）：`degenerated || truncated` → status `"partial"`（现状只看 truncated，扩展）。
3. **终稿标记**：`merge_markdown` 对 `truncated || degenerated` 的页，在该页（裁剪后）内容前插入固定格式标记行并空一行：

   ```text
   > ⚠️ ocrx：第 104 页识别不完整（输出被复读退化截断），本页内容可能缺失或错误，请重扫后核对本页。
   ```

   措辞按 `truncated`（预算截断）与 `degenerated`（复读退化）二选一；标记行逐字符冻结进测试。合并输入的页数与标记由 `collect_verified_page_markdowns` 一并携带（`PageMarkdown` 增加标记来源字段，不从 .md 文本反解）。
4. resume 重跑成功后重合并时标记自动消失（该页不再是 truncated/degenerated）。

### 3.5 恢复路径（D3，修复 G4）

**resume 重处理条件（v2 收紧）**：仅 `checkpoint.degenerated == true` → `CheckpointResumeAction::Reprocess`。纯 `truncated`（max_tokens 且未检出环）页在同参数 `--resume` 下维持 `SkipCommitted`：确定性下重跑必然同样截断，且提高 `--max-output-tokens` 本身会改变 compatibility parameters 使页面自然进入 Reprocess，不依赖该条件（与决策 D-5 一致，v1 中 `truncated == true → Reprocess` 的表述与 D-5 自相矛盾，已修正）。终稿标记对纯截断页持续保留，直到用户带新参数重扫。

**改动点唯一**：`PageCheckpoint::disposition`（`lib.rs:1336-1370`）。resume 队列无需“并入退化页”——`initial_ocr_queue_for_mode`（`main.rs:1989`）本就包含全部待 OCR 页，resume 仅靠 `resume_skip_pages` 剔除 SkipCommitted（`main.rs:2228`），disposition 改判后退化页自然留在队列中。

**扰动重试策略**（打破确定性，仅对退化页生效）：

- 单次处理运行内对同一页最多尝试 3 次：attempt 0 = 原始渲染；attempt 1/2 = 模型输入等比缩放 0.94 / 1.06（保持像素预算约束与宽高比，RenderAudit 如实记录 scaled 与比例）。runtime 侧 image token 网格为固定值（`llama_mtmd.rs:43-78`，`image_min_tokens == image_max_tokens`），扰动通过重采样改变像素输入与 logits，机制成立。
- attempt n 退化（runtime 早停 / 适配器检测 / max_tokens 且检出环）→ 换下一 attempt；非退化 → 提交该 attempt 并停止。
- 全部 attempt 退化 → 提交最后一个 attempt 的产物，保留全部警告，页面按 partial 收尾，终稿带标记，raw 边车保留。
- **预期命中率声明**：复读环是强吸引子，缩放扰动 escape 与否取决于环对输入扰动的敏感度，先验未知；**三连退化（§5.2-B）应被视为常态结果而非缺陷**，其实际频率在 D4.3 验收时以真实书籍统计补充记录。单次退化 attempt 在 runtime 早停下成本很低（环确认即停，不再跑满预算），三连尝试的代价可接受。
- `attempts` 字段记录最近一次处理运行的尝试次数（1..=3）；每次 Reprocess 重置重新计数。对已耗尽 3 次的坏页再次 `--resume` 会开启全新一轮 3 次尝试（每次 resume 均为用户显式发起，允许重试；runtime 早停使重复代价有界）。

**退出与建议（修复 G5）**：exit 10 时文本报告追加一行，列出受影响页码并给出命令：

```text
warning: 1 page(s) incomplete — page 104 (degenerated); rerun with --resume to retry degenerated pages
```

`--resume` 提示仅在存在退化页时给出。纯截断页的建议按预算状态分支（v2.2）：默认预算（=上限 16384，§3.7）下截断 → 提示"该页超出单页输出预算上限，页面保持不完整标记"；用户此前用 `--max-output-tokens` 降低过预算 → 提示恢复默认重扫（parameters 变化自动触发 Reprocess）。

### 3.6 不修改项

- OCR 提示词、温度、GPU/CPU 回退、native 路由、bbox 裁剪与 `validate_markdown_image_links` 全部不动；输出预算与上下文默认按 §3.7（D5）变更，不在冻结之列。
- `remove_bbox_image_placeholders`、`replace_bbox_images`、`extract_bboxes` 的**函数体**与去重/安全逻辑不动（管线中它们的输入改为裁剪后文本，见 §3.2.2）。

### 3.7 输出预算对齐（D5，v2.2 扩范围）

G7 的两半——默认值偏离（需求的 1/4）与上限静默钳位——使需求 §7.3 的预算契约形同虚设：不仅默认配置下长页更早截断，连用户提高预算的自救路径都不存在。经 2026-10-31 用户决策（D-12），本节将预算对齐纳入方案范围：

1. **适配器上限与默认**：`MAX_OUTPUT_TOKENS` 4096 → 16384；`request()` 删除 `min` 静默钳位，生产路径 `write_page_artifacts_with_status_cancelled` 对超限请求**显式报错**（`try_request` 的拒绝语义扩展到生产路径）；CLI 默认 `--max-output-tokens` 4096 → 16384。
2. **上下文默认**：16384 → 20480（1024 图像 + 16384 输出 + 1024 提示 = 18432，留 ~2K headroom），使默认配置通过既有容量校验（`main.rs:1027-1040`）；该校验保持为唯一容量守门，报错信息指导同步调整，不引入新的隐式推导。
3. **前置硬门槛 D5.0**：读取冻结模型 GGUF 元数据的 `n_ctx_train`，按公式确定默认组合（v2.3 修正 v2.2 的区间缺口——硬门槛必须守护它自己的默认值 20480，而非仅 18432 下限，否则实测值落在 [18432, 20480) 时会带出"上下文超出训练窗口"的组合，恰是预算上调后新暴露的形态）：
   - `context_default = min(20480, n_ctx_train)`
   - `output_default = min(16384, n_ctx_train − 2048)`（2048 = 图像 1024 + 提示 1024，与既有容量校验公式一致）

   `n_ctx_train ≥ 20480` 时即 §3.7.1/§3.7.2 的完整默认组合；实测值更小时按公式回退，D4.1 按实测值修订 §7.3 并在 D-12 记录。D5.0 必须先于 D5.1–D5.4 执行。
4. **兼容键精确化**：checkpoint parameters 的 `tokens=` 记录生效预算（替代 `tokens=None`）；新旧 checkpoint 必然失配，旧版本输出目录 `--resume` 全量 Reprocess 属预期迁移行为，写入 D4.2 的文档/Skill 说明。
5. **顺序依赖**：D5 必须在 D2（runtime 环早停）之后实施——预算 ×4 后，无早停时退化页的解码时间与产物体积同步 ×4；早停使退化页成本有界，是预算上调的安全前提。金集终验（D4.3）在 D5 之后按最终默认值执行。
6. **金集影响论证**：2026-10-30 门禁报告 30 页全部 0 截断（均在 4096 内正常终止），预算上调对金集为可证明的 no-op，§5.3 逐字节门禁不受影响。

---

## 4. 任务分解

| 任务 | 内容 | 主要文件 | 验收 |
|---|---|---|---|
| D1.1 | 统一退化裁剪管线（§3.2.2）：`degenerate_tail_crop` 实现、`write_page_artifacts_with_status_cancelled` 单次裁剪并三路分发（.md / parse / bbox）、警告元数据并入、删除 `remove_confident_tail_repetition`、`parse_page` 独立路径保留裁剪、幂等性 | `src/ocrx-ovisocr2/src/lib.rs` | §5.1 单测全绿，含 .md/.json 单源一致性与生产路径警告接线测试 |
| D1.2 | core 契约：`Termination::RepetitionLoop`、`PageWarning::Degenerated`、from_json/to_json/golden 更新 | `src/ocrx-core/src/lib.rs` | golden 往返测试含新值 |
| D1.3 | finish_reason 映射收紧 `_ => Invalid` + `"repetition_loop"`；**RepetitionLoop ⇒ degenerated 不变式**在共享路径求值 | `src/ocrx-ovisocr2/src/lib.rs` | 单测覆盖未知值与不变式（含文本零命中的 token 环用例） |
| D1.4 | 事故回归 fixture：按 `page-0104.json` 中的环片段构造原始输出 fixture（行内周期 + 尾部不完整周期），存 `src/ocrx-ovisocr2/tests/fixtures/degeneration/`；bbox 标签环 fixture（`repetition_removed=false` 判据） | 新增 fixture | 解析断言：裁剪后文本无循环正文、`degenerated` 置位、.md 与 page.json 同源一致 |
| D2.1 | 运行时 token 环检测（§3.2.1，K=256），环形窗口、无堆分配、取消/心跳语义不变 | `src/ocrx-runtime/src/llama_mtmd.rs` | fake 单测：p=1/2/9 档位触发、p=64+r=4 边界触发、p>64 不触发、环前内容完整、长非重复合成 token 流（≥K 窗口无周期尾）不触发；金集不触发早停由 §5.3 端到端门禁覆盖 |
| D2.2 | runtime fake 提供 `"repetition_loop"` 行为；`GenerationProgress` 在早停路径正常收尾 | `src/ocrx-runtime/src/lib.rs` | fake 驱动的 adapter/scan 集成测试 |
| D3.1 | checkpoint 加 `degenerated`/`attempts`/`raw` 字段（双向兼容），`PageCheckpoint`/读写/`diagnostic()` 扩展 | `src/ocrx-output/src/lib.rs` | v5 旧文件可读、新文件旧解析器可读 |
| D3.2 | merge 标记：`PageMarkdown` 携带标记来源，`finalize_document`/`merge_markdown` 插入标记；resume 重合并摘除 | `src/ocrx-output/src/lib.rs` | 合并单测 + 金集零标记 |
| D3.3 | resume 重处理：`disposition` 判定（仅 `degenerated` → Reprocess，队列经 `resume_skip_pages` 自然放行）、扰动重试状态机（≤3 attempts、Reprocess 重置计数）、`JobSummary.degenerated_pages` | `src/ocrx-output/src/lib.rs`、`src/ocrx-cli/src/main.rs` | §5.2 fake E2E |
| D3.4 | exit 10 建议文本、四处 success 判定收口、`report.result` 文本输出 | `src/ocrx-cli/src/main.rs` | 报告快照测试 |
| D3.5 | `page-NNNN.raw` 边车：`repetition_removed || degenerated` 时原子写入 pre-crop 原始输出，checkpoint 记录指针；不参与收集/清理 | `src/ocrx-ovisocr2/src/lib.rs`、`src/ocrx-output/src/lib.rs` | 边车存在性与内容测试；正常页无边车 |
| D5.0 | 前置硬门槛：读取冻结模型 GGUF 的 `n_ctx_train`，按 §3.7.3 公式确定 `context_default`/`output_default`（含回退值） | GGUF 元数据、`src/ocrx-models` | 实测值与公式结果写入 D-12 与 D4.1 依据 |
| D5.1 | 适配器 `MAX_OUTPUT_TOKENS` 16384、删除 `request()` 静默钳位、生产路径超限显式报错；CLI 默认 16384 且校验**镜像上限**（超限在扫描启动前一次性报错，而非逐页失败） | `src/ocrx-ovisocr2/src/lib.rs`、`src/ocrx-cli/src/main.rs` | 钳位删除 + CLI 上限镜像的单测 |
| D5.2 | 上下文默认 20480（按 D5.0 公式结果，`n_ctx_train` 不足时取回退值）、help 文本、容量校验联动（默认组合必须通过） | `src/ocrx-cli/src/main.rs` | 校验单测、help 快照更新 |
| D5.3 | checkpoint parameters `tokens=` 记录生效预算；新旧失配迁移说明 | `src/ocrx-output/src/lib.rs`、`src/ocrx-cli/src/main.rs` | 兼容键测试、迁移说明入文档 |
| D5.4 | 新默认下资源复测（CPU RSS / GPU VRAM / 页均耗时 / 截断率），更新基线记录 | `docs/develop/`、`scripts/` | 复测记录通过资源门禁 |
| D4.1 | 需求对齐：§7.4 扩写（行级+行内+标签级检测、runtime 早停、resume 重试、终稿标记；“保留原始页面输出到 checkpoint”修订为边车+指针）；§7.3 预算条款按 D5.0 实测结果校对（16384 可达时保留原文并补充上下文约束；不可达时按回退值修订） | `docs/requirement/requirements-v0.1.md` | 文评 |
| D4.2 | 技术设计 §7.1 生命周期图更新；`src/ocrx-skill` SKILL 文本补 exit 10 处置 | `docs/develop/technical-design-v0.1.md`、`src/ocrx-skill` | 文评 |
| D4.3 | 金集验收：30 页参考集全量重扫 + 评分，证明行为基线 §2.3；附真实书籍退化统计记录 | `scripts/`、`benchmarks/` | §5.3 门禁 |

执行顺序：D1.1–D1.4 → D2.1–D2.2 → D3.1–D3.5 → D5.0–D5.4 → D4.*。约束：D5.0 必须先于 D5.1–D5.4；D5 必须在 D2 之后（§3.7.5）；D4.3 金集终验在最终默认值下执行。每个任务合并点通过既有门禁：`fmt --all -- --check`、`test --workspace`、clippy 零警告（Windows 实机）。

---

## 5. 测试与验收

### 5.1 单元测试（fake，无模型）

- 环检测器：行级 p=1（现行为回归，含 `"short\nshort"` 5 字节不触发）、p≥2 多行周期、**行级 8 字节守卫边界（8 字节触发 / 7 字节不触发）**；行内周期含中文/公式/`<img bbox_…>` 标签环；尾部不完整周期；短重复（如表格中相邻相同单元格、`(1)…(2)…` 结构、省略号）必须不触发。
- **裁剪幂等性**：对裁剪输出再次裁剪，结果文本与首次裁剪逐字节一致，且不产生 `repetition_removed`/`degenerated` 标志（口径同 §2.3：不新增删除、不新增警告）。
- **`repetition_removed` 判据**：bbox 标签环 → `removed=false, degenerated=true`；文本环 → 两者皆 true。**判据与警告接线测试必须经 `write_page_artifacts_with_status_cancelled` 生产路径驱动**——入口裁剪后 parse 内部再检测为 no-op，仅直调 `parse_page` 的测试无法发现元数据漏接线。
- **不变式**：`finish_reason="repetition_loop"` 且文本检测零命中（如 16 个连续“。”单行）→ `degenerated=true`。
- 终止映射：`"repetition_loop"` → `RepetitionLoop`；未知串 → `Invalid`；旧取值不变。
- checkpoint 往返：含/不含 `degenerated`、`attempts`、`raw`；旧 v5 文本逐字节兼容读取。
- merge 标记：truncated 页、degenerated 页、二者的措辞差异、正常页无标记。
- **单源一致性**：退化页的 `page-NNNN.md` 内容 == `replace_bbox_images(裁剪后文本)`，且 page.json blocks 拼接**按既有 tag 替换规则**（bbox 占位标签 ↔ figure 图片链接）与 .md 对齐——blocks 剥离 bbox 标签并以 VisualRegion 承载，与 .md 不逐字节相等（杜绝 v1 式分叉）。
- **预算契约**：适配器对 >16384 显式报错（钳位行为删除）；**CLI 校验镜像上限**——`--max-output-tokens 20000` 在扫描启动前一次性报错，不得以逐页适配器失败的形式呈现；默认组合（图像 1024 + 输出 16384 + 上下文 20480）通过容量校验；checkpoint `tokens=` 记录生效值。

### 5.2 集成测试（fake runtime 注入）

- 场景 A：attempt 0 环、attempt 1（0.94x）正常 → 提交 attempt 1，`attempts=2`，无标记，status success，无边车泄漏。
- 场景 B：三次 attempt 全环 → 提交 attempt 2，`degenerated_pages=1`，exit 10，终稿 = 标记行 + **裁剪后**文本（无循环正文），raw 边车存在，建议文本含页码与 `--resume`。
- 场景 C：纯 max_tokens（无环）→ 不重试，`attempts=1`，终稿带 truncated 标记；**同参数 `--resume` 对该页 SkipCommitted**；建议文本按预算状态分支（默认=上限时说明已达上限，降低过预算时提示恢复默认重扫）。
- 场景 D：`--resume` 对 A 场景已修复页 SkipCommitted、对退化页 Reprocess（attempts 重置为 1 重新计数）；重合并后标记摘除。**对 `attempts=3` 的坏页再次 resume → 开启新的一轮 3 次尝试**。
- 场景 E：Ctrl+C 发生在 attempt 1 → 页面按 cancelled 处理，checkpoint 可续。
- 场景 F：runtime p=1 token 环早停（文本检测零命中）→ `termination=repetition_loop`、`degenerated=true`、exit 10、终稿带标记、resume Reprocess。

### 5.3 金集门禁（真实模型，发布前必过）

机械比对规则（可执行，无豁免条款）：金集 30 页在改动前后

1. 每页 `page-NNNN.json` **逐字节一致**（金集页警告均为空，不存在“仅 warnings 差异”的页面，故不允许任何差异）；
2. `document.md` **逐字节一致**；
3. `result.json` 除新增 `pages.degenerated` 字段（值为 0）与易变字段 `elapsed_ms`（及任何计时派生字段，逐次运行必然不同）外逐字段一致；
4. 评分门禁沿用 2026-10-30 执行报告口径（文本 NED、0 截断/重复/空白失败页），不允许退化；评分脚本输入（页面 md/json）不变。

附带验证：以本事故的九年级下册第 104 页做真实复扫，确认 runtime 早停生效（生成 token 数远小于 4096）、扰动重试命中或按 §5.2-B 收敛为带标记 partial + raw 边车。

门禁执行时点：D4.3 在 D5 预算对齐后的**最终默认值**下运行；预算上调对金集为可证明 no-op（30 页全部 0 截断、均在 4096 内正常终止，见 §3.7.6），故逐字节门禁与耗时 ±3% 门禁在最终默认值下依然有效。

---

## 6. 决策记录

| # | 决策 | 理由与放弃的备选 |
|---|---|---|
| D-1 | 用“检测后早停”而非 repetition penalty / n-gram 阻断 | 采样干预会改变所有正常页输出，违反 §2.3 行为基线；检测只在命中环时介入 |
| D-2 | 新增 `Termination::RepetitionLoop` 而非复用 `MaxTokens` | 保留事实语义供诊断与重试策略区分，并作为 `degenerated` 不变式的触发条件；page.json 旧读不兼容已记录并接受（内部产物） |
| D-3 | 环检测在 bbox 剥离之前对原始输出执行 | bbox 标签环剥离后不可见（本事故差点如此）；先检测后剥离，剥离逻辑本身不动 |
| D-4 | ~~默认输出预算维持 4096，修订需求 §7.3 文本~~ **v2.2 废止，由 D-12 取代** | v2.1 立场（仅文档对齐）存在内在矛盾：§3.5 的"提高预算重扫"恢复路径被适配器静默钳位架空——该路径实际不存在；2026-10-31 用户决策扩范围 |
| D-5 | 纯 max_tokens 页不做扰动重试，同参数 resume 维持 SkipCommitted | 缩放对预算不足无效且确定性下必然复现。注意（v2.2）：默认预算=上限 16384 后，"提高预算触发 Reprocess"仅在用户此前降低过预算或从旧版本（tokens=None）迁移时成立；预算已在上限时该页保持带标记不完整 |
| D-6 | 扰动采用渲染缩放（0.94/1.06）而非温度/种子扰动 | 温度 0 是冻结契约；缩放改变像素输入即改变 logits，实现成本最低且被 RenderAudit 完整记录。image token 网格固定不阻碍该机制 |
| D-7 | result.json 做 schema 3 加法而非 bump 4 | 字段纯加法、消费方（评分脚本）宽松解析；bump 会强制所有消费方同步，无收益 |
| D-8 | 裁剪上移到产物入口，单次裁剪同时驱动 .md 与 page.json（v2-BLK-1） | 备选“终稿保留循环全文仅加标记”违背目标 2.1.3/2.1.4，且加剧 .md/.json 分叉；单源管线以幂等裁剪为代价换取产物一致，正常页裁剪为 no-op 故基线不变 |
| D-9 | 原始输出保留为 `page-NNNN.raw` 边车 + checkpoint 指针（v2-MAJOR-2，落实需求 §7.4） | raw 体积可达单页输出预算上限（v2.2 后 16384 token，约 40–64 KB），hex 编码入 checkpoint 本体会使坏页 checkpoint 膨胀一倍以上；边车文件加指针同时满足保留与 checkpoint 自描述，D4.1 同步修订 §7.4 措辞 |
| D-10 | `RepetitionLoop ⇒ degenerated` 为适配器强制不变式（v2-BLK-2） | token 环在文本上可能不可检；无此规则将复刻静默成功。与文本检测取“或”，机制承载于共享路径而非口头约定 |
| D-11 | `attempts` 记录最近一次处理运行的尝试次数，Reprocess 重置（v2-MINOR-2） | 累计语义需要持久化历史且无法界定“预算耗尽后是否永不再试”；每次 resume 均用户显式发起、runtime 早停使单次退化代价有界，重置语义最简单且可预期 |
| D-12 | 输出预算对齐（v2.2 扩范围）：默认与适配器上限均为 16384（实现需求 §7.3 原文），超限显式拒绝，上下文默认 20480，checkpoint `tokens=` 记录生效值 | 消除"默认 1/4"与"静默钳位"两个契约破洞，使"提高预算"恢复路径真实存在；安全前提为 D2 环早停（退化页成本有界）与 D5.4 资源门禁；n_ctx_train 不足时按 D5.0 回退为最大可行预算并修订 §7.3 |

---

## 7. 风险

| 风险 | 缓解 |
|---|---|
| 检测器假阳性误删正常内容 | 阈值档位冻结 + 金集零触发门禁 + §5.1 反例集 + 行级 8 字节守卫；p=1 档阈值最保守；raw 边车保证误裁可人工恢复 |
| 扰动重试仍进环（三连退化） | 属预期常态而非缺陷（§3.5 命中率声明）：按 §5.2-B 收敛为带标记 partial + raw 边车，用户可改 DPI/预算后全量重扫 |
| runtime 每-token 检测拖慢解码 | 最坏约 Σp≈2,080 次窗口比较，无堆分配；金集耗时回归比对（允许 ±3%） |
| 终稿标记被下游评分脚本误当正文 | 标记为独立 blockquote 行；评分前处理按行剥离（D4.3 顺带核对 scorer） |
| 长周期环（p>64 token 或含空行/短行的多行环）逃过全部文本检测 | 由 runtime max_tokens/`truncated` 标记与终稿标记兜底为“不完整”而非静默成功；§3.2.1 已声明漏检边界 |
| 预算上调的耗时/资源代价：未截断密集页解码上限 ×4，KV cache 随上下文 16384→20480 增长约 25% | D2 早停使退化页成本有界；D5.4 在登记基准设备复测 RSS/VRAM/页均耗时并过资源门禁；确需控制耗时的场景由用户显式调低 `--max-output-tokens`（parameters 变化自动 Reprocess） |
| 冻结模型 `n_ctx_train` 不足以容纳完整默认组合（< 20480） | D5.0 为前置硬门槛，最先执行；按 §3.7.3 公式回退 context/output 默认值（杜绝 [18432, 20480) 区间的超窗组合），D4.1 修订 §7.3 并在 D-12 记录实测值——方案不因该结果返工 |

---

## 8. 修订记录

**v2（2026-10-30）**——响应评审 `docs/review/repetition-degeneration-detection-recovery-development-plan.md`（结论 BLOCK）：

- **BLK-1**：§3.2.2 重写为“统一退化裁剪管线”，裁剪上移到 `write_page_artifacts_with_status_cancelled` 产物入口，单次裁剪同时驱动 `page-NNNN.md`/`document.md`、page.json 与 bbox 提取；新增决策 D-8、单源一致性测试（§5.1）与场景 B 断言（终稿无循环正文）。
- **BLK-2**：§3.2.3 新增显式不变式 `termination == RepetitionLoop ⇒ degenerated = true`，承载于适配器共享路径；新增决策 D-10、D1.3 单测与集成场景 F。
- **BLK-3**：§3.2.2 行级周期规则补齐 ≥8 字节守卫（逐行、trim 后），现有测试 `"short\nshort"` 语义保持；新增守卫边界单测。
- **BLK-4**：§3.2.1 窗口 K=128 → 256，阈值表增加 p·r 上界列证明自洽；每-token 开销措辞修正为 Σp≈2,080 次比较；新增 p=64+r=4 边界与 p>64 漏检边界声明。
- **MAJOR-1**：§3.5 resume 条件收紧为仅 `degenerated` → Reprocess，纯截断页同参数 SkipCommitted，与 D-5 对齐；改动点澄清为 `disposition`（v1-MINOR-1 一并修正，删除“并入 initial_queue”的错误表述）。
- **MAJOR-2**：新增 `page-NNNN.raw` 边车 + checkpoint `raw` 指针（决策 D-9、任务 D3.5），落实需求 §7.4 原始输出保留；G3 补充对应条款，D4.1 修订 §7.4 措辞。
- **MAJOR-3**：§3.5 新增扰动命中率声明：三连退化为预期常态而非缺陷，D4.3 记录真实频率。
- **MINOR-1/2/3/4**：resume 队列改动点修正；`attempts` 语义定义为“最近一次运行、Reprocess 重置”并补场景 D；§5.3 改写为可机械执行的四条比对规则（取消豁免措辞）；`repetition_removed` 给出 strip-compare 可操作判据。

**v2.1（2026-10-30）**——响应重审（结论 UNBLOCK/PASS）的 2 条非阻断注意事项：

- **NOTE-1（警告元数据接线）**：§3.2.2 新增“警告的元数据来源”段——`adapt_response` 以（裁剪后文本，入口 crop 元数据）为输入，page.json 警告由入口元数据并入，parse 内部对已裁剪文本的再检测为 no-op；§5.1 `repetition_removed` 判据测试改为必须经 `write_page_artifacts_with_status_cancelled` 生产路径驱动（仅直调 `parse_page` 漏检错误接线）；§2.3 幂等表述精确为“不新增删除、不新增警告”；D1.1 任务与验收同步。
- **NOTE-2（单源比对措辞）**：§5.1 单源一致性由“blocks 拼接与 .md 一致（逐字节）”修正为“按既有 tag 替换规则（bbox 占位标签 ↔ figure 图片链接）对齐”——blocks 剥离 bbox 标签并由 VisualRegion 承载，逐字节相等按字面不可满足。

**v2.2（2026-10-31）**——用户决策扩范围，G7 由"文档对齐"改为"实现对齐"（§3.7 输出预算对齐，D5 任务组）：

- **G7 重新定性**：缺口从"需求文本矛盾"升级为"预算契约失效"——补充发现适配器把 >4096 的 `--max-output-tokens` 静默钳回 4096（`request()` 的 `min` 钳位），CLI 校验不拦截，用户提高预算完全无效，方案的"提高预算重扫"恢复路径实际不存在。该发现源于实施前对 G7 相关代码的复核。
- **§3.7 新增**：适配器上限与默认 16384（实现需求 §7.3 原文）、超限显式拒绝、上下文默认 20480（使默认组合通过既有容量校验）、checkpoint `tokens=` 记录生效值（新旧失配属预期迁移）、D5.0 前置硬门槛（`n_ctx_train` 可达性，不可达则回退最大可行预算）、与 D2 的顺序依赖（预算 ×4 的安全前提是环早停）、金集 no-op 论证（30 页全部 0 截断）。
- **联动修订**：目标 2.1.7 新增；§2.2 删除"G7 只做文档对齐"；§3.3 契约矩阵新增 parameters 行；§3.5 纯截断页建议文本改为按预算状态分支；§3.6 冻结清单移除预算项；D4.1/D4.3、场景 C、§5.1/§5.3、风险表同步；决策 D-4 废止、D-5 补条件、新增 D-12。

**v2.3（2026-10-31）**——v2.2 复审 PASS 维持，剩余 1 项 MAJOR 与 3 项 MINOR 并入：

- **MAJOR-1（D5.0 门槛区间缺口）**：§3.7.3 回退条件由"`n_ctx_train < 18432` 则回退输出预算"改为公式化规则——`context_default = min(20480, n_ctx_train)`、`output_default = min(16384, n_ctx_train − 2048)`。理由：v2.2 的门槛只守护 18432 下限，实测值落在 [18432, 20480) 时将继续执行"输出 16384 + 上下文 20480 > 训练窗口"的组合——超窗解码发生在输出接近 16K 的极端页，恰是预算上调后新暴露的形态；硬门槛必须守护它自己的默认值。D5.0/D5.2 任务行同步。
- **MINOR-1（CLI 上限镜像）**：D5.1 与 §5.1 预算契约增加 CLI 校验镜像 16384 上限——`--max-output-tokens 20000` 在扫描启动前一次性前置报错，而非通过 CLI 校验后逐页在适配器处失败。
- **MINOR-2（§5.3 规则 3 可执行性）**：比对规则排除易变字段 `elapsed_ms` 及任何计时派生字段（该措辞自 v2 起遗留，本次补记修正）。
- **MINOR-3（幂等措辞残留）**：§5.1 幂等 bullet 由"文本与警告均不变"统一为 §2.3 口径——结果文本与首次裁剪逐字节一致，且不产生 `repetition_removed`/`degenerated` 标志。
