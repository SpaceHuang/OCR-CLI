# P4.2 多页合批设计文档（评审门禁交付物）

**方案：** `docs/develop/scan-throughput-optimization-development-plan.md` §7.3（P4.2）
**日期：** 2026-11-02
**状态：** 待评审。本文按方案冻结的五项约束逐一落定设计值；评审通过是 P4.3/P4.4 实施的前置条件。
**输入：** P0 基准（`docs/test/reports/perf-baseline-2026-11-01.md`，R0.2/R0.3/R0.6）、P4.1.5 双 Spike 实测（§5）、复读退化方案冻结值（§8.3：默认输出预算 16384 + 上下文提升）。

## ① 上下文预算（冻结）

- 每页预算 = 图像 token（≤1024，`max_image_tokens` 默认）+ 提示（≈0.2k）+ 输出上限（默认 16384，§8.3 冻结值）≈ **17.6k token**。
- 约束式：**N × 每页预算 ≤ n_ctx(window) ≤ n_ctx_train = 262144**（GGUF revision `8a22290a` `qwen35.context_length`，实测见方案 §7.3①）。
- **实现规则**：窗口上下文的 n_ctx 在运行时按 `Σ(各页请求的图像 token + 提示 + max_output_tokens)` 计算并取上限整备，不做静态常量；单流路径沿用既有 `context_size`（默认 20480）不变。窗口与单流上下文各自独立创建、各自复用（`clear_kv_cache` / `kv_cache_seq_rm`），总 KV 占用见 §⑥ 预算断言。
- KV/SSM 元数据核算（方案 §7.3①，v3.6）：混合注意力（`full_attention_interval=4`，KV ≈12 KB/token）→ N=4 全预算 ≈70k token ≈ 0.86 GB；N=8 ≈141k ≈ 1.7 GB。**批深天花板（n_ctx_train 推出）= 14**；默认值见 §④。

## ② 确定性弱化与准入策略（冻结）

- batched GEMM 与单流 GEMV 归约顺序不同：同一页在批内/单流、不同批组合下输出可能分叉（§2.3 第二档）。
- **确定性定义**：相同（输入、批组合、批深）下逐字节可复现。
- **准入策略（确定性优先）**：固定批深（`WINDOW_DEPTH`，§④）、按页序填充窗口、**不做动态回填**（窗口不满 N 时按实际页数运行，不延迟凑批）；checkpoint 兼容键纳入批深（`;batch=N`，跨批深不互恢复）。
- **resume 提交原子性：机制 A（窗口原子提交）**——窗口内全部页到达终态（成功或失败）后才逐页提交 checkpoint；取消的在飞窗口零提交；断点丢失粒度从 1 页扩大到 1 窗（N 页）。方案 §7.3③ 的建议机制被采纳，机制 B 不实现。
- **自一致性门禁**：同一输入 + 同一批深连跑两遍，`page-*.json`/`document.md` 逐字节相同（进入 P4.4 门禁）。
- **金集验收切至 §2.3 第二档**：与单流基线（P0 报告冻结参照）做结构化相似度比对，阈值由 P4.1.5 Spike A 实测定线（§5/§④）。

## ③ 页级语义（冻结）

- 窗口内逐页：心跳（`seq_id` → 页号映射后转发既有 `(page, progress)` 回调）、取消（共享 `CancellationToken` 克隆进窗口请求，逐 token 检查点语义与单流一致）、EOS/`max_tokens`/环检测终止原因逐序列独立。
- **环检测逐序列实例化**：每序列独立 `RepetitionLoopDetector`（K=256 环形窗口、早停、`finish_reason: "repetition_loop"`），与复读退化方案 D2 的语义一致（§8.3）。
- 单页推理失败不拖垮同批：逐序列错误作为该页的 `Err` 值返回；批级故障（decode/OOM/上下文溢出）→ 窗口全部页返回同一批级错误，扫描侧触发**批深回退 1**（等价单流）重提该窗口，失败继续语义不变。
- 扰动重试（复读方案 attempt 1/2）：窗口完成后对退化页**单独**以批深 1 提交（扰动输入本就不同批），既有 attempts 状态机不变。
- 产物与 checkpoint 仍按页序提交（机制 A 的提交阶段按页序执行既有逐页落盘逻辑）。

## ④ 默认批深（冻结：**N = 4**；2026-11-02 评审修复后按修正数据终版冻结）

`默认 N = min(吞吐曲线饱和点, 6 GB 安全上限, 窗口代价可接受点)`：

