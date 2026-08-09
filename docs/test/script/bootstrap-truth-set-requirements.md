# 真值集半自动引导脚本 — 需求说明

状态：需求已细化，待实现  
关联：`docs/test/p0-30-page-truth-quality-gate.md`、`assets/regression/README.md`  
目标脚本（拟定）：`scripts/bootstrap-truth-set.py`（实现语言可改为 Rust 子命令，接口保持一致）

## 1. 要解决什么问题

人工从零写 30 页 `PageDocument` 真值太慢。

期望流程：

1. 你给出一批 PDF，并指定要从哪些页取样。  
2. 脚本调用当前 `ocrx` 扫这些页，把能自动填的字段先填进草稿真值集。  
3. 你对着**整页预览图**改草稿 JSON，改完标记为已校准。  
4. 校准通过后的文件才进入正式质量门禁（不是把 OCR 输出直接当真值）。

一句话：**OCR 出草稿，人改成真值。**

---

## 2. 明确不做的事

| 不做 | 原因 |
|---|---|
| 不声称输出已是“真值” | 草稿未经人工校准，不能用于发布门禁 |
| 不根据评分结果反向改真值 | 会造假抬分 |
| 不自动凑齐 8 类覆盖并宣称合格 | 类别只能**建议**，最终由人确认 |
| 不替代 `validate-regression-set.py` | 正式校验仍走现有脚本 |
| 不在本需求里实现完整 GUI 标注器 | v1 用目录 + JSON + 预览图 + 清单即可 |

---

## 3. 你怎么用（目标命令）

### 3.1 输入：选页清单

你先准备一个选页文件（CSV 或 JSON 二选一，脚本两种都认）。CSV 示例：

```csv
source_pdf,source_page,note
test-data/教材A.pdf,3,公式多
test-data/教材A.pdf,12,表格
test-data/论文B.pdf,1,双栏英文
test-data/论文B.pdf,8,空白页候选
```

规则：

- `source_pdf`：相对仓库根或绝对路径。  
- `source_page`：PDF 内页码，从 **1** 开始。  
- 一行 = 回归集里的一页；行顺序决定真值页号 `page-0001`、`page-0002`…  
- 至少 30 行才建议导出正式集；脚本在行数 < 30 时警告，但不强制失败（方便你先试跑 5 页）。

### 3.2 一键引导（推荐）

```bash
python3 scripts/bootstrap-truth-set.py \
  --selection docs/test/script/examples/selection.example.csv \
  --set-id ovisocr2-draft-v1 \
  --out assets/regression/ovisocr2-draft-v1 \
  --ocrx target/release/ocrx \
  --profile compact \
  --dpi 300
```

做完这些事：

1. 把各 PDF 选中页拷贝/记录进 `inputs/`（见 §5）  
2. 渲染每页预览 PNG（给人看的）  
3. 调 `ocrx scan` 生成 baseline OCR  
4. 把 OCR 结果转成草稿 `truth/page-NNNN.json`  
5. 写 `manifest.json` 草稿  
6. 写 `review/checklist.md`（你按这个改）

### 3.3 分步命令（长任务可拆开）

| 子命令 | 作用 |
|---|---|
| `init` | 建目录、写空 manifest、校验选页文件 |
| `render` | 只出预览图（不跑模型，快） |
| `ocr` | 调 `ocrx scan` 跑选中页 |
| `draft` | 把 OCR 产物转成 `truth/*.json` 草稿 |
| `status` | 打印：多少页草稿 / 已校准 / 缺预览 / 类别覆盖缺口 |
| `export-gate` | 仅导出 `review_status=approved` 的页到可跑门禁的目录 |

默认 `bootstrap` = `init → render → ocr → draft → status`。

---

## 4. 当前 `ocrx` 能提供什么、缺什么

### 4.1 现有产物（本机已验证）

对每个输出目录，`ocrx scan` 会写出：

| 文件 | 内容 |
|---|---|
| `page-NNNN.md` | 该页 Markdown（正文、`<table>`、`$$...$$`、图片引用） |
| `page-NNNN.checkpoint` | 兼容键、是否完成 |
| `page-NNNN.render-audit` | 渲染审计 |
| `images/page-NNNN.png` | 整页图（任务结束可能被清理，见下） |
| `images/page-NNNN-figure-MMM.png` | 裁切插图 |
| `document.md` | 合并 Markdown |

