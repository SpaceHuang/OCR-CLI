# P4.4 多页合批编排与门禁报告（2026-11-02）

**方案：** `docs/develop/scan-throughput-optimization-development-plan.md` §7.3 P4.4 / §7.4
**设计：** `docs/develop/batched-inference-2026-11-02.md`（P4.2 冻结约束）
**分支：** `feature/scan-throughput-optimization`（P4.1/P4.1.5/P4.2 之后）

## 1. 实施内容

| 层 | 内容 |
|---|---|
| ocrx-runtime（P4.3） | actor 消息协议新增 `GenerateWindow`/`WindowGenerated`；worker 多槽位解码：逐序列 `eval_chunks(seq_id)`（种子在逐页 encode 后立即采样）、共享 `LlamaBatch` 逐步解码 + `get_logits_ith`、逐序列 `RepetitionLoopDetector`（K=256，复读方案 D2 随迁实例化）、逐序列 EOS/终止原因/心跳（`Progress` 消息带页号）、取消即全窗 "cancelled"、批级错误统一 `BATCH_ERROR_PREFIX` 前缀、窗口上下文按预算建宽且显式 `with_n_seq_max`（Spike B 约束）、逐序列 `kv_cache_seq_rm` 回收。`Runtime` trait 新增可选 `generate_window`（默认 = 逐页串行，FakeRuntime 零改动） |
| ocrx-ovisocr2 | `write_window_artifacts_with_status_cancelled`：窗口识别 + 与单页完全相同的适配管道（提取 `adapt_and_write` 共享体，产物不分叉） |
| ocrx-output（P4.4） | `PageArtifactRecognizer::recognize_window_cancelled`（默认串行）；`ScanService` 页循环重构为窗口流水线：窗口组装（含 resume 逐页跳过排水）→ attempt-1 批量渲染+识别 → 终态归并（批级错误→整窗深度 1 回退重跑；退化页→单流 attempts 2/3 扰动重试状态机）→ **机制 A 窗口原子提交**（提取 `commit_terminal_pages`，取消即零提交）；批深入 checkpoint 兼容键（`;batch=N`，跨批深不互恢复） |
| 脚本 | `windows-ctrl-c-smoke.ps1` 适配窗口原子契约（见 §3） |

## 2. 正确性门禁（N=4 批深实测，Vulkan，金集 30 页两遍）

| 门禁 | 结果 |
|---|---|
| 完成/失败 | 30/30、0 失败/截断/退化（两遍一致，token 数 13343 一致） |
| **自一致性**（同输入同批深两遍逐字节互比） | ✅ **IDENTICAL**（除 result.json/gpu-resource.json 计时字段外全部产物逐字节一致） |
| **结构化相似度 vs 单流基线**（§2.3 第二档，阈值 ≥0.999） | ✅ `document.md` 1.0000；逐页 min 1.0000 / mean 1.0000（n=30） |
| 批级故障→深度 1 回退 | ✅ 单测 `batch_level_failure_falls_back_to_single_stream_and_commits_all_pages`（fake 批级失败 → 串行重跑 3 页全部提交） |
| 窗口协议 | ✅ 协议单测（progress 页号转发/结果序/poison）+ 串行/重试状态机既有 66 项 ocrx-output 测试全绿 |

## 3. 取消与 resume（机制 A）

`windows-ctrl-c-smoke.ps1` 适配窗口原子粒度：fixture 换 `complex-20-pages.pdf`、选择 1-8 页（两个满窗）；锚定**窗口 1（页 1-4）原子提交后**发真实 CTRL_BREAK；断言 130、页 4 checkpoint 保留、**页 5-8 零 checkpoint（机制 A 证据）**、无 `.part`、`--resume` 完成全部 8 页且页 5 checkpoint 出现（整窗重扫验证）。执行结果：✅（`ctrl-c smoke ok`）。

## 4. 批深-吞吐集成曲线与默认值冻结（决定性数据）

同一构建、金集 30 页、reference、本机 4090 D（两遍中位）：

| 后端 / 模式 | 墙钟 | 聚合解码 tok/s | 生成 token |
|---|---:|---:|---:|
| Vulkan 单流（P0 基线） | 54.9–55.8 s | 283 | 12 839 |
| Vulkan 批深 4 | 61.6 / 65.6 s | 279 | 13 343 |
| CUDA 单流（T1.4） | 52.4 s | 305 | 12 840 |
| CUDA 批深 4 | 56.7 / 58.3 s | 306 | 13 343 |

**结论：集成批深 4 无收益且回退 5–11%**——批内步耗时 ≈ 单步 ×4（两后端一致）。结构性解释：qwen35 混合注意力（24 层中 18 层为逐序列 SSM/线性状态）不随 batch 并行，理论收益上限 ≈24/18≈1.33×；隔离 bench（R0.6，4.76×@B=8）未在集成管线兑现。跨后端批跑 token 数完全一致（13343）且文本产物与单流相似度 1.0——合批**正确性**成立，**收益**不成立。

**冻结（P4.4 决定性数据）：`WINDOW_DEPTH = 1`（等价单流）**——按 §3 规则与 §7.4 回滚路径执行；窗口协议/编排/原子提交/回退/门禁全部保留（单测覆盖，常量 >1 即整体激活），N=4 批跑的门禁数据（自一致性/相似度/取消原子性）同时证明该路径在启用时的正确性。最终 N=1 金集复验见 §5。

## 5. N=1 最终门禁

- 金集（reference，1 遍）：exit 0、30/30、产物与 P0 逐字节参照**逐字节一致**（第一档复验）；`p44-final-golden`。
- Ctrl+C 冒烟：✅（§3 同一脚本，N=1 下锚点语义退化为串行取消，契约断言不变）。
- `cargo fmt`/`clippy --workspace --all-targets` 零警告、`cargo test --workspace` 356 通过 0 失败。

## 6. 资源与显存

- 100 页资源回归（`ocrx-pdf` 既有测试）在窗口重构下无变化（渲染路径未动）；窗口内存叠加（N 页 RGB ×2880²≈25 MB/页）随批深线性、当前 N=1 不新增常驻。
- 6 GB 预算断言在 N=1 下即 P4.1 形态（单流上下文 20480 + 权重 1.7 GB，实测 VRAM 峰值 ≈5.1–5.5 GB 已在 T1.4/P0 记录）；N=4 批跑实测亦正常完成（窗口上下文 ≈70k token KV，未触发 OOM）。

## 7. 数据

`docs/test/reports/perf-baseline-2026-11-01-data/p44-golden-batched*.json`、`p44-cuda-batched*.json`、`p44-final-golden*.json`；批深设计决定详见 P4.2 文档 §④。
