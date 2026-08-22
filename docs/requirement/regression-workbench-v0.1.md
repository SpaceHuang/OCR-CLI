# OCRX 回归样本工作台需求 v0.1

状态：共识基线（待开发）
日期：2026-08-22
工作名称：`ocrx-workbench`

## 1. 目标

`ocrx-workbench` 是随 OCR-CLI 仓库交付的本地桌面工作台，用于查阅、分析和维护 OCR 回归样本。它将页面预览、人工真值、模型预测和评分结果放在同一审阅界面中，使标注人能定位问题、编辑真值并重跑评分。

它是 OCR-CLI 的质量工具，不是面向任意目录或任意 OCR 引擎的通用标注平台。

## 2. 用户与核心任务

| 角色 | 核心任务 |
| --- | --- |
| 标注人 | 对照预览图创建或修正真值 block、视觉区域 bbox、阅读顺序、类别和审核状态。 |
| 审核人 | 对照真值与预测，查看低分原因，确认或退回标注。 |
| 跑数人 | 浏览 profile/DPI 的预测和报告，重跑评分并导出诊断。 |

## 3. 范围与非目标

### 3.1 v0.1 范围

- 浏览样本集、页面预览、真值、预测和历史评分报告。
- 按类别、审核状态、profile、DPI、低分指标、截断或失败状态筛选和排序页面。
- 在预览图上绘制、选中、移动、缩放、删除视觉区域 bbox。
- 编辑真值 JSON 对应的 block、内容、`source_order`、类别、警告与审核状态。
- 同时叠加真值和预测 bbox，并展示匹配、漏检、误检、重复和 IoU。
- 展示文本差异与页级评分解释；保存真值后将相关报告标为过期，并允许重新运行评分。
- 将真值原子写回固定样本目录，并保存可追溯的修改历史。

### 3.2 非目标

- 不让用户选择任意数据目录、真值目录、预测目录或报告路径。
- 不直接修改模型预测工件。
- v0.1 不启动模型推理、不下载模型、不调度 GPU 任务，也不自动重渲染原始 PDF。
- v0.1 不替代 `ocrx` CLI、评分器或发布质量门禁；它消费并解释这些工件。
- v0.1 不将所有 Markdown、公式、表格 block 的位置纳入正式评分 bbox。

## 4. 固定数据根目录与目录契约

应用必须从自身启动位置定位 OCR-CLI 工作区根目录，而不是依赖当前工作目录或弹出路径选择器。根目录识别应验证至少以下标识：`Cargo.toml`、`assets/regression/`、`scripts/score-regression.py`。

所有数据路径必须限定在工作区内。标准样本目录为：

```text
assets/regression/<set-id>/
  manifest.json
  preview/page-NNNN.png
  truth/page-NNNN.json
  annotations/page-NNNN.json          # 可选：页面布局审阅标注
  predictions/<profile>/<dpi>/page-NNNN.json
  reports/<report>.json
  history/                         # 工作台新增的修改历史
  workbench.json                   # 可选：审核/UI 元数据
```

- 启动后枚举 `assets/regression/*/manifest.json` 作为样本集列表。
- `preview/` 是进入页面审阅的必需输入；缺失时清晰报错，不在 v0.1 自动从 PDF 生成。
- `predictions/` 和 `reports/` 可以缺失；此时仍可进行真值标注，但不得伪造评分结果。
- `annotations/` 可以缺失；缺失不影响现有真值和评分，但该页不具备非视觉 block 的审阅定位信息。
- 内部发布时可由仓库脚本启动应用，以确保 macOS `.app`、开发期 `cargo run` 和命令行启动均可稳定找到工作区根目录。

## 5. 数据所有权、编辑与审计

### 5.1 权威来源

- `truth/page-NNNN.json` 是人工真值的唯一权威来源。
- `predictions/<profile>/<dpi>/page-NNNN.json` 是不可变的模型运行工件，只读。
- `reports/` 是评分输出；真值修改后，引用旧真值版本的报告必须显示为过期。

### 5.2 保存规则

- 保存前必须验证 JSON、页码、block 结构、bbox 边界、非零面积和阅读顺序字段。
- 保存使用原子写入，不能留下半写入 JSON。
- 每次保存都在 `history/` 留存修改前后差异、时间、操作人与修改理由（若无身份系统，操作人记录本机用户）。
- 已批准页面发生任何内容或 bbox 修改后，审核状态自动回退为 `draft`。
- 支持本次会话撤销/重做，以及从历史恢复已保存版本。

