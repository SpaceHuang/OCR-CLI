# Q3/Q4/Q1.4 验收证据 · 金集门禁 + 语料对账（2026-11-07）

**分支：** `feature/quality-optimization-tdd`　**新二进制：** feature 分支 release（Vulkan，pins 0.1.152/0.1.153 不变）　**参照二进制：** 基线 `f3b6e068`（`C:/Users/WIN11/.local/bin/ocrx`）
**方法：** 同机同参各自扫描，产物逐字节比对（改动前后隔离）

## 1. 金集 30 页零差异门禁（§5 主门禁）——PASS

`assets/regression/ovisocr2-draft-v1`，balanced 档，300dpi：

| 比对集 | 结果 |
|---|---|
| 30 × page-0001..0030.json / .md / .routing.json + document.md（91 文件） | **91/91 逐字节一致** |
| result.json | 差异仅：`elapsed_ms`、输出路径字段、**新增 `pages.think_blocks_stripped_total: 0`**（Q3 声明的 schema 3 键，金集无泄漏 → 0） |

结论：Q3/Q4/Q1.4 三个改动在无 think、无坏链接、正常重试路径的金集上零行为变化。

## 2. Q3 语料级对账（验收①②）——PASS

新二进制重扫 213 页退化语料（`scans/degeneration-v0.1-newbin`）：

- 账本守恒：**213/213 退化页集合与基线一致**（attempt 分布不变，退化行为未受三改动影响）。
- 语料含 think 页 18 页：**18/18 新产物 page md 与导出脚本剥离结果逐字节一致**（产品/导出双实现对账），新产物零 `<think>` 残留。
- 警告/计数语义符合设计：18 页全部为**空块**泄漏（`<think>\n\n</think>`）→ 静默计数（`think_blocks_stripped` 各 1，checkpoint additive 字段总计 18），**不置 `think_removed` 警告**（空块静默是 Q3.1 的显式设计；非空块才置警告，金集用例覆盖）。
- 遗留（Q7.2 里程碑覆盖）：311 个非语料正常页的全集对账随全量 balanced 复评一并验证。

## 3. Q4 验收——PASS

基线 `recognition_failed` 页（`newspaper_TheBostonGlobe-2025-1-8@magazinesclubnew_page_011`）新二进制单独扫描：

- `complete=true`（原整页失败 → 正常提交）；
- warnings 含 **`invalid_image_link_dropped`**，输出正文 10,130 字符，坏链接（`../../`）未出现在产物；
- 该页同时标记 degenerated（复读）为其模型行为，与链接降级无关。

path-traversal 安全语义：剔除而非放行（单测 + 本页实证）。

## 4. Q1.4 行为不变证据

默认阶梯表 = 基线两档比例（47/50、53/50）：语料复扫 attempt 分布与基线一致（213 页全部 attempts=3），渲染调用序列由表驱动测试锁定（`(1,1,1) → 47/50 → 53/50`）。

## 5. 附带发现：0.1.161 gold-set 漂移（Q1.3 输入，scratch 分支记录）

`scratch/q12-sampling-escape` 的 0.1.161 二进制对同一金集：**38/91 文件内容级漂移**（llama.cpp 版本升级改变解码），样本页出现新复读伪影（"经经"）。按 D-13 规则 3：Q1.3 落地**必须**追加 `;decode=<gen>` 代次键 + 新旧不混扫回归；且 Q1.2 GO 需权衡 0.1.161 普通路径质量漂移（见 scratch 分支 `docs/develop/q12-sampling-spike-build-record-2026-11-07.md`，已更新）。
