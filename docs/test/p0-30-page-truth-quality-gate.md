# P0：30 页真值质量门禁测试计划

状态：待执行（缺人工真值集与四档模型对照资产）  
依据：`docs/requirement/requirements-v0.1.md` §17.2、`assets/regression/README.md`  
脚本：`scripts/validate-regression-set.py`、`scripts/score-regression.py`、`scripts/run-quality-gate.py`

## 1. 测什么、怎样算过

本门禁回答一件事：**OvisOCR2 四个 GGUF profile，在同一份 ≥30 页人工校对真值上，是否达到发布阈值。**

| 结果 | 条件 |
|---|---|
| 通过 | 真值集校验通过；`reference` / `quality` / `balanced` / `compact` 全部达标；GGUF `reference` 相对同 revision 官方 BF16 的下降不超过规定差值 |
| 失败 | 任一 profile 任一指标不达标，或 GGUF↔BF16 偏差超限，或出现重复/截断/空白失败页 |
| 阻断发布 | `reference` 失败，或 `quality` 失败，或 `compact` 失败（见需求 §17.2.4） |

不得用第二个模型、隐藏 fallback、或缩小测试页数来“凑过”。

---

## 2. 开始前必须准备齐的东西

### 2.1 人

| 角色 | 做什么 |
|---|---|
| 标注人 | 逐页写/改真值 JSON；公式、表格、bbox 必须人工看图校对 |
| 版权负责人 | 确认 PDF 能否进仓库，或只能本机私有存放 |
| 跑数人 | 装模型、跑 scan、跑评分脚本、归档报告 |

### 2.2 机器

| 用途 | 建议配置 | 说明 |
|---|---|---|
| 日常标注 / compact 跑数 | 本机 M2 16GB + Metal | 已验证可跑 compact |
| quality / reference / BF16 | ≥24GB 内存机，或本机分批长时间跑 | reference≈1.5GB+mmproj，BF16 更吃内存 |
| 公共：PDFium dylib | 与本机验证相同路径即可 | 见 `docs/develop/local-machine-validation-2026-08-06.md` |

### 2.3 模型文件（缺一不可）

同一 **model ID + revision** 下准备：

1. `compact` GGUF + mmproj  
2. `balanced` GGUF + mmproj  
3. `quality` GGUF + mmproj  
4. `reference` GGUF + mmproj  
5. 同 revision 官方 **Safetensors BF16**（用于和 GGUF `reference` 比偏差）

当前本机只有 compact，门禁不能只报 compact 结果。

### 2.4 软件与版本（写入报告，改一项就换基准版本号）

记录并冻结：

- `ocrx` git commit  
- llama.cpp / MTMD commit  
- adapter / prompt 版本  
- 渲染：DPI（主门禁用 **300**）、像素预算（与模型一致，常见 `1048576`）  
- 采样：`temperature=0.0`，随机种子固定  
- 评分脚本路径与脚本自身的 git commit  

---

## 3. 回归集怎么建

### 3.1 目录长什么样

建议落在（可分发则进仓库；不可分发则本机目录 + 只提交 hash/说明）：

```text
assets/regression/<set-id>/
  manifest.json
  inputs/
    source.pdf                 # 或 acquisition.md 写明如何拿到同一文件
  truth/
    page-0001.json
    page-0002.json
    ...
    page-0030.json             # 至少 30 个，文件数必须等于 manifest.page_count
  predictions/                 # 跑数后生成，可进发布工件，不必进主仓
    bf16/300/page-NNNN.json
    reference/300/page-NNNN.json
    quality/300/page-NNNN.json
    balanced/300/page-NNNN.json
    compact/300/page-NNNN.json
  reports/
    quality-gate-<date>.json
```

`<set-id>` 举例：`ovisocr2-zh-en-v1`。改真值或改 PDF 就换新 set-id，不要覆盖旧集。

### 3.2 选页清单（至少 30 页）

先做一张表，再动手标注。每行一页：PDF 页码、类别标签、语言/版式备注。

**脚本强制类别（每类 ≥2 页）**——`validate-regression-set.py` 会查：

| 类别 `categories` | 这一页要有什么 |
|---|---|
| `text` | 以正文为主，无明显公式/表/图干扰 |
| `formula` | 至少 1 个行内或块级公式 |
| `table` | 至少 1 个表格 |
| `visual` | 至少 1 个插图/图表，需标 bbox |
| `mixed` | 正文+公式+表或图混排 |
| `blank` | 几乎无字或空白页（真值 blocks 可为空或仅极少内容） |
| `truncated` | 故意选超长页，或标注为截断场景（用于压测 token 上限） |
| `failure` | 加密页、损坏页、或预期应失败/告警的页 |