### 5.3 审核状态

v0.1 使用 `draft`、`in_review`、`approved` 三种状态。状态及其必要说明可放在 `workbench.json`，避免在未升级 `PageDocument` 前污染现有真值契约。

## 6. 坐标兼容与迁移

### 6.1 内部规范

画布、比较和 IoU 计算统一使用比例坐标 `[0, 1]`；显示层转换为像素坐标。存储层允许以下两种格式：

| 格式 | 文件值 | 内部转换 |
| --- | --- | --- |
| Legacy ratio | `0.23` | 原样使用 |
| Canonical integer | `230` | 除以 `1000` |

新样本集的标准存储格式为 `[0,1000)` 整数。`left < right`、`top < bottom`，边界严格小于 `1000`。

### 6.2 兼容规则

- 坐标格式必须由样本 `manifest.json` 中的显式 `coordinate_system` 声明决定。
- 缺少该字段的既有样本集按 legacy 比例小数读取；现有数据无需批量迁移。
- 编辑旧样本时，默认按其原格式回写。
- “迁移到整数坐标”必须是显式操作，先预览结果，再创建新的 `<set-id>-v2` 样本集；不得覆盖旧基准集。
- 评分器的真值与预测 bbox 都必须先经过同一归一化入口，之后才计算 IoU 和聚合指标。不得通过“坐标是否大于 1”的隐式猜测作为长期契约。

建议的 manifest 扩展：

```json
{
  "coordinate_system": {
    "storage": "normalized_integer",
    "range": "[0,1000)"
  }
}
```

## 7. 语义与布局标注契约

`PageDocument` 保存 OCR 的语义结果和阅读顺序；页面布局标注保存内容在预览图中的审阅位置。两层可关联，但不混合职责：

- `visual_region` 保持现有产品语义：插图或图表、裁图输出和 bbox 发布评分对象。
- Markdown、公式、表格的区域属于人工审阅定位，不改变现有 `visual_region` 的含义，也不进入 v0.1 发布评分。
- 布局区域可以不关联 block；block 也可以没有布局区域。未关联对象在 UI 中必须可见并可筛选。

### 7.1 `PageDocument` v2 与稳定关联

布局关联不得长期以 `source_order` 作为主键，因为重排或插入 block 会改变它。新建或显式迁移的样本集应升级到 `PageDocument v2`，为每个 block 加入不可变的 `id`：

```json
{
  "id": "blk_01JEXAMPLE",
  "type": "markdown",
  "content": "经济学原理",
  "source_order": 2
}
```

- `id` 是稳定对象身份；内容编辑和阅读顺序重排不得改变它。
- `source_order` 继续表达可编辑的阅读顺序。
- 旧 `PageDocument v1` 保持可读取、可按原格式写回；其临时关联可使用 `source_order`，但工具必须提示其在重排后的不稳定性。
- 从 v1 升级时必须作为显式、可预览、创建新样本集版本的迁移操作完成；不得原地静默改变旧基准集。

### 7.2 布局标注文件

`annotations/page-NNNN.json` 使用独立 schema，区域与 block 通过链接建立多对多关联：

```json
{
  "schema_version": 1,
  "page_number": 1,
  "coordinate_system": "normalized_integer_1000",
  "regions": [
    {
      "id": "region-title",
      "kind": "text",
      "bbox": {"left": 140, "top": 690, "right": 690, "bottom": 770},
      "review_status": "approved"
    }
  ],
  "links": [
    {
      "region_id": "region-title",
      "block_id": "blk_01JEXAMPLE",
      "role": "primary"
    }
  ]
}
```

- `regions[]` 是预览图上的独立矩形，具有稳定 `id`、类型、bbox 和审核状态。
- `kind` 至少支持 `text`、`formula`、`table`、`visual`、`caption`；后续类型必须版本化扩展。
- 一个 block 可关联零到多个区域，例如跨栏段落或续表。
- 一个区域可关联零到多个 block，例如插图与图注组成的复合区域。
- `links[].role` v1 使用 `primary`；需要更细关系时，再以版本化枚举扩展，避免无定义的多对多关系。
- 所有布局 bbox 使用样本集声明的坐标格式，并经过第 6 节的统一归一化规则。

### 7.3 标注粒度

- `visual_region`：对象级 bbox，进入现有正式评分。
- `formula`、`html_table`：block 级可选审阅定位框。
- `markdown`：段落级可选审阅定位框；不在 v0.1 支持行级、词级或字符级人工框选。
- 竖排、双栏、跨栏、续表或被图打断的段落允许关联多个区域。

