# RBAC 契约 v1：区域—块对应与拼接契约（Region-Block Association Contract）

**状态：** v1 冻结（2026-11-15，next-phase 方案 BLK-1/D-28）
**冻结先于机制：** 本文档是 B1 裁剪重读机制实现的第一交付物；实现（`src/ocrx-scan/src/reread.rs`）与本契约一一对应，五类形态各有单测锁定。B2/C1 探针与 C3 产品使用**同一规则版本**（v1）。
**适用范围：** M4（表格区域重读）、M5（公式区域重读）、M6b（阶梯耗尽坏页 text-only 区域管线）。
**输入前提：** 区域来源 = 版面检测模型对渲染位图的输出（B0'，与 OCR 输出解耦）；输出块 = OCR 管线产物 `PageDocument.blocks`（`PageBlock`）。输出块无坐标——一切对应关系按**类别序数**建立，几何仅用于去重。

---

## 1. 区域规范化（对应的前置）

1. **类内序数**：检测区域按页内同类检测顺序编号（`index_in_class`，0 基）。检测输出按检测器阅读序给出；同一类别的第 k 个区域记 R_class[k]。
2. **重复去重（形态④前置）**：同类检测区域两两 IoU ≥ 0.9 时去重，**取序在前者**（`index_in_class` 小者保留；被去重区域不参与配对，后续区域序数**保持原编号**——去重是掩码，不是重编号）。
3. **类别映射**：检测 `table` 区域 ↔ 输出 `PageBlock::HtmlTable` 块；检测 `equation_isolated`/`equation_semantic` 区域（合并为一个公式类）↔ 输出 `PageBlock::Formula` 块；检测 `text` 区域 ↔ 输出 `PageBlock::Markdown` 块（仅 M6b 使用）。

## 2. 五类形态的确定性处置（冻结）

### ① 数量一致 —— M4/M5：原位替换，不重组页面

页内第 k 个检测 table 区域 R_table[k] ↔ 输出第 k 个 table 块 B_table[k]（公式同理）。替换时**文档顺序不动**：B_table[k] 的位置原位保留，仅内容可被重读结果替换。官方类别序数配对键（第 k 张表）因此不变。

### ② M6b：检测阅读序为组装权威

text 区域重读结果按检测序拼接（每个 text 区域产出一个 Markdown 块，顺序 = 区域序）；table/formula 块以**"保留块连续段"插入**：原页输出中的 table/formula 块按其相对顺序构成连续段，插入锚 = 页内**首个该类检测区域**在 text 拼接序列中的位置；页内无该类检测区域时，保留段在文末追加。

### ③ 数量不一致 —— 双向

- **detection_N > output_N（未配对区域 = miss 病例）**：未配对区域**不产出新块**。缺表救援不在本方案范围（开放项 O-5）。两类分支（M4/M5 与 M6b）一致。
- **detection_N < output_N（未配对输出块 = 检测漏检）**：
  - **仅 M6b**：保留块追加在配对块之后（内容永不丢弃）。
  - **M4/M5**：未配对输出块**原位保留**（① 的文档顺序不动优先；不追加不移动）。

### ④ 重复 —— 同类检测区域 IoU ≥ 0.9 去重（取序在前者）

一个输出块被两个区域配对不可能发生：区域先去重，再按序数配对；若上游仍给出重复区域（罕见），第二个区域按 miss 处理（不产新块、不替换）。

### ⑤ 乱序 —— 序数配对天然吸收

检测序为最终文档序：M4 原位替换不改序（①）；M6b 保留段保持对照相对序（②）。输出块在页面文档中的位置从不因配对而移动（M4/M5），或按检测序重组且保留段内部相对序不变（M6b）。

---

## 3. 与官方评分的连接（不变式）

- **M4/M5（正常页）**：文档顺序不动 → 官方类别序数配对键不动（665=665 先例；第 k 张表仍是第 k 张表）。替换只改第 k 张表的内容。
- **M6b（阶梯耗尽坏页）**：table/formula 块逐字节保留（M4/M5 为其所有者）→ 表格/公式官方样本按构造不回归；text 按检测序重排（文本 edit 按页计，不依赖块序号键）。

## 4. 触发与比较（v1 冻结值，D-29；机制侧配套）

**M4 疑弱触发（表格 HTML 块满足任一）**：
- F1 行列不齐：各行单元格数不一致；
- F2 空单元格率 ≥ 0.3；
- F3 结构属性缺失（无 th/rowspan/colspan）且检测区域面积 ≥ 全页 10%；
- F4 相同行重复 ≥ 3；
- F5 HTML 不可解析为表格。

**比较保留函数（四元组字典序，平局保留原表）**：原表与重读表各解析为（可解析, 结构属性数, 单元格填充率, 行×列数），重读**严格更优**才替换，平局保留原表。

**M5 正常页公式触发（满足任一）**：
- G1 LaTeX 括号/环境不配对；
- G2 含替换字形（□/�/？）；
- G3 长度 < 8 字符且检测区域面积 ≥ 全页 5%；
- G4 math 模式内含 CJK。

**恒触发例外**：坏页（attempts ≥ 2 路径）进入 M4/M5 重读时**恒触发**（不查 F/G 特征）；正常页按 F/G 特征触发，且默认 off（`--table-reread`/`--formula-reread` 显式开启）。

**校准政策（D-29）**：以上为 v1 冻结值；B2/B3 裁决允许**至多一轮**校准（新值写入修订记录并全量复跑），评分过程中不得调整定义。

---

## 5. 验收映射

| 契约条款 | 单测（`src/ocrx-scan/src/reread.rs`） |
|---|---|
| ① 序数配对 + 原位替换 | `ordinal_pairs_same_class_blocks_in_document_order` |
| ③ miss 不产新块 | `unpaired_regions_produce_no_new_blocks` |
| ③ M4/M5 漏检原位保留 | `unpaired_blocks_keep_position_m4m5` |
| ③ M6b 漏检保留追加 | `unpaired_blocks_append_after_paired_m6b` |
| ② M6b 检测序拼接 + 保留段插入 | `m6b_splices_text_by_detection_order_with_kept_segments` |
| ② 无该类检测区域文末追加 | `m6b_appends_kept_segment_when_no_region_of_class` |
| ④ IoU ≥ 0.9 去重 | `duplicate_regions_dedup_by_iou_first_wins` |
| ⑤ 乱序吸收 | `detection_order_absorbs_shuffled_regions` |
| F1–F5 | `table_trigger_features_f1..f5` |
| G1–G4 | `formula_trigger_features_g1..g4` |
| 比较保留字典序 + 平局 | `compare_keep_lexicographic_tie_keeps_original` |

## 6. 修订记录

- **v1（2026-11-15）**：初版冻结（方案 v1.3 BLK-1/RBLK-1 闭环条款全量落档：③ 拆分 M4/M5 原位保留与 M6b 追加；B3 保护门回归事件 ΔCDM < −0.1 与 B2 同构）。