| 依据 | 值 | 来源 |
|---|---|---|
| 集成实测（修复后，Vulkan） | **N=4 端到端 40.9–41.5 s vs 单流 55.5 s = 1.36×；解码聚合 417–430 tok/s vs 283 = 1.52×** | P4.4 门禁重跑（§5/P4.4 报告） |
| 隔离 bench（R0.6） | B=4 预测 3.21×；集成兑现 1.52×——混合注意力（18/24 层逐序列 SSM/线性状态）封顶实际可兑现收益 | `perf-baseline-2026-11-01.md` §8 |
| 6 GB 预算上限 | N=4 实际分配（llama.cpp 加载日志，Vulkan）：权重 1436 MiB + KV 828 MiB（窗口 70k ctx）+ 计算缓冲 ≈717 MiB ≈ **2.9 GB ≤ 6 GB** ✓ | P4.4 门禁 §6 |
| 窗口代价（断点粒度） | N=4 → 断点损失 ≈4 页 ×1.4 s ≈5.6 s（resume 整窗重扫） | 机制 A 语义 |
| 分叉实测 | N=4 集成金集：相似度 1.0000（30/30 页）、自一致性逐字节一致、零失败/截断/退化 | P4.4 门禁重跑 |

**冻结值 N=4（终版）**。冻结过程记录（供审计）：初版 N=4 → 第一轮集成实测回退 5–11% → 冻结 N=1；评审发现首轮 N=4 实测跑在坏管线上（窗口解码每步重喂种子 token，输出与计时均失真）→ 修复并经真实模型验证（窗口输出 = 单流输出逐字节一致）→ 修正数据支持 N=4 的真实收益（1.36×）→ **终版冻结 N=4**。教训入档：性能冻结数据必须在正确性验证通过的构建上采集。

4090 D 上更大 N（8–14）是否随显存探测自动提升：归后续决策，不新增用户旋钮（方案 §7.4）。

## ⑤ 遥测与接口面（冻结）

- `RuntimeTelemetry` 聚合口径不变（全窗口累加），`OCRX_GPU_REPORT=1` schema 兼容；批深经 checkpoint 兼容键与运行日志可见，不进 schema（第二档验收不依赖遥测分列）。
- `Runtime` trait 最小扩展：一个可选窗口方法（默认实现 = 逐页 `generate_with_progress` 串行，FakeRuntime 及全部既有实现零改动）；真实 actor 覆盖实现多槽位窗口。多请求能力收敛为 runtime 内部细节（§7.1 为后端可扩展性重构留出映射空间）。

## ⑥ 显存预算断言（P4.4 门禁）

以 llama.cpp 加载时 stderr 报告的实际分配量（权重 + KV + 计算缓冲）合计断言 **≤ 6 GB**（口径：8 GB VRAM 标准设备 − ~2 GB 系统预留），在发布后端（Vulkan）上以默认 N=4 实测；不依赖真实 6 GB 设备。断言执行与记录见 P4.4 门禁报告。

## 5. P4.1.5 Spike 实测（2026-11-02，CPU/reference/96-token cap/金集前 6 页/150 DPI）

载体：`src/ocrx-runtime/tests/multi_sequence_spike.rs`（env-gated）；原始记录 `p415-spike-data.json`。

**Spike A（批分叉定量）**：单流基线 vs 合批（N=2、N=4），12 个（页×批深）组合全部 **first_divergence_token = null、similarity = 1.0**（逐 token 零分叉）。混合注意力架构（qwen35）在本钉定版本上的共享 batch 解码与单流按位一致。

**Spike B（多序列组合验证）**：全部组合行为可行并经断言验证——逐序列 `eval_chunks(seq_id)`、共享 `LlamaBatch` 逐槽 logits（`get_logits_ith`）、逐序列 `kv_cache_seq_rm` 槽位回收。**关键发现（进入 P4.3 必遵守）**：`LlamaContextParams::default()` 的 `n_seq_max = 1`，llama_decode 拒绝 `seq_id ≥ n_seq_max`（`init: invalid seq_id = 1 >= 1`）——**窗口上下文必须显式 `with_n_seq_max(N)`**，否则批内第二页起全部失败。

**定线（§2.3 第二档阈值冻结）**：P4.4 金集结构化相似度门禁按页 **≥ 0.999**（实测分布下限 1.000，留浮点余量）；另加自一致性门禁（同输入同批深两遍逐字节相同）。

## 6. P4.3/P4.4 实施落点

- P4.3（runtime）：actor 消息循环多槽位——`GenerateWindow` 协议、逐序列 `eval_chunks`/`get_logits_ith`、共享 batch 逐 token 解码、逐序列环检测/EOS/终止原因、批级错误标记、窗口上下文按预算动态建宽、`kv_cache_seq_rm` 槽位回收。Spike B 已验证全部绑定层组合行为可行。
- P4.4（扫描侧）：`ocrx-output` 窗口循环（机制 A 原子提交）、兼容键 `;batch=N`、批级故障→批深回退 1、心跳页映射、取消零提交、100 页资源回归（预取×批深内存叠加）、预算断言、自一致性门禁。

## 7. 执行记录

- Spike 运行：`OCRX_E2E_MODEL=1 OCRX_PDFIUM_LIB_DIR=<vendor> cargo test -p ocrx-runtime --test multi_sequence_spike -- --ignored --nocapture`（2026-11-02，CPU/reference，42.9 s，6 基线页 + 2 组批深）。
- 结果：PASS——Spike A 零分叉（12/12 组合 similarity 1.0）；Spike B 组合可行 + `n_seq_max` 约束发现（§5）。