## 8. 标注与比较界面

主界面采用三栏工作台：左栏为样本集和页面列表，中间为预览图与 bbox 画布，右栏为真值、预测和差异编辑区；底部展示当前页评分、报告新鲜度和保存状态。

### 8.1 画布

- 支持缩放、平移、适配页面、框选和键盘微调。
- 真值 bbox、预测 bbox、已匹配 bbox 具有稳定且可区分的颜色与图例。
- 选中某个框时，同步高亮对应 block、匹配对象、IoU 和相关文本内容。
- 视觉区域 bbox 是 v0.1 正式评分对象。
- Markdown、公式和表格 block 的可选审阅定位框按第 7 节保存、关联和显示，仅帮助人工核对。

### 8.2 结构编辑器

- 可编辑 `markdown`、`formula`、`html_table`、`visual_region` block 的内容与顺序。
- 可编辑页面 `categories`、`warnings`、审核状态与审阅备注。
- 提供 JSON 原文高级编辑；保存前执行与表单编辑相同的完整校验。
- 对公式提供 LaTeX 预览，对表格提供 HTML 预览和源码对照。

## 9. 评分与诊断

### 9.1 评分定义

工作台必须说明而非只显示分数：

| 指标 | 含义 |
| --- | --- |
| `text_ned` | 真值与预测正文的归一化编辑距离，越低越好。 |
| `formula_completeness` | 真值公式在预测中得到匹配的比例。 |
| `table_teds` | 表格结构质量指标；若现有脚本使用近似匹配，UI 必须如实标注。 |
| `reading_order` | 预测与真值阅读顺序的一致程度。 |
| `bbox_precision` / `bbox_recall` | 基于一对一 IoU 匹配的视觉区域误检/漏检情况。 |
| `duplicate_rate` | 预测视觉区域的重复比例。 |
| `truncated` / `blank_or_failed` | 截断、空白或失败的运行状态。 |

没有相应真值的公式、表格或 bbox 指标必须显示为 `N/A` 并给出样本数，不得显示为满分。

### 9.2 可解释输出

v0.1 至少提供：

- bbox 的匹配对、IoU、漏检、误检和重复框。
- 文本块或页面文本的差异视图。
- 自动归因摘要，例如“2 个漏检视觉区域”“bbox IoU 低于阈值”“预测已截断”。
- 按风险排序的页面列表，优先暴露低分、失败、截断和未审核页面。

评分使用的 IoU 阈值、脚本版本、样本集版本、profile、DPI 和运行时间必须随报告显示。评分阈值必须由受版本控制的配置声明，不得隐藏在 UI 中。

### 9.3 重跑评分

修改真值后，工作台只允许显式调用固定工作区中的评分器，对已存在的预测目录重算评分；不得自动启动 OCR 模型推理。重跑后生成新报告，保留旧报告的可追溯性。

## 10. 技术与交付边界

- 桌面壳建议采用 Tauri 2；Rust 后端复用 `ocrx-core`、`ocrx-pdf` 的领域校验和渲染相关能力。
- 图像画布与富文本/差异编辑可以采用 Web 前端技术，但业务规则、路径白名单、JSON 校验、评分调用和原子写入必须位于 Rust 后端。
- 首发验收环境为 macOS 14+ Apple Silicon；架构与打包保留 Windows 扩展能力。
- 工具默认离线工作，不上传样本、预览、真值、预测或报告。

## 11. 分期

### P1：只读诊断

固定目录发现、样本/页面浏览、预览叠图、真值与预测读取、筛选、评分报告展示、bbox 对象级解释、文本 diff。

### P2：真值编辑

bbox 与 block 编辑、校验、原子保存、审核状态、修改理由、历史、撤销/重做、报告过期标记与手动重跑评分。

### P3：运行工作台

在确认资源调度、运行隔离和可复现记录要求后，再从 UI 启动 profile × DPI 的 OCR 运行；本期不属于 v0.1 承诺。

## 12. 实施前仍待冻结的决策

以下事项不阻塞 P1，但会影响 P2 的 schema 和交互，实施 P2 前必须写入版本化契约：

1. 修改理由是否强制填写，以及是否接入项目身份或 Git 提交信息。
2. 评分器的正式 bbox IoU 阈值与表格指标实现；需求、脚本和报告必须一致。
3. `coordinate_system` 的最终 manifest schema 版本及 legacy 样本的默认声明。