**需求还要求覆盖这些内容（可叠在上面各类里）**：

- 简体中文、繁体中文、英文、中英混排  
- 双栏论文  
- 行内公式 + 块级公式  
- 简单表 + 复杂表（合并单元格更好有）  
- 插图、图表  
- 扫描噪声页  
- 竖排或旋转文字  
- 超长输出页  

实用凑页法：从已有教材 PDF 抽 24～26 页正常页，再单独准备 2 页空白、2 页失败/截断样例，总数 ≥30。

### 3.3 `manifest.json` 必填字段

对照 `assets/regression/manifest.example.json`，填完整再跑校验：

```json
{
  "schema_version": 1,
  "set_id": "ovisocr2-zh-en-v1",
  "page_count": 30,
  "input_sha256": "<source.pdf 的 sha256>",
  "renderer": {"dpi": 300, "pixel_budget": 1048576},
  "prompt_revision": "<prompt git 或版本号>",
  "adapter_revision": "<adapter git 或版本号>",
  "runtime_revision": "<llama.cpp/MTMD commit>",
  "model_revision": "<OvisOCR2 model revision>",
  "license": {
    "redistributable": false,
    "provenance": "来源说明；不可再分发则写清楚仅本机使用"
  },
  "required_categories": [
    "text", "formula", "table", "visual",
    "mixed", "blank", "truncated", "failure"
  ]
}
```

`input_sha256` 用：

```bash
shasum -a 256 assets/regression/<set-id>/inputs/source.pdf
```

### 3.4 真值文件长什么样

每个 `truth/page-NNNN.json` = **PageDocument v1** + 覆盖用的 `categories` 数组。

最小例子（有正文、公式、表、图）：

```json
{
  "schema_version": 1,
  "page_number": 1,
  "categories": ["mixed", "formula", "table", "visual"],
  "blocks": [
    {"type": "markdown", "content": "# 标题\n\n第一段正文。\n"},
    {"type": "formula", "latex": "E = mc^2", "display": true},
    {"type": "markdown", "content": "\n"},
    {"type": "html_table", "html": "<table><tr><td>A</td><td>B</td></tr></table>"},
    {"type": "markdown", "content": "\n图示：\n"},
    {
      "type": "visual_region",
      "bbox": {"left": 100, "top": 200, "right": 300, "bottom": 400},
      "source_order": 0
    }
  ],
  "termination": "eos",
  "warnings": []
}
```

标注规则（写死，避免两人标注不一致）：

1. **阅读顺序**：`blocks` 数组顺序 = 人眼从上到下、从左到右的阅读顺序。双栏先左栏后右栏。  
2. **正文**：放进 `markdown`；不要把公式/表 HTML 再抄进正文里重复计分。  
3. **公式**：只写 LaTeX 本体；无公式页不要塞空 `formula` block。  
4. **表格**：写结构清晰的 HTML `<table>...</table>`；去掉纯样式属性。  
5. **插图 bbox**：坐标系与产品一致，归一化到 `[0,1000)`；`left < right`，`top < bottom`，面积 > 0。  
6. **空白页**：`blocks` 可以为 `[]`，`categories` 含 `blank`。  
7. **失败页**：可标 `termination` 为预期状态，并在页备注/报告里写“预期失败原因”。  
8. **禁止**：根据模型输出“倒着改真值”来抬分；真值只对原图/PDF。

参考样例：`src/ocrx-ovisocr2/tests/fixtures/reference/page-01.page.json`（无 `categories`，真值集需自行加上）。

### 3.5 真值集自检（标注完成后第一件事）

```bash
python3 scripts/validate-regression-set.py assets/regression/<set-id>
```

期望：打印 JSON，含 `pages >= 30` 和各类别计数；退出码 `0`。

常见失败原因：

- 页数 < 30，或 `truth` 文件数 ≠ `page_count`  
- 某类别少于 2 页  
- 缺 `input_sha256` / `prompt_revision` / `adapter_revision`  
- 某页 `schema_version` ≠ 1 或没有 `page_number`

---

## 4. 预测结果怎么生成

### 4.1 固定跑数条件

对每个 profile 使用**同一份** PDF、**同一组**页码：

| 项 | 值 |
|---|---|
| DPI | 主报告 **300**；可选加跑 200、250 做对比（`run-quality-gate.py` 默认扫三档 DPI） |
| 像素预算 | 与模型一致（manifest 里写死的那个） |
| temperature | `0.0` |
| 页范围 | 与真值 `page_number` 一一对应 |

### 4.2 用 CLI 扫页（示例）