**没有**按页写出 `PageDocument` JSON。这是本脚本必须自己补的一环。

### 4.2 草稿 JSON 怎么从 OCR 来（v1 策略）

优先级从高到低：

1. **优先（若实现期 CLI 已支持）**：`ocrx scan` 增加按页写出 `page-NNNN.page.json`（即 adapter 解析后的 `PageDocument`）。脚本直接拷贝为草稿，再补 `categories` / `review_*` 字段。  
2. **兜底（当前即可做）**：读 `page-NNNN.md`，按与 `ocrx-ovisocr2` 相同规则拆块：  
   - `$$...$$` → `formula`（`display: true`）  
   - `<table>...</table>` → `html_table`  
   - `<img src="images/page-NNNN-figure-MMM.png" />` → `visual_region`（bbox 先填占位或从文件名关联；**坐标不可信，必须人改**）  
   - 其余 → `markdown`  
   - `termination`：checkpoint 成功则 `eos`；失败页 `invalid` 或按扫描结果标记  

无论走哪条路径，草稿文件必须带元数据标明来源是 OCR，见 §6。

### 4.3 整页预览图

人工校准必须能看图。脚本 `render` 阶段要保证：

```text
preview/page-0001.png
preview/page-0002.png
...
```

实现方式二选一（实现时选稳的）：

- 直接调现有 PDFium / `pdfium-worker` 按选页渲 PNG；或  
- 从 `ocrx` 输出的 `images/page-*.png` **在清理前拷贝**到 `preview/`（`ocrx` 成功结束后会删整页 PNG，所以必须在 scan 后立刻拷，或开 `keep-pages` 类选项）。

校准人只需要打开 `preview/page-NNNN.png` 对照改 `truth/page-NNNN.json`。

---

## 5. 输出目录结构

```text
assets/regression/<set-id>/
  manifest.json                 # 草稿，校准完成后人补齐 license/revision
  selection.csv                 # 你提供的选页清单副本
  page-map.json                 # 回归页号 → 源 PDF + 源页码
  inputs/
    acquisition.md              # 自动生成：各 PDF 路径、sha256、是否可再分发（默认 false）
    # 可选：复制或硬链接源 PDF；大文件默认只记路径+hash，不复制
  work/
    ocr/<source-id>/            # 各源 PDF 的 ocrx 原始输出（可删）
  preview/
    page-0001.png
    ...
  truth/
    page-0001.json              # 草稿或已校准
    ...
  review/
    checklist.md                # 逐页待办
    coverage.md                 # 类别覆盖统计与缺口
  bootstrap-report.json         # 本次引导运行记录
```

`page-map.json` 示例：

```json
{
  "schema_version": 1,
  "pages": [
    {
      "page_number": 1,
      "source_pdf": "test-data/教材A.pdf",
      "source_page": 3,
      "source_sha256": "..."
    }
  ]
}
```

说明：正式门禁文档里写的是「一个 source.pdf」。多 PDF 取样时：

- **引导阶段**允许多源（本脚本的核心价值）。  
- **导出门禁集**时二选一（实现期选定并写进 README）：  
  - A. 拼成单一 PDF 再算一个 `input_sha256`；或  
  - B. manifest 扩展为多文件清单（需同步改 `validate-regression-set.py`）。  

本需求默认采用 **A（拼页导出）**，少改现有校验脚本；拼页发生在 `export-gate`，不在日常校准路径里打断你。

---

## 6. 草稿真值文件格式

在 `PageDocument` v1 上增加引导专用字段（评分器应忽略未知字段；若现有 scorer 过严，导出时剥掉这些字段）。

```json
{
  "schema_version": 1,
  "page_number": 1,
  "categories": ["formula", "mixed"],
  "blocks": [ ],
  "termination": "eos",
  "warnings": [],

  "review_status": "draft",
  "bootstrap": {
    "source": "ocrx",
    "ocrx_profile": "compact",
    "ocrx_commit": "<可选>",
    "generated_at": "2026-08-06T12:00:00+08:00",
    "needs_human": [
      "reading_order",
      "formula_latex",
      "table_html",
      "bbox",
      "categories"
    ]
  }
}
```

