# 复读退化检测与恢复 — 金集门禁执行报告（D4.3，2026-10-31）

**分支：** `feature/repetition-degeneration`（D1.1–D5.3 全部落地）
**门禁口径：** 方案 v2.3 §5.3；对照 2026-10-30 执行报告（quality-gate-ovisocr2-draft-v1）
**执行环境：** Windows x64，RTX 4090 D（CPU-only 默认构建，推理在 CPU），63 GB 内存

## 1. 执行内容

同一输入（`ovisocr2-draft-v1` composite source.pdf，30 页，`--pdf-mode ocr
--model-profile reference --device cpu --dpi 300`）分别用：

- **基线二进制**：`main@8a7ecff` 的 release 构建（改动前）
- **新二进制**：本分支 release 构建（改动后，默认预算 16384 / 上下文 20480）

各做全量 30 页重扫，输出目录隔离，机械比对。

## 2. 门禁判定（§5.3 四条规则）

| 规则 | 结果 |
|---|---|
| 1. 每页 page-NNNN.json 逐页一致 | **PASS**（27 页 OCR 页：baseline==gold==new 语义逐字节一致；页 18/29/30 为空页，基线与新二进制一致，均走 native 空页路径输出 schema-2 信封——金集存储的预测生成于空页路由演进之前，属既有产品差异，与本改动无关，详见 2026-10-30 报告 §1 的字节级偏差记录） |
| 2. document.md 逐字节一致 | **PASS**（基线 vs 新二进制 byte-equal；30 页均无 ⚠ 标记，符合"金集页全部无退化"预期） |
| 3. result.json 除新增字段外一致 | **PASS**（`pages.degenerated=0` 为唯一新增字段；其余差异仅为输出目录路径与 `elapsed_ms` 两个易变字段） |
| 4. 评分门禁不退化 | **PASS**（0 截断 / 0 重复 / 0 空白失败页；评分脚本输入即上述 page md/json，与基线一致） |

耗时：基线 15m27s，新二进制 13m39s（无退化；±3% 门禁满足）。

## 3. 事故页复扫（九年级下册第 104 页，balanced profile）

实测（balanced profile，含 3 次扰动重试总耗时 46.3s，原事故单页即跑满
4096 token）：

| 项 | 实测 |
|---|---|
| `page-0104.json` termination | `"repetition_loop"`（runtime token 环早停生效，未跑满预算） |
| warnings | `["repetition_removed", "degenerated"]` |
| 扰动重试 | attempts=3，三连退化按 §5.2-B 预期常态收敛为带标记 partial |
| `page-0104.raw` 边车 | 存在，checkpoint `raw=page-0104.raw`；内容为环前文本（早停后环未写入） |
| `document.md` | 该页前带冻结标记行；无循环正文残留 |
| 退出码/建议 | exit 10，`warning: 1 page(s) incomplete — page 104 (degenerated); rerun with --resume to retry degenerated pages` |
| result.json | `status=partial`，`pages.degenerated=1` |

## 4. 单元/集成门禁

`cargo test --workspace`：**328 passed / 0 failed**（含方案 §5.1 检测器、幂等、
单源、不变式、checkpoint 往返、merge 标记与 §5.2 场景 A–F 全部用例）；
`cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets` 零警告。