把 `<set-id>`、PDF、profile 换成实际值。输出目录按 profile/DPI 分开，避免互相覆盖：

```bash
# 例：compact @ 300 DPI，输出到 predictions 树
cargo run -p ocrx-cli --release -- scan \
  --input assets/regression/<set-id>/inputs/source.pdf \
  --output /tmp/ocrx-gate/<set-id>/compact-300 \
  --profile compact \
  --pages 1-30 \
  --overwrite
```

然后把每页 checkpoint / 页面 JSON **整理成**与真值同名的：

```text
predictions/compact/300/page-0001.json
...
predictions/compact/300/page-0030.json
```

文件内容必须是 **PageDocument**（与真值同 schema），不是整份 `document.md`。若 CLI 当前只落 Markdown，需从页面 checkpoint 或 adapter 解析结果导出 JSON；导出脚本应固定并纳入同一次基准版本。

对 `balanced` / `quality` / `reference` 各做一遍。BF16 官方路径单独跑，结果放到 `predictions/bf16/300/`。

### 4.3 页面对齐检查

跑分前确认：

```bash
# 真值与预测页码文件齐全
ls assets/regression/<set-id>/truth/page-*.json | wc -l
ls predictions/compact/300/page-*.json | wc -l
# 两数必须等于 manifest.page_count
```

缺一页，评分脚本直接失败（这是预期行为）。

---

## 5. 怎么打分

### 5.1 单 profile 打分

```bash
python3 scripts/score-regression.py \
  assets/regression/<set-id>/manifest.json \
  assets/regression/<set-id>/truth \
  predictions/compact/300
```

标准输出是一份 JSON 报告（含逐页分与汇总）。保存到：

```text
assets/regression/<set-id>/reports/score-compact-300.json
```

### 5.2 多 profile × DPI 一键跑

```bash
python3 scripts/run-quality-gate.py \
  assets/regression/<set-id>/manifest.json \
  assets/regression/<set-id>/truth \
  predictions
```

要求 `predictions/<profile>/<dpi>/page-NNNN.json` 目录结构齐全。脚本会遍历：

- profile：`compact` / `balanced` / `quality` / `reference`  
- DPI：`200` / `250` / `300`  
- pixel_budget：`1048576`

主发布判定以 **300 DPI** 行为准；200/250 用于质量-耗时对比，写进报告附录即可。

### 5.3 评分前允许的文本归一化（仅这些）

- Unicode NFC  
- CRLF → LF  
- 去掉行尾空白  
- 连续 ≥3 个空行压成 2 个空行  

**不允许**：忽略大小写、忽略标点、简繁转换、全角半角混算。

### 5.4 指标与发布阈值（对照需求 §17.2.3）

| 指标 | reference | quality | balanced | compact |
|---|---:|---:|---:|---:|
| 文本 NED（越高越好） | ≥ 0.97 | ≥ 0.95，且相对 reference 降幅 ≤ 0.02 | ≥ 0.93，且相对 reference 降幅 ≤ 0.04 | ≥ 0.90，且相对 reference 降幅 ≤ 0.07 |
| 公式完整率 | ≥ 0.95 | ≥ 0.92，Δ≤0.03 | ≥ 0.88，Δ≤0.07 | ≥ 0.82 |
| 表格 TEDS | ≥ 0.95 | ≥ 0.92，Δ≤0.03 | ≥ 0.88，Δ≤0.07 | ≥ 0.82 |
| 阅读顺序准确率 | ≥ 0.98 | ≥ 0.96 | ≥ 0.94 | ≥ 0.92 |
| bbox 有效率 | ≥ 0.98 | ≥ 0.97 | ≥ 0.95 | ≥ 0.93 |
| bbox 召回率 | ≥ 0.95 | ≥ 0.92 | ≥ 0.88 | ≥ 0.82 |
| 重复输出页 | 0 | 0 | 0 | 0 |
| 截断页 | 0 | 0 | 0 | 0 |
| 空白/失败页 | 0 | 0 | 0 | 0 |

另：**GGUF `reference` vs 官方 BF16** 最大允许下降：

| 指标 | 最大下降 |
|---|---:|
| 文本 NED | 0.01 |
| 公式完整率 | 0.02 |
| 表格 TEDS | 0.02 |
| 阅读顺序 | 0.01 |
| bbox 有效率或召回率 | 0.02 |

公式/表格/bbox **只在有对应真值的页上算**；报告必须写出样本数。没有公式的页不算公式满分。

### 5.5 跑分前先核对评分器

当前 `scripts/score-regression.py` 是可运行骨架。正式门禁前必须人工核对它是否与 §17.2.2 一致，至少查这几项：