### 6.1 `review_status` 取值

| 值 | 含义 |
|---|---|
| `draft` | 脚本刚生成，未人工看 |
| `in_review` | 正在改 |
| `approved` | 人确认可进门禁 |
| `rejected` | 该页作废，换页重来 |

只有 `approved` 能进 `export-gate`。

### 6.2 脚本能自动填 vs 必须人改

| 字段 | 脚本 | 人 |
|---|---|---|
| `schema_version` / `page_number` | 自动 | 一般不动 |
| `blocks[].markdown` | OCR 填初稿 | **必校**：错字、漏行、阅读顺序 |
| `blocks[].formula.latex` | 有则填 | **必校**：符号、上下标 |
| `blocks[].html_table` | 有则填 | **必校**：合并单元格、行列 |
| `blocks[].visual_region.bbox` | 常不可靠/占位 | **必校**：对着 preview 改到 `[0,1000)` |
| `termination` / `warnings` | 按 OCR 结果填 | 失败/截断页再确认 |
| `categories` | 按规则**建议** | **必确认**；可改可增删 |
| `review_status` | 初始 `draft` | 改完设 `approved` |
| manifest 的 license / revision | 写占位 | 发布前人手补齐 |

### 6.3 `categories` 自动建议规则（可改，须可测）

对一页草稿扫描 `blocks` 后建议：

| 条件 | 建议标签 |
|---|---|
| 有 `formula` | `formula` |
| 有 `html_table` | `table` |
| 有 `visual_region` 或 figure 图文件 | `visual` |
| 同时命中以上 ≥2 类，或正文+任一结构 | `mixed` |
| 仅正文、无公式表图 | `text` |
| Markdown 去空白后长度 < 20 | `blank`（建议，人确认） |
| `termination == max_tokens` 或 warnings 含 truncated | `truncated` |
| OCR 失败 / 无 md / termination invalid | `failure` |

一页可多个标签。脚本写进 JSON，并在 `review/coverage.md` 统计「建议覆盖」与「距每类 ≥2 页还差几个」。

---

## 7. 对 `ocrx` 的调用约定

```bash
ocrx scan <pdf> \
  --output-dir <work/ocr/<source-id>> \
  --pages <逗号与区间，如 3,12,20-22> \
  --model-profile <profile> \
  --overwrite
```

要求：

- 默认 profile：`compact`（本机已有）；可用 `--profile` 改。  
- 同一 `source_pdf` 的多页合并成一次 `--pages`，少启进程。  
- 扫描失败的页：仍生成草稿 JSON（`blocks` 可空，`review_status=draft`，`categories` 含建议 `failure`），并在 checklist 标红。  
- 环境变量 / PDFium 路径与本机现有用法一致；脚本把实际命令记进 `bootstrap-report.json`，方便复现。

若实现期给 CLI 加 `--emit-page-json`，脚本优先用它。

---

## 8. 人工校准工作流（脚本配合的部分）

### 8.1 `review/checklist.md`（自动生成）

每页一行，例如：

```markdown
# 校准清单 — ovisocr2-draft-v1

进度：approved 0 / 30 · draft 30 · rejected 0

| 回归页 | 预览 | 真值 JSON | 建议类别 | 状态 | 你要做的事 |
|---:|---|---|---|---|---|
| 1 | preview/page-0001.png | truth/page-0001.json | formula,mixed | draft | 对图改公式与 bbox，确认阅读顺序 |
| 2 | ... | ... | text | draft | 校对正文 |
```

### 8.2 人怎么改

1. 打开对应 `preview/page-NNNN.png`。  
2. 编辑 `truth/page-NNNN.json` 的 `blocks` / `categories`。  
3. 把 `review_status` 改为 `approved`。  
4. 跑：

```bash
python3 scripts/bootstrap-truth-set.py status --out assets/regression/<set-id>
```

5. 全员 `approved` 且类别覆盖够之后：

```bash
python3 scripts/bootstrap-truth-set.py export-gate \
  --out assets/regression/<set-id> \
  --gate-out assets/regression/<set-id>-gate
```

