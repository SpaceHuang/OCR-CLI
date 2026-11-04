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

### ④ 场景收益 — **两轮实测，门禁以实现评审整改后数据为准（PASS）**

**第一轮（初版实现，已作废）**：md-small-20 = 114.2 s（1.35×）、md-mixed = 50.9 s（1.30×）——当时归因为"渲染+PNG 编码 ≈4 s/文档固有成本"。实现评审（`docs/review/scan-throughput-phase2-implementation-review.md` BLK-2）证伪了该归因：共享槽当时只复用了 load，**每份文档仍执行完整资产 SHA-256 校验**（reference 1.52 GB + mmproj，实测每文档 ≈4.4 s），被误算进了"渲染+PNG"。

**第二轮（BLK-2 整改后，vulkan reference，2026-11-05 重采）**：

| 场景 | 逐个调用基线 | 编排单调用 | 加速 | 门禁 ≥1.5× |
|---|---:|---:|---:|---|
| `md-small-20`（20×2页） | 154.3–155.8 s | **26.2 s**（26 154 ms） | **5.9×** | ✅ |
| `md-mixed`（8 份 1/2/4/8 页） | 66.0 s | **18.2 s** | **3.6×** | — |

- 内容产物抽查：doc-00 `document.md` 与整改前逐字节一致（校验短路不改变任何输出）。
- 整改后每文档摊薄后的真实工作 floor ≈1.3 s（渲染 + 窗口解码 ≈1.0 s + PNG 编码 + 提交收尾）——第一轮报告中"渲染+PNG ≈4 s"的推断错误由本数据修正。
- 校验+加载每调用恰好一次由 gated 测试 `model_is_resolved_and_verified_exactly_once_per_invocation` 锁定（`OCRX_STEP_PROFILE=1` 的 `[model] resolved and verified` stderr 计数 = 1）。

**DoD 对照（方案 §7）**："md-small-20 基线 + X1 收益归档" ✅；"md-small-20 ≥1.5×"（X1.4④）✅ 5.9×。

## 3. 机制 A 与 resume 语义

- 窗口组成/原子提交/checkpoint 兼容键（`;batch=8` + `;platform=` + 输入 sha256）/`--resume`/`--overwrite` 全部**逐文档独立**，编排层只做排队——单文档语义零改动（② 的字节级证据）。
- 取消 = 整调用：共享 CancellationToken，在飞窗口零提交；已提交文档保留（机制 A 自然成立）。

## 4. 已知边界

- `--password` 为调用级（所有文档共用），按设计冻结稿记录。
- 目录展开不递归；`scan dir-with-one-pdf -o root` 报 exit 2（要求直接给文件）。
- 编排模式下 `scan-summary.json` 写失败降级为 warning，不丢弃已完成扫描。
