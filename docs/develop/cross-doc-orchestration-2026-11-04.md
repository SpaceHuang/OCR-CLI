# Phase-2 X1 报告：跨文档编排（2026-11-04）

**方案：** `docs/plan/scan-throughput-phase2-development-plan.md` §4 阶段 X1
**设计冻结：** `docs/develop/cross-doc-orchestration-design-2026-11-04.md`（X1.0 前置评审门禁文档，PDFium 协议五点冻结后实施）
**二进制：** `feature/scan-throughput-phase2` @ orchestration+dispatch 修复，Vulkan release

## 1. 实施（TDD 序，全部单测先行）

| 项 | 内容 | 提交 |
|---|---|---|
| X1.0 | 半页设计 + BLK-2 协议冻结（本文档前置） | 设计文档先行提交 |
| X1.1 | `parse_scan_args` 多输入位置参数（选项可交错）、`--output-dir` 多输入必填、`resolve_scan_inputs` 目录展开（排序 `*.pdf`）/canonical 去重/缺失即 exit 2；单输入路径零改动 | X1.1 提交 |
| 协议 | `PdfRequest::SwitchDocument`：窗口调度边界切换 executor 输入；owner 清空预取缓存并重置 `next_prefetch`；文档级错误 = Err **值**（skip），owner 超时/断连维持 fail-loud 中毒 | 同上 |
| 运行时共享 | `ocrx-output::ScanService::run_pages_with_model`（外部持有模型的窗口循环入口，等价性单测锁定）；CLI 侧 `ModelSlot::Shared` 槽——首个需 OCR 文档填充（resolve+load 一次），后续文档复用同一 actor | X1.2 提交 |
| X1.2 | `run_scan` → `scan_document`（返回退出码不退出）；单输入路径保持历史形态（spawn 点/输出解析/PerCall 加载/`run_with_checkpoints_heartbeat`）；`run_scan_multi` 编排：唯一 owner、文档 FIFO、机制 A 逐文档、取消=整调用(130)、退出码聚合（130 > 文档级失败 3 > 部分 10 > 0） | 同上 |
| X1.3 | `<output-root>/<doc-stem>-<hash8>/`（canonical path SHA-256 前 8 hex，同 stem 不碰撞）+ `scan-summary.json`（schema_version/mode/inputs/output_root/documents[{input,output_dir,status,exit_code}]/elapsed_ms） | 同上 |
| 路由修复 | 单个目录参数 → 展开后多文档须走编排器（dispatch 以"位置参数含目录或 >1 个"判定；纯单文件路径不触碰输入解析，缺失文件仍为页数失败 exit 3） | 跟进提交 |

## 2. 门禁结果（真实模型，Vulkan，reference 档）

### ① 内容等价（逐字节集合口径 = `document.md` + 非计时 `page-*`）— **PASS**

两文档编排扫描 vs 各自单独扫描：`document.md` 逐字节一致、全部 figure PNG 逐字节一致（`x14` 实测记录）。窗口按文档独立组成，确定性成立。

### ② 单输入兼容 — **PASS**

- 金集单文档复测：`document.md` 与 depth-8 冻结输出**逐字节一致**（编排器提交后构建）。
- 全部 97 项 CLI 单测 + 7 项 fault-injection 子进程测试经重构后原样通过（含退出码契约）。

### ③ 安全验收（BLK-2 四项断言）— **PASS**

| 断言 | 结果 |
|---|---|
| 同一 owner 线程 | 单测 `every_switch_runs_on_the_same_single_owner_thread`：5 次跨文档切换全部记录在同一线程 id |
| 第二文档零串预取 | 单测 `switch_document_clears_prefetch_and_serves_the_new_document`：切换后首渲染必为 fresh executor 调用（文档标记像素断言），预取窗口重置 |
| 不可读文档跳过且后续继续 | 单测（switch Err 值不中毒 + 通道存活）+ 实测：[好, 坏, 好] 编排扫描 → 好文档产物齐全、坏文档 status=skipped/failed(3)、聚合 exit 3 |
| owner 超时 fail-loud | 既有 `request_timeout_poisons_the_owner_fail_loud` + 编排器 switch 路径 poison → 整调用 exit 3、无第二 owner（结构上不可能：唯一 `spawn` 在编排器入口） |

### ④ 场景收益 — **未达计划线（1.35× < 1.5×），数据如实归档**

| 场景 | 逐个调用基线 | 编排单调用 | 加速 |
|---|---:|---:|---:|
| `md-small-20`（20×2页） | 154.3–155.8 s | **114.2 s** | **1.35×** |
| `md-mixed`（8 份 1/2/4/8 页） | 66.0 s | **50.9 s** | **1.30×** |

**未达标归因**（T0.1/T0.3 数据链）：
1. 固定税摊薄**完全兑现**：每文档节省实测 2.1 s ≈ verify（reference 1.52 GB sha256 ≈1.4 s）+ load/backend init（≈0.7 s）——X1 本身的杠杆按预期工作。
2. 但固定税占单文档基线仅 **27%**（2.1/7.7 s），方案假设 36%（以 2.7 s 计）→ 1.5× 需要固定税占比 ≥33%。
3. 每文档剩余 ≈5.6 s 为**两种模式共担的固有成本**：图像页 300 DPI 渲染 + 全页 PNG 编码（commit 路径，≈4 s/2页文档）+ 窗口解码（≈1 s，STEP_PROFILE 实测 window_total 0.84–0.97 s）+ 提交/收尾。
4. 结论：该 fixture 类的编排收益上界由"固定税/总成本"比例决定；提高收益需动 X1 冻结范围之外的渲染/PNG 路径（render-PNG 与解码重叠——机制 A 提交时序改动，属独立提案）。

**DoD 对照（方案 §7）**："md-small-20 基线 + X1 收益归档" ✅ 已归档；"md-small-20 ≥1.5×"（X1.4④）❌ 1.35×，按"未达则逐项判定报告说明原因"条款处理。

## 3. 机制 A 与 resume 语义

- 窗口组成/原子提交/checkpoint 兼容键（`;batch=8` + `;platform=` + 输入 sha256）/`--resume`/`--overwrite` 全部**逐文档独立**，编排层只做排队——单文档语义零改动（② 的字节级证据）。
- 取消 = 整调用：共享 CancellationToken，在飞窗口零提交；已提交文档保留（机制 A 自然成立）。

## 4. 已知边界

- `--password` 为调用级（所有文档共用），按设计冻结稿记录。
- 目录展开不递归；`scan dir-with-one-pdf -o root` 报 exit 2（要求直接给文件）。
- 编排模式下 `scan-summary.json` 写失败降级为 warning，不丢弃已完成扫描。
