# 真值集半自动引导脚本 — 需求说明

状态：需求已改版（去掉 ocrx 自测自）  
关联：`docs/test/p0-30-page-truth-quality-gate.md`、`assets/regression/README.md`  
外部工具：[run-llama/liteparse](https://github.com/run-llama/liteparse)（CLI：`lit`）  
目标脚本（拟定）：`scripts/bootstrap-truth-set.py`

## 1. 为什么改方案

旧方案用 **ocrx 扫页 → 当草稿真值 → 再评 ocrx**，等于自己测自己：

- 模型常犯的错会写进「真值」，分数虚高  
- 漏检的公式/表/图，真值里也可能没有，门禁测不到  

新原则：

| 角色 | 工具 | 干什么 |
|---|---|---|
| 选页 + 页面特征 | **LiteParse**（独立第三方） | 扫一批 PDF，挑出覆盖各类场景的候选页 |
| 预览图 | **LiteParse** `lit screenshot` | 给人校对用 |
| 草稿正文/表/图线索 | **LiteParse** `lit parse`（可选） | 独立解析器出初稿，**不是** ocrx |
| 最终真值 | **人** | 对图改 JSON，标 `approved` |
| 被测系统 | **ocrx** | 只在质量门禁阶段对已校准真值跑分 |

一句话：**LiteParse 帮你挑页并给独立草稿；人改成真值；ocrx 只当考生。**

---

## 2. 要解决什么问题

人工翻 PDF 找「公式页 / 表页 / 双栏 / 扫描噪声 / 空白」太慢。

期望流程：

1. 你给出一批 PDF（不必手写完整选页表）。  
2. 脚本用 LiteParse **廉价复杂度检测**扫全书，按规则自动挑 ≥30 页候选，并标建议类别。  
3. 你可改选页结果；脚本再渲预览图，可选跑 LiteParse 全文解析填草稿 JSON。  
4. 你对着预览图校准；`approved` 后才进质量门禁。  
5. 门禁里用 ocrx 四档 profile 对真值打分——此时才第一次让 ocrx 碰这些页。

---

## 3. 明确不做的事

| 不做 | 原因 |
|---|---|
| **不用 ocrx 生成真值草稿或选页** | 避免自测自 |
| 不声称 LiteParse 输出已是真值 | 仍须人工校准 |
| 不把 LiteParse 分数当成 ocrx 发布阈值 | 两者模型/路径不同，不可混算 |
| 不根据 ocrx 跑分结果回改真值 | 会造假抬分 |
| 不自动宣称 8 类覆盖已合格 | 类别可建议，须人确认 |
| 不实现完整 GUI 标注器 | v1：目录 + JSON + 预览图 + 清单 |

---

## 4. LiteParse 在本流程里怎么用

依赖：本机安装 `lit`（`pip install liteparse` / `cargo install liteparse` / `npm i -g @llamaindex/liteparse` 任一即可）。  
文档：https://developers.llamaindex.ai/liteparse/  
仓库：https://github.com/run-llama/liteparse

### 4.1 选页主信号：`lit is-complex`（便宜、全书可扫）

```bash
lit is-complex textbook.pdf --compact
```

stdout 是**逐页 JSON**。每页至少用这些字段做挑选（字段名以本机 `lit` 实际输出为准，实现时对一下版本）：

| 信号 | 用来建议哪类页 |
|---|---|
| `needs_ocr` + `reasons` 含 `scanned` / `no-text` / `sparse-text` / `garbled` | 扫描噪声、难页；也可能是 `blank` / `failure` 候选 |
| `layout.reasons` 含 `multi-column` 或 `column_count` ≥ 2 | 双栏论文类 |
| `layout.reasons` 含 `table-likely`，或 `ruled_table_count` / `text_table_run_count` > 0 | `table` |
| `layout.reasons` 含 `dense-graphics`，或 `figure_count` > 0 | `visual` |
| 文本很少 / `is_solid_fill` 类空白信号（若 screenshot 侧有） | `blank` |
| 文本层正常、无表无图、单栏 | `text` |

公式页：`is-complex` 不一定直接标「公式」。v1 做法：

1. 对 `is-complex` 筛出的候选子集，再跑一次  
   `lit parse <pdf> --target-pages "..." --format markdown`（或 json）  
2. 在输出里用启发式找 `$...$` / `$$...$$` / 常见公式 Unicode（∑∫√²ₓ 等）→ 建议 `formula`  
3. 仍不够时，在 `review/coverage.md` 里写「公式类还差 N 页」，你手动补选页。

### 4.2 预览图：`lit screenshot`

```bash
lit screenshot textbook.pdf --target-pages "3,12,20-22" --dpi 300 -o <out>/work/screenshots/<source-id>
```

脚本把选中页拷到统一命名：

```text
preview/page-0001.png
preview/page-0002.png
...
```

### 4.3 草稿内容（可选，仍独立于 ocrx）：`lit parse`

```bash
lit parse textbook.pdf \
  --target-pages "3,12" \
  --format markdown \
  --extract-images \
  --image-output-dir <out>/work/liteparse/<source-id>/images \
  -o <out>/work/liteparse/<source-id>/page-bundle.md
```

或 `--format json` 拿 bbox。脚本把 LiteParse 结果映射成 `PageDocument` **草稿**：

| LiteParse 线索 | 草稿 block |
|---|---|
| 普通段落 / 标题 | `markdown` |
| Markdown 表或可识别表结构 | `html_table`（尽力；不行就整段放 markdown 并标 needs_human） |
| `$$` / 公式启发式命中 | `formula` |
| 嵌入图 / figure 区 | `visual_region`（bbox 从 LiteParse 坐标归一化到 `[0,1000)`；不准就占位并标 needs_human） |

`bootstrap.source` 必须写 `liteparse`，并记录 `lit` 版本。

---

## 5. 你怎么用（目标命令）

### 5.1 输入：PDF 列表（主路径）

不必先手写 30 行选页。准备：

```text
docs/test/script/examples/sources.example.txt
# 每行一个 PDF；# 开头为注释
test-data/教材A.pdf
test-data/论文B.pdf
```

或 CSV：

```csv
source_pdf,max_pages_scan,note
test-data/教材A.pdf,200,苏科数学
test-data/论文B.pdf,40,双栏英文
```

### 5.2 一键引导

```bash
python3 scripts/bootstrap-truth-set.py \
  --sources docs/test/script/examples/sources.example.txt \
  --set-id ovisocr2-draft-v1 \
  --out assets/regression/ovisocr2-draft-v1 \
  --lit lit \
  --target-pages 30 \
  --dpi 300
```

默认流水线：

`probe → select → screenshot → draft → status`

| 阶段 | 做什么 |
|---|---|
| `probe` | 对各 PDF 跑 `lit is-complex`，落盘 `work/probe/<source-id>.json` |
| `select` | 按覆盖规则自动凑页，写出 `selection.csv` + `page-map.json` |
| `screenshot` | `lit screenshot` → `preview/` |
| `draft` | `lit parse` 填 `truth/page-NNNN.json`（`review_status=draft`） |
| `status` | 类别缺口、校准进度 |

### 5.3 仍支持手改 / 手写选页

自动选完后你可以改 `selection.csv`，再：

```bash
python3 scripts/bootstrap-truth-set.py select --from-csv ...   # 跳过自动挑，用你的表
python3 scripts/bootstrap-truth-set.py screenshot --out ...
python3 scripts/bootstrap-truth-set.py draft --out ...
```

手写 CSV 格式不变：

```csv
source_pdf,source_page,note
test-data/教材A.pdf,3,公式多
```

### 5.4 子命令一览

| 子命令 | 作用 |
|---|---|
| `init` | 建目录、写 manifest 占位 |
| `probe` | LiteParse 复杂度扫描 |
| `select` | 自动选页或导入 CSV |
| `screenshot` | 预览图 |
| `draft` | LiteParse → 真值草稿 |
| `status` | 进度与覆盖 |
| `export-gate` | 只导出 `approved` 页 |

**禁止出现的子命令 / 标志：** `ocr`、`--ocrx`、调用 `ocrx scan` 写 truth。

---

## 6. 自动选页规则（v1 必须可测）

目标：在全部 probe 结果里挑约 `--target-pages`（默认 30）页，尽量满足质量门禁 8 类 **每类 ≥2**。

### 6.1 配额（默认）

| 建议类别 | 至少页数 | 优先匹配的 LiteParse 信号 |
|---|---:|---|
| `table` | 2 | `table-likely` / 表计数 > 0 |
| `visual` | 2 | `dense-graphics` / `figure_count` > 0 |
| `mixed` | 2 | 同时有表或图 + 足够正文 |
| `text` | 2 | 单栏、无表无图、文本充足 |
| `blank` | 2 | 几乎无字 / 纯色页 |
| `formula` | 2 | parse 启发式命中；不够则留缺口给人补 |
| `truncated` | 2 | 文本极长页（字符数或块数排名靠前）作**候选**；是否真截断由门禁测 ocrx 时验证，真值侧先标建议 |
| `failure` | 2 | 加密失败、解析失败、或 `garbled` 且无法出字；可另备损坏/加密 PDF |

同一页可带多个 `categories`。`select` 写 `review/selection-rationale.md`：每页为什么入选。

### 6.2 选不够时

- 不伪造页。  
- `status` / `coverage.md` 明确列出缺口，例如：`formula: 有 0，还差 2`。  
- 退出码：自动选页阶段用警告码（如 0 + stderr 警告，或约定 4 = 覆盖不足）；**不得**在覆盖不足时静默 `export-gate` 成功。

### 6.3 去重

同一 PDF 相邻页若特征几乎相同，默认隔页取样（可配置 `--min-page-gap 2`），避免 30 页全是目录。

---

## 7. 输出目录结构

```text
assets/regression/<set-id>/
  manifest.json
  sources.txt                     # 或 sources.csv 副本
  selection.csv                   # 最终选页（可人手改）
  page-map.json
  inputs/
    acquisition.md                # 路径、sha256、redistributable=false
  work/
    probe/<source-id>.json        # lit is-complex 原始结果
    liteparse/<source-id>/        # lit parse / 图片
    screenshots/<source-id>/      # lit screenshot 原始输出
  preview/
    page-0001.png
    ...
  truth/
    page-0001.json
    ...
  review/
    checklist.md
    coverage.md
    selection-rationale.md
  bootstrap-report.json
```

`page-map.json` 增加选页依据（便于审计）：

```json
{
  "schema_version": 1,
  "selector": "liteparse",
  "liteparse_version": "<lit --version>",
  "pages": [
    {
      "page_number": 1,
      "source_pdf": "test-data/教材A.pdf",
      "source_page": 3,
      "source_sha256": "...",
      "suggested_categories": ["table", "mixed"],
      "probe_reasons": ["table-likely"]
    }
  ]
}
```

多源 PDF：引导期支持；`export-gate` 默认拼成单 PDF（v1.1），与门禁 `input_sha256` 对齐。

---

## 8. 草稿真值文件格式

```json
{
  "schema_version": 1,
  "page_number": 1,
  "categories": ["table", "mixed"],
  "blocks": [],
  "termination": "eos",
  "warnings": [],
  "review_status": "draft",
  "bootstrap": {
    "source": "liteparse",
    "liteparse_version": "x.y.z",
    "generated_at": "2026-08-12T22:00:00+08:00",
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

### 8.1 `review_status`

| 值 | 含义 |
|---|---|
| `draft` | 脚本生成，未人工看 |
| `in_review` | 正在改 |
| `approved` | 可进门禁 |
| `rejected` | 作废换页 |

只有 `approved` 能 `export-gate`。

### 8.2 自动填 vs 人改

| 字段 | 脚本（LiteParse） | 人 |
|---|---|---|
| 页号 / schema | 自动 | 一般不动 |
| `markdown` / 表 / 公式初稿 | 有则填 | **必校** |
| `visual_region.bbox` | 尽力归一化或占位 | **必校** |
| `categories` | 按 probe+parse **建议** | **必确认** |
| `review_status` | `draft` | 改完 → `approved` |
| manifest license / ocrx revision | 占位 | 发布前补齐 |

注意：manifest 里的 `model_revision` / `adapter_revision` 等是**门禁跑 ocrx 时**填的元数据，不是 LiteParse 的版本。LiteParse 版本只写在 `bootstrap` / `page-map.selector`。

---

## 9. 人工校准（不变，工具换了）

1. 打开 `preview/page-NNNN.png`。  
2. 改 `truth/page-NNNN.json`。  
3. `review_status` → `approved`。  
4. `status` 看覆盖。  
5. `export-gate` → 再跑 `validate-regression-set.py` 与质量门禁（此时才用 ocrx）。

---

## 10. 验收标准（脚本本身）

| # | 验收项 | 通过标准 |
|---|---|---|
| 1 | 无 `lit` | 清晰报错，告诉如何安装，退出码 ≠0 |
| 2 | `probe` | 每个源 PDF 有 `work/probe/*.json` |
| 3 | `select` | 写出 `selection.csv`；`selection-rationale.md` 非空 |
| 4 | 不调用 ocrx | 代码与报告中不得出现 `ocrx scan` 写 truth |
| 5 | `screenshot` | `preview/page-*.png` 数 = 选页行数 |
| 6 | `draft` | 每页草稿 `bootstrap.source == "liteparse"` |
| 7 | 覆盖缺口可见 | 缺类时 `coverage.md` 写明还差几页 |
| 8 | 未校准不能导出 | 存在非 `approved` 时 `export-gate` 失败 |
| 9 | 小样本 | 用 1～2 个本地 PDF、`--target-pages 6` 跑通全流程 |
| 10 | 报告 | `bootstrap-report.json` 含 `lit` 版本、命令、选页统计 |

---

## 11. 实现分期

### v1

- `probe`（`lit is-complex`）+ `select` + `screenshot` + `status`  
- 手写/改 `selection.csv`  
- 类别建议与覆盖缺口  
- **不**调用 ocrx  

### v1.1

- `draft`：`lit parse` → `PageDocument` 映射  
- 公式启发式  
- 多源拼 gate PDF  

### v1.2（可选）

- 对「公式类缺口」提供交互：列出 parse 命中公式的页供你勾选  

### 明确不做

- 用 ocrx 填真值  
- 可视化拖拽 bbox  
- 在本脚本里跑四档质量门禁  

---

## 12. 与质量门禁的关系

```text
一批 PDF
  → lit is-complex（选页）
  → lit screenshot（预览）
  → lit parse（可选草稿）
  → 人工校准 approved
  → export-gate
  → validate-regression-set.py
  → ocrx 四档预测 + score / run-quality-gate   ← 这里才出现 ocrx
  → GO / NO-GO
```

---

## 13. 已拍板的默认假设

1. 选页与草稿来源：**LiteParse**，不是 ocrx。  
2. CLI 依赖：本机 `lit`；脚本用 `--lit` 指路径。  
3. 默认目标页数：30。  
4. 预览 DPI：300。  
5. 版权默认不可再分发，只记路径与 hash。  
6. 未 `approved` 不得进门禁。  

---

## 14. LiteParse 的能力边界（写进预期，避免误用）

- 本地、非 LLM；复杂扫描件/密表质量有限——所以必须人校。  
- Markdown/表是启发式，公式识别弱——`formula` 类可能要你手补选页。  
- 它**不能**替代人工真值，也**不能**替代 ocrx 发布阈值测试。  

若本机 `lit is-complex` 字段名与上文不完全一致，实现时以 `lit is-complex --help` / 实际 JSON 为准，并在 `bootstrap-report.json` 记录所用版本。