| 检查项 | 需求要求 | 若不满足则 |
|---|---|---|
| 文本 NED | `1 - edit_distance / max(len_ref, len_hyp)`，按真值字符数加权 | 先改脚本，再重跑全 profile |
| 公式 | 规范化 LaTeX + KaTeX 可解析，再按序匹配 | 同上 |
| 表格 | DOM Tree Edit Distance Similarity（TEDS） | 同上 |
| 阅读顺序 | 有序文本块 pairwise order accuracy | 同上 |
| bbox | IoU ≥ 0.5 一对一匹配；有效率看语法/坐标/可裁图 | 同上 |

**规则：评分器有 bug 就修脚本并升基准版本号，禁止为过门而放宽阈值。**

---

## 6. 执行顺序（按天可跟）

### 第 0 天：冻结版本

1. 选定 model revision、GGUF 来源、llama.cpp commit。  
2. 建空目录与 `manifest.json` 草稿。  
3. 确认版权方案（入库 vs 本机私有）。

### 第 1～N 天：选页 + 标注

1. 填选页表（≥30 页，类别齐）。  
2. 渲染或截图对照 PDF 写真值 JSON。  
3. 第二人抽查不少于 5 页（公式/表/bbox 必查）。  
4. 跑 `validate-regression-set.py`，修到退出码 0。

### 跑数日：生成预测

1. 安装四档 GGUF + mmproj，`ocrx model verify` 逐档确认。  
2. 按 §4 生成 `predictions/<profile>/300/`。  
3. （可选）生成 200/250。  
4. 跑 BF16 对照，写入 `predictions/bf16/300/`。

### 评分日：出报告 + 判定

1. 核对评分器（§5.5）。  
2. 跑 `score-regression.py` / `run-quality-gate.py`。  
3. 用 Excel/表格填「实测 vs 阈值」，标 PASS/FAIL。  
4. 按 §7 写正式报告并归档。  
5. 按 §1 / §17.2.4 给出 go/no-go 与默认 profile 建议。

---

## 7. 报告必须包含的内容

文件建议：`docs/test/reports/quality-gate-<set-id>-<YYYY-MM-DD>.md`（或同内容放在回归集 `reports/`）。

至少写清：

1. set-id、page_count、input_sha256  
2. model ID、revision、各 profile 量化名、mmproj revision  
3. runtime / adapter / prompt / ocrx commit  
4. DPI、像素预算、temperature、种子  
5. 每个 profile 的：指标得分、样本数、相对 reference 差值、失败页列表  
6. GGUF reference vs BF16 差值表  
7. 最终结论一行：`GO` 或 `NO-GO`，以及 8GB/16GB/24GB 默认 profile  

禁止只写“通过”而不留原始分数 JSON。

---

## 8. 判定后怎么选默认 profile

按需求 §17.2.4 顺序执行，不要跳步：

1. `reference` 挂 → **NO-GO**（GGUF/llama 路径不合格）  
2. `quality` 挂 → **NO-GO**（不能用更低量化顶替）  
3. `balanced` 过 → 16GB 默认 `balanced`  
4. `balanced` 挂但 `quality` 过 → 16GB 默认升为 `quality`，并改文档里的内存要求  
5. `compact` 过 → 8GB 默认 `compact`  
6. `compact` 挂 → **NO-GO**（不能取消 8GB 支持，也不能默默下更大模型把机器打进 swap）  
7. ≥24GB 默认用已通过的 `quality`

---

## 9. 本计划之外、不要混进这次门禁的事

- 单元测试 / `cargo test`（已有本机记录，不算本质量门禁）  
- P6 干净环境安装包烟测（另做发布证据包）  
- 用教材 OCR 出的 Markdown 当“真值”（那是模型输出，不是人工真值）

---

## 10. 一页纸检查清单

复制下面打勾：

- [ ] PDF 选定，sha256 写入 manifest  
- [ ] ≥30 页真值 JSON，文件名 `page-0001` 起连续  
- [ ] 8 类 `categories` 每类 ≥2 页  
- [ ] `validate-regression-set.py` 退出码 0  
- [ ] compact / balanced / quality / reference 权重齐  
- [ ] 同 revision BF16 对照结果齐  
- [ ] 评分器已对照 §17.2.2 核对（或已修并升版本）  
- [ ] 300 DPI 四档预测 JSON 齐  
- [ ] 四档分数对照阈值表全部 PASS  
- [ ] GGUF reference vs BF16 差值全部在限内  
- [ ] 重复/截断/空白失败页均为 0  
- [ ] 报告已归档，结论写明 GO/NO-GO 与默认 profile  
