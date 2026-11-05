# S0.1 版本审计 + 可实施性认定（phase-3 backend sampling spike）

**日期**：2026-11-05
**计划依据**：`docs/develop/scan-throughput-phase3-development-plan.md` v2.1 §4 S0.1
**审计对象**：`llama-cpp-2`（wrapper）≥0.1.153 各版本的安全 API 覆盖面；对照 workspace 现行 pins（wrapper `=0.1.152` / sys `=0.1.153`，`src/ocrx-runtime/Cargo.toml`，regular + Windows target 双段，B1 守卫测试 `windows_target_declares_pinned_llama_bindings_with_mtmd` 约束）。

## 认定结论

**可实施性 = 通过（有指名候选）**：`llama-cpp-2 0.1.161`（2026-11-03 发布）+ `llama-cpp-sys-2 0.1.161`（wrapper 硬依赖同版本，`Cargo.toml [dependencies.llama-cpp-sys-2] version = "0.1.161"`）。

四个必要覆盖面逐项核对（全部基于实读 crate 源码，非符号存在性推断）：

| 必要覆盖面 | 0.1.161 实现 | 判定 |
|---|---|---|
| sampler chain 创建/attach | `LlamaModel::new_context_with_samplers(backend, params, [(seq_id, LlamaSampler)])`（`src/model.rs`）：构造 `llama_sampler_seq_config` 数组填入 `context_params.samplers`/`n_samplers` —— 即 `llama.h` 注记的**推荐初始化路径**（"prefer initializing the context with llama_context_params.samplers"），而非实验性的事后 `llama_set_sampler`。`LlamaSampler::greedy()` 存在（`llama_sampler_init_greedy` 封装），文档明确 greedy 属于 backend-execution 支持列表（greedy/dist/temp/top_k/top_p/min_p/logit_bias） | ✅ |
| sampled-token 读取（含 `LLAMA_TOKEN_NULL`） | `LlamaContext::sampled_token_ith(i) -> Option<LlamaToken>`（`src/context.rs:431`）：显式处理 `LLAMA_TOKEN_NULL`(-1) → `None`，doc 注明仅可用于以 `llama_sampler_seq_config` 创建的 context | ✅ |
| 错误返回（`llama_set_sampler` 返回 `bool`） | 该问题在推荐路径下**不存在**：params 路径不调用 `llama_set_sampler`（该符号在 wrapper 中未暴露，也无需暴露）；context 创建失败经 `LlamaContextLoadError::NullReturn` 标准错误面 | ✅（绕开） |
| sampler 生命周期 | sampler 所有权转移至 context（`_backend_samplers: Vec<(i32, LlamaSampler)>` 字段，"kept alive for the context's lifetime"），无悬挂风险 | ✅ |

附加事实：

- 0.1.161 的 `sampler` cargo feature 为**空门控**（`sampler = []`，src 内无 `cfg(feature = "sampler")`），上述 API 默认可用，无需额外 feature。
- **C API 侧无需新能力**：现行 sys 0.1.153 的 `llama.h` 已含全部所需符号——`llama_set_sampler`（返回 `bool`，[EXPERIMENTAL]）、`llama_get_sampled_token_ith`（"Returns LLAMA_TOKEN_NULL if no token was sampled"）、`llama_get_sampled_logits_ith`、`llama_sampler_seq_config` 结构体与 `llama_context_params.samplers`/`n_samplers` 字段（`llama.h:329/389/1039/1283`）。升级 sys 0.1.153→0.1.161 仅更新捆绑的 llama.cpp 版本，不引入 C API 缺口。
- **wrapper 版本跨度**：0.1.152→0.1.161 共 9 个版本（0.1.153…0.1.161）；`sampler`-attach API 在 0.1.161 才成型（`sampler` feature 于 0.1.161 新增），更早版本无此封装 → 认定唯一候选 0.1.161。
- **runtime 调用面兼容初查**：本项目 `ocrx-runtime`（`llama_mtmd.rs`）使用的 wrapper API——`LlamaBatch::new(n_tokens, n_seq_max)`/`add(token, pos, seq_ids, logits)`、`LlamaContext::decode(&mut batch)`/`get_logits_ith(i) -> &[f32]`、`MtmdContext::init_from_file`/`MtmdContextParams`/`MtmdInputText`/`mtmd_default_marker`——在 0.1.152 与 0.1.161 源码中签名一致。此为静态比对；**权威验证 = S0.2 scratch 全量构建**。
- **B1 守卫不受影响**：`windows_target_declares_pinned_llama_bindings_with_mtmd` 只断言两个 crate 在 Windows target 声明存在且带 `mtmd` feature、非 workspace 继承，不锁定版本号 → 升级 pins 无需修改守卫测试。

## vendor patch 子决策

**不需要**：已存在满足全部覆盖面的上游发布版本（0.1.161），无 patch 必要性。

## 对 S0.5 的输入

可实施性条件 ① 已满足（可指名候选 = 0.1.161，安全 API 覆盖面完整）。条件 ②（收益 ≥8% + divergence 面）由 S0.3/S0.4 实测裁决。

## 验证记录

- crates.io API 实查：`llama-cpp-2` 与 `llama-cpp-sys-2` 全版本列表（最新均 0.1.161，2026-11-03）。
- 0.1.161 crate 源码完整解包实读（`model.rs`/`context.rs`/`sampling.rs`/`mtmd.rs`/`Cargo.toml`）。
- 现行 sys 0.1.153 `llama.h` 本地实读（registry 源码缓存）。
- 0.1.152 ↔ 0.1.161 调用面静态比对（本项目实际使用的 API 子集）。
- 未运行构建（S0.2 范围）与真实模型（S0.3/S0.4 范围）。