`export-gate` 输出：

- 去掉 `bootstrap` / 或保留但 scorer 可忽略  
- 拼好的单一输入 PDF（若采用方案 A）  
- 可直接喂给 `validate-regression-set.py` 的目录  

---

## 9. 验收标准（脚本本身怎么算做完）

用仓库内小样本（例如已有 10 页教材 OCR 结果，或 3 个 PDF 各抽 2 页）跑通：

| # | 验收项 | 通过标准 |
|---|---|---|
| 1 | 选页文件非法 | 非空路径、页码 ≥1；错误信息可读，退出码 ≠0 |
| 2 | `init` | 目录与 `page-map.json` 生成正确 |
| 3 | `render` | `preview/page-*.png` 数量 = 选页行数，能打开 |
| 4 | `ocr` | 对可扫页写出 `page-*.md`；失败页有记录不中断整批（除非 `--fail-fast`） |
| 5 | `draft` | 每页有 `truth/page-NNNN.json`，含 `review_status=draft` 与 `blocks` |
| 6 | 公式/表回填 | 对含 `$$` / `<table>` 的 md，草稿里出现对应 block 类型 |
| 7 | `status` | 打印类别建议计数与距「每类 2 页」的缺口 |
| 8 | 覆盖不误报合格 | 即使建议标签齐全，未全部 `approved` 时 `export-gate` 拒绝 |
| 9 | `export-gate` | 仅导出 approved；导出目录能通过 `validate-regression-set.py`（页数与类别由测试夹具构造） |
| 10 | 报告 | `bootstrap-report.json` 含命令、profile、时间、成功/失败页列表 |

---

## 10. 实现分期

### v1（先做，立刻减负）

- 选页 CSV/JSON  
- `init` / `render` / `ocr` / `draft` / `status`  
- Markdown → `PageDocument` 草稿  
- 类别建议 + checklist  
- 不强制拼 PDF；`export-gate` 可暂要求单源 PDF，或多源时只导出 truth + page-map 并提示「拼 PDF 待 v1.1」

### v1.1

- 多源拼成单一 gate PDF + 正确 `input_sha256`  
- 可选：`ocrx --emit-page-json` 对接  
- bbox：若 md 无坐标，提供「从 figure 裁切框反推占位」的尽力而为逻辑（仍标 `needs_human`）

### 不做进脚本的（另工具）

- 可视化拖拽改 bbox  
- 多人标注冲突合并  
- 自动调用质量门禁四档跑分  

---

## 11. 与质量门禁的关系

```text
一批 PDF + 选页
    → bootstrap-truth-set（本需求）
    → 人工校准 review_status=approved
    → export-gate
    → validate-regression-set.py
    → 四档预测 + score / run-quality-gate
    → GO / NO-GO
```

引导脚本产出的是**草稿流水线**；`docs/test/p0-30-page-truth-quality-gate.md` 里的正式门禁，只吃校准后的导出结果。

---

## 12. 已拍板的默认假设（实现按此做；要改先改本文档）

1. 脚本入口：`scripts/bootstrap-truth-set.py`（Python3，少依赖；需要 PDF 渲图时调仓库已有二进制/库）。  
2. 默认 OCR profile：`compact`。  
3. 默认 DPI：`300`。  
4. 多 PDF 选页：引导期支持；门禁导出默认拼成单 PDF（v1.1）。  
5. 草稿带 `review_status`；未 `approved` 不得当真值。  
6. 版权字段默认 `redistributable: false`，只记 provenance 路径与 hash。  
7. 不把模型输出静默当成最终真值写入无标记文件。

---

## 13. 实现前仍可微调的两点（不影响先写 v1）

1. 选页格式最终以 CSV 为主还是 JSON 为主（建议 CSV 给人填，JSON 给程序）。  
2. bbox 占位：全零无效框 vs 缺省不写 visual_region（建议：**有 figure 文件则写 block，bbox 用明显占位如 0,0,1,1 并列入 needs_human**，避免人漏标图）。

确认本需求后，下一步就是按 §10 实现 v1 脚本，并在 `docs/test/script/examples/` 放一份示例选页文件。
