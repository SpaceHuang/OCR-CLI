# OCR-CLI 扫描吞吐优化方案（基于 GPU 低利用率分析）

**文档状态：** 修订 v3.11——2026-10-31 重审 v3.10 UNBLOCK/PASS，MINOR 收尾：§6.4/§9 残留"降级透传"措辞统一改为"仅限代码 revert（编译期），无运行时回退"（v3.10 重审）；v3.10 重审 v3.9 BLOCK 后修订：P3 补扰动渲染转发协议（BLK-1）、移除主线程 PDFium 回退改 fail-loud（BLK-2）、T1.2 承接已验证构建环境（MAJOR-1）；同时确认复读分支已合并（`85cc1a3`），P0/P1/P4 解除排序等待（§8.3）；v3.9 T1.4 本机预研数据已采（CUDA/Vulkan 加速比 1.26–1.38×，§5.5）；v3.8 决策：默认批深以 6 GB 显存不 OOM 为冻结准则（§7.3 P4.4）；v3.7 Spike B（多序列解码组合验证）经确认保留为 P4.1.5 正式任务；v3.6 批深约束核算修正（KV ≈12 KB/token、VRAM 不构成约束，§7.3 ①）；v3.5 重审 UNBLOCK/PASS 后并入 3 项非阻断 MAJOR 整改（R0.5b 按实际 token 扣减、resume 提交原子性两机制、n_ctx_train 硬上限实测 262144）；v3.4 排序决策：复读退化方案先行（分支开发中）、本方案随后，预算与 P0 基线口径随之冻结（§8.3）；v3.3 P4 风险决策两项（质量阈值实测定线、resume 整窗重扫）+ 新增 P4.1.5 de-risk 实验（§7）；v3.2 实测核验 CUDA Toolkit 12.4 已在机、P2 阻塞解除（§1.3 修正 5）；v3.1 并入评审补充意见 2 项；v3 范围决策：P4（多页合批）提前为排期项、优先于后端可扩展性重构实施（§7 决策记录）；同日评审补充意见 2 项（R0.5 差分口径修正并新增 R0.5b、`llama_token_data` 12 字节更正）已并入；v2 为评审 BLOCK（`docs/review/scan-throughput-optimization-development-plan.md`，BLK-1 + 3 MAJOR + 2 MINOR）修订，全部意见已并入。修订记录见 §12  
**创建日期：** 2026-10-31（v1）/ 2026-10-31（v2 评审修订、v3 范围决策）  
**依据：** `docs/analyze/gpu-underutilization-analysis-2026-10-31.md`（诊断报告，本方案承接其 §5 改进路径建议）  
**基线数据：** `docs/develop/windows-gpu-baseline-2026-08-18.md`、`docs/test/reports/quality-gate-ovisocr2-draft-v1-2026-10-30.md` §6  
**关联计划：** `docs/develop/windows-platform-development-plan.md`（P2 即其 W4.3 执行）、`docs/develop/model-backend-extensibility-refactoring-development-plan.md`（后端可扩展性，v3 起排于 P4 之后，见 §7 决策记录）、`docs/develop/repetition-degeneration-detection-recovery-development-plan.md`（已在分支开发、先于本方案合并；解码循环与预算基线适配见 §8.3）  
**方案性质：** 本文独立给出目标、措施设计、TDD 任务、验收门禁与实施顺序；实施时不需要借助其他设计文档才能理解本方案。文中全部性能预期均为"待实测"假设，不作为验收承诺；验收只认 §3 冻结的基准协议实测值。

---

## 1. 背景与问题摘要

### 1.1 诊断结论（引自分析报告）

- RTX 4090 D 推理期间任务管理器 ≈5% 利用率是亚 1B 模型 batch=1 自回归解码的固有负载形态：每 token 需将全部权重从显存读取一遍（GEMV，算术强度 ≈2 FLOP/byte），瓶颈在显存带宽而非算力；微秒级 kernel 之间的空隙计入 GPU% 空闲（分析报告 §3.1）。**不是 offload 故障，不是回归**：GPU 加速比 9.6×（Vulkan 58.4 s vs CPU 562.6 s，30 页 reference）证明 offload 生效。
- 可兑现的提速余量四项（分析报告 §5）：CUDA 发布链路缺口（§3.4）、每 token CPU 同步点（§3.2）、渲染与解码串行（§3.3）、单请求架构（§5#4）。
- **KPI 口径修正：目标是吞吐（30 页集墙钟、tok/s），不是任务管理器利用率读数。** 单流解码下，除多页并发外的一切优化落地后，该读数仍会维持个位数（§3.1 物理形态不变）；P4 合批是该读数显著抬升的唯一路径，但读数本身仍不作为验收目标。

### 1.2 现状基线与代码热点

| 项 | 事实 | 出处 |
|---|---|---|
| 4090 D / Vulkan，30 页 reference | 58.4 s（≈2 s/页） | 质量门报告 §6 |
| 4090 D / CPU，同集 | 562.6 s | 同上 |
| RTX 2070 Max-Q / Vulkan，单页 | 76.2 tok/s | `windows-gpu-baseline-2026-08-18.md` |
| 解码循环每 token 形态 | `context.get_logits()` 返回**主机侧借入切片**（绑定零拷贝，`llama-cpp-2/src/context.rs:237-243`），其内部 `llama_get_logits` 每 token 执行 `ctx->synchronize()`（`llama-context.cpp:3563-3568`，主机侧采样路径的固有同步）；随后 Rust 全词表 argmax（`llama_mtmd.rs:226-231`）；每 token 新分配 `LlamaBatch`（`:269`） | 见 §1.3 修正 3 |
| 页级固定开销 | 每页新建 `LlamaContext`（KV cache 与计算缓冲重建，`llama_mtmd.rs:175-182`） | — |
| 渲染与推理 | 严格串行（渲染期间 GPU 空闲，解码期间渲染能力空闲） | `src/ocrx-output/src/lib.rs:607-663`（页循环内 render→recognize 相邻调用） |
| CUDA 发布链路 | feature 已声明、安装器已优先探测（`nvcuda.dll → cuda`），但打包脚本硬拒 cuda 工件（`scripts/package-windows-x64.ps1:111`），发布链路无任何一环能产出 CUDA 构建 | 分析报告 §3.4 事实链 1–6 |

### 1.3 事实修正（v1 已录 1、2；v2 新增 3、4）

实施前逐环核对代码、钉定绑定与内嵌 llama.cpp 源码，四处事实与分析报告或本方案 v1 表述不同，设计据此修正：

1. **默认渲染路径是进程内，不是 worker 进程。** 分析报告 §3.3 以"pdfium 渲染（CPU，独立 worker 进程）"描述流水线，但 `src/ocrx-cli/src/main.rs:2135-2166` 显示：仅显式设置 `OCRX_PDF_WORKER` 时才走 `WorkerScanRenderer`，**默认是进程内 `PdfScanRenderer`**；且发布归档不打包 `pdfium-worker.exe`（`package-windows-x64.ps1` 仅构建 `-p ocrx`），安装脚本也不设置该环境变量。渲染/解码串行的问题两种模式同样存在，但重叠设计必须以默认进程内路径为第一目标（§6）。
2. **多数绑定 API 已暴露，但暴露 ≠ 收益。** 钉死的 `llama-cpp-2 =0.1.152` 中，`LlamaBatch::clear()`（`llama_batch.rs:34`）、`LlamaContextParams::with_flash_attention_policy`（`context/params/get_set.rs:313`）、`clear_kv_cache()`（`context/kv_cache.rs:103`）、`LlamaSampler::greedy()`/`sample()`（`sampling.rs:30,595`）、多序列 `LlamaBatch::add_sequence`（`llama_batch.rs:113`）均存在，P1/P4 无版本升级诉求（与 WP.2 冻结不冲突）；但其中 sampler 路径经核实**不构成性能收益**（见修正 3），`clear_kv_cache` 的使用受 `LlamaContext` 生命周期约束（见修正 4）。
3. **"每 token 整表 DtoH 拷回"不成立（修正分析报告 §3.2 首行表述）。** 绑定层 `get_logits()` 返回 `llama_get_logits` 主机缓冲的借入切片，**无 Rust 侧拷贝**（`context.rs:237-243`）；logits 的设备→主机搬运发生在 llama.cpp 图执行输出处理内（`llama-context.cpp:1853-1855` 一带），且 `llama_get_logits*` 每 token 的 `ctx->synchronize()` 对任何主机侧采样路径（含 sampler chain）同样存在（`:3563-3579`）。更关键的是：`llama_sampler_sample` 对**独立**（非 chain）采样器使用每次调用的局部缓冲，并把全部词表逐项复制为 12 字节的 `llama_token_data`（int32 id + float logit + float p，`include/llama.h:207-211`）再做 greedy 扫描（`llama-sampler.cpp:806-872`，复制路径 `:823-855`）——**把 Rust argmax（对借入切片的纯扫描）换成 greedy 采样器不是消除热点，反而新增每 token 一次全词表分配+拷贝**。因此 v1 的 T2.1 已重写（§4），"复用候选缓冲的 chain 采样器"仅作为实测归因后的备选项保留。
4. **`LlamaContext<'a>` 借用 `LlamaModel`，跨调用持有 context 与现所有权结构冲突（评审 BLK-1）。** 绑定中 `LlamaContext<'a>` 持有 `model: &'a LlamaModel`（`context.rs:25-30`），`new_context<'a>(&'a self, …) -> LlamaContext<'a>` 让 context 借用创建它的 model（`model.rs:803-807`）。把它作为字段放进同时拥有 `LlamaModel` 的 `LlamaMtmdRuntime` 是安全 Rust 不能表达的自引用结构；workspace `unsafe_code = "forbid"` 排除生命周期擦除绕过。`clear_kv_cache()` 只解决 KV 状态清理，不解决所有权。分析报告 §5#3 的"复用 context"必须先重建所有权模型——T2.3/P4.1 给出推理线程所有权设计（§4），该模型同时是 P4 合批的执行基础（§7）。
5. **"本机缺 CUDA toolkit"的阻塞记录已过时（v3.2 实测核验）。** 分析报告 §3.4 事实链第 3 步与 W4.3 的阻塞依据（`windows-platform-development-plan.md` §0.2/§0.4，2026-08 记录"本机缺 CUDA toolkit ≥12.x"）不再成立：2026-10-31 实测本机已安装 **CUDA Toolkit 12.4**（`nvcc` V12.4.131，`CUDA_PATH` 指向 `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4`），显卡驱动 610.88 未变，满足 W4.3 的 ≥12.x 门槛。遗留验证点：本机 MSVC 为 17.14（cl 14.44），高于 nvcc 12.4 的官方支持范围（约至 VS 17.9），`--features cuda` 构建可能报 unsupported compiler——T1.0/T1.1 的首个验证项即此，处置预案见 §5。

---

## 2. 目标、非目标与行为基线

### 2.1 目标

1. 30 页门禁集（`ovisocr2-draft-v1`，reference 档，默认 300 DPI）墙钟时间相对 **P0 在复读方案合并后重测的基线**（58.4 s 为历史参照，§8.3）**分阶段可测量下降**；每阶段收益与归因按 §3 冻结的基准协议实测并落报告。
2. 闭合 CUDA 发布链路（分析 §3.4 = W4.3 执行）：toolkit 就绪后 `--features cuda` 构建 → 打包 → 安装 → E2E/质量对照全部可执行；通过则 CUDA 工件进入发布，未通过仅阻断 CUDA 自身（W4.3 既有纪律）。
3. 运行时层降低每 token / 每页固定开销（batch 复用、flash attention、会话所有权重构），对**全部后端（含 CPU）**生效；每项改动先实测归因、后决定去留（§3 收益判定规则）。
4. 渲染与解码重叠，隐藏渲染时段的 GPU 空闲；默认进程内路径与 worker 路径分别落地（§6）。
5. 多页合批（P4）按 §7 分解落地：解码吞吐随批深实测爬坡，最终默认批深以 §3 基准曲线冻结。
6. 全部阶段维持既有公共契约不变（§2.3；P2/P4 引致的质量口径调整见该节第二档）。

### 2.2 非目标

- 不以任务管理器 GPU% 读数为 KPI；多页并发（P4）已随 v3 决策进入排期（§7），其抬升利用率读数仅作副产物观察；跨文档、跨任务并发仍属非目标。
- 不改 OCR prompt、贪心解码策略（temperature=0）、质量评分器、真值集、模型量化档位。
- 不新增 CLI 参数或环境变量作为性能开关：性能优化为**默认行为**，决策不暴露旋钮；`OCRX_PDF_WORKER` 既有 opt-in 语义不变。
- 不升级 `llama-cpp-2`/`llama-cpp-sys-2`（WP.2 冻结；§1.3.2 已核对现版本能力）。
- 不引入 GPU 侧采样（llama.cpp 后端采样器路径 `llama-sampler.cpp:809-819` 存在但当前无已验证的可用接入，不作为本方案前提）。
- 不实现跨文档、多任务并发；P4 的合批仅限单 job 内页窗口，产物与 checkpoint 仍按页序提交。
- 不把 worker 模式设为默认、不打包 `pdfium-worker.exe`（发布足迹变更超出本方案，如需另行决策）。

### 2.3 行为基线（硬门禁，按后端身份分两档）

- **同后端、同解码路径优化（P1、P3、P4.1 actor 阶段）**：30 页金集的 `page-NNNN.json`、`document.md`、`result.json` 除性能字段（`elapsed_ms` 等计时类）外**逐字节不变**。
- **后端身份/解码数值路径变化（P2 CUDA、P4 合批阶段）**：输出与既有基线**不做逐字节要求**——跨后端浮点归约顺序差异是预期的（既有先例：Vulkan vs CPU 文本相似度 mean 0.9999 / min 0.9974，并非逐字节相等，质量门报告 §6）；P4 合批的 batched kernel 同样改变浮点归约路径，且批组合不同输出亦可能不同（§7 P4.2 确定性约束）。该档按结构化/质量相似度门禁验收；`result.json` 的 `backend` 字段在 P2 下**预期发生变化**（`Vulkan → Cuda`），不视为违反基线。
- 全阶段不变项：退出码表、`PageDocument::SCHEMA_VERSION=1`、`result.json` schema（后端字段值除外）、checkpoint v5、stdout/stderr 纪律、原子写、resume 语义。
- 与复读退化方案（独立轨道）的解码循环共存约束见 §8.3。

---

## 3. P0 基准协议冻结（测量先行，无生产代码改动）

目的：后续所有阶段的收益有统一口径与归因手段，避免"感觉变快"。先行落地，半天级工作量。

| 任务 | 内容 | 验收 |
|---|---|---|
| R0.1 命令冻结 | 30 页门禁集、reference、同一构建参数（**基线在复读退化方案合并后的构建上采集**，§8.3——其输出预算变更会改变逐页解码时长分布，58.4 s 仅历史参照）；改动前后各跑一遍，记录 `result.json` `elapsed_ms` 与 `RuntimeTelemetry` 四字段（`load_time_ms / first_token_ms / generated_tokens / generation_time_ms`，`OCRX_GPU_REPORT=1`） | 基线报告落 `docs/test/reports/perf-baseline-<date>.md`（格式沿用质量门报告）；同构建连跑两遍 `elapsed_ms` 偏差 <2% 方可作为对照基线 |
| R0.2 解码上界参照 | 用与绑定同源的 llama.cpp 尝试构建 `llama-bench` 测同 GGUF 单流 tok/s，作为流水线上界；工件不可得时以单页 fixture 的 `generation_time_ms/generated_tokens` 作解码段代理指标并在报告注明口径 | 报告含上界值或口径声明 |
| R0.3 观测口径 | 解码期 `nvidia-smi dmon` 采样（SM vs 显存控制器占用），确认带宽瓶颈形态；P4 实施后同口径复测，验证合批把瓶颈从"带宽空转"推向"带宽饱和" | 报告附录含采样结论 |
| R0.4 归因规则 | `generation_time_ms`（解码段）与 `elapsed_ms - generation_time_ms`（渲染+编码+后处理段）拆分对比：P1 应压缩段内固定开销、P3 应压缩后者占比。注意 `first_token_ms` 的跨页聚合口径是**最小值**（`llama_mtmd.rs:93-99`），不是均值，任何均值口径必须另行采集（见 R0.5/P4.1） | 每阶段报告按二段归因 |
| R0.5 每 token 边际开销归因（v2 新增为评审 MAJOR-1 整改落点；v3.1 修正口径） | 同一单页 fixture 分别以 `--max-output-tokens 32` 与 `2048` 扫描：(t2048 − t32) / (n2048 − n32) ≈ **每 token 边际成本**。**差分按设计消去一切不随 token 数缩放的项**（模型加载、渲染、bitmap/tokenize、图像编码、页级 context 创建、后处理），故本任务只量化按 token 缩放项（argmax、batch 分配等；argmax 理论量级：~150k f32 比较 ≈ 数十 µs，对比 ~14 ms/token 预算，预期 <1%），并与 R0.2 上界对照得到"本仓库解码循环相对裸 llama.cpp 的每 token 附加开销"；**页级固定开销的归因见 R0.5b，不得引用本任务数据** | 归因数据落基线报告；作为 T2.1/T2.2 判定与 P4 批深设计的输入 |
| R0.5b 页级固定开销归因（v3.1 新增，评审意见：R0.5 差分消去固定成本，无法支撑 T2.3/P4.1 的 ≥2% 判定） | 构造/选用 k 页**内容相同**的 fixture（P0 允许新增测试 fixture，不动生产代码），同一 `--max-output-tokens` cap 下分别扫描 1 页与 k 页：(t_k − t_1) / (k − 1) ≈ **每页全成本**；页级固定开销 ≈ 每页全成本 − 每 token 边际（R0.5）× **实际生成 token 数**（`RuntimeTelemetry.generated_tokens` 按 k 页均值；cap 只是控制变量而非扣减基数——按 cap 扣减会在页提前 EOS 时多扣、低估固定开销，v3.5 评审 MAJOR 整改）− 每页渲染段（R0.4 口径）。交叉校验：单页独立运行的 `first_token_ms` 本身覆盖 bitmap/tokenize/context 创建/图像编码（生成时钟起点在 bitmap 构造之前，`llama_mtmd.rs:143-147`），其多次运行均值可直接作为"编码 + 页级固定"之和的上界参照 | 页级固定开销占单页耗时的份额落基线报告；作为 P4.1 收益预期输入，以及"若 P4 收缩则 T2.3 回退条件项"的 ≥2% 判定数据源 |
| R0.6 合批收益上界预估（v3 新增，P4 设计输入） | 以 R0.2 的单流 tok/s 与 4090 D 显存带宽（≈1 TB/s）估算批深 N 的理论上界（带宽饱和前吞吐 ≈ N×单流）；`llama-bench` 若支持 `-p/-n` 变体与并行参数则以多请求模式实测参照 | 预估表落基线报告，供 P4.2 设计文档确定起始批深与爬坡区间 |

**收益判定规则（适用于 P1/P3 各子项）：** 单项实测收益 <2% 且增加代码复杂度时，放弃该项并回滚，在报告记录"保持简单优先于榨取最后几个百分点"。P4 的批深取值同理由 R0.6 + P4.4 实测曲线冻结，预期收益不构成跳过实测的理由。

---

## 4. P1 运行时清理（分析 §3.2 的可行子集，全部后端受益）

v2 重新划定范围：v1 的 T2.1（sampler 替换）经核实不成立，取消默认实施（改由 R0.5 归因决定是否有任何微优化空间）；T2.3 无法按"局部加字段+清缓存"实现（BLK-1），重构为推理线程所有权模型——v3 起因 P4 排期提升为必选项，编号 **P4.1**，任务归属 §7，设计规格仍列于本节（runtime 层变更集中于此）。**P1 的现实预期：解码段每 token 开销在 llama.cpp 单流架构内主要由后端实现决定（P2 是该段的主杠杆，P4 是数量级杠杆）；P1 自身的可兑现收益集中在编码/prefill 段（T2.4）与批复用（T2.2），页级固定开销的消除随 P4.1 实施并归入 P4 收益。**

各子项独立提交、独立可回滚；每项 Green 后立即跑 env-gated 真实 E2E + 金集 + §3 基准。

### T2.1 解码段归因与采样路径决策（v2 重写：不实施 sampler 替换）

- **结论（先行冻结，非待办）：** 维持现有"借入切片 argmax"实现。依据 §1.3 修正 3：argmax 对借入切片零拷贝扫描，理论占比 <1%（R0.5 复核）；`LlamaSampler::greedy()`（独立采样器）路径反而新增每 token 全词表 `llama_token_data` 分配+拷贝（`llama-sampler.cpp:823-855`），预期为净退化。
- 若 R0.5 出乎意料显示 Rust 包装层开销 ≥2%：唯一允许的重试方向是 `LlamaSampler::chain_simple([greedy])`（chain 复用候选缓冲，避免每 token 局部分配），仍需实测收益 ≥2% 才保留；否则回退。
- 不设计、不验证 GPU 侧采样（非目标）。
- 验收：R0.5 归因数据落报告即视为本项完成；无生产代码改动预期。

### T2.2 batch 复用（不变）

- 现状：`llama_mtmd.rs:269` 每 token `LlamaBatch::new(1, 1)`（分配 + 绑定开销）。
- 方案：循环外创建 `LlamaBatch::new(1, 1)`，每步 `batch.clear()` 后 `add`（`llama_batch.rs:34`，clear 即 token 计数归零）。
- 验收：金集逐字节不变（§2.3 第一档）；收益按 R0.5 判定，<2% 即回滚（保留现状的每 token 小分配）。

### T2.3 推理会话所有权重构（= P4.1；v2 重设计为评审 BLK-1 整改落点，v3 由条件项提升为必选）

**问题重述。** 目标是消除每页 `new_context`（`llama_mtmd.rs:175-182`）的页级固定开销，并为 P4 合批提供可持有的多序列会话。`LlamaContext<'a>` 借用 `LlamaModel`（§1.3 修正 4），"runtime 结构体同时持有 model 与跨调用 context"在安全 Rust 下不可表达；`Option<Lazy>`、字段顺序、`Rc`/`Arc` 均不解决借用，`Box::leak` 造 `'static` 在模型可重载的场景不可接受，workspace `unsafe_code = "forbid"` 排除裸指针方案。

**选定设计：专用推理线程所有权（actor）。** 借用关系在线程闭包的局部作用域内合法成立：

```text
LlamaMtmdRuntime（handle，主线程）          推理线程（唯一所有者）
├─ request_tx / progress_rx / result       ├─ LlamaBackend、LlamaModel
├─ join: Option<JoinHandle>               ├─ MtmdContext
└─ Runtime trait 实现（签名不变）           ├─ LlamaContext<'a>（借局部 model，页间 clear_kv_cache）
                                           ├─ LlamaSampler（如启用）、遥测累加
                                           └─ 消息循环：Load 握手 → Generate{request, CancellationToken 克隆}
                                              → 每秒 progress 流 → Result → … → 通道关闭即退出
                                           （P4 起扩展为多槽位窗口协议，见 §7）
```

- **所有权**：backend/model/mtmd/context 全部为推理线程闭包局部变量，`new_context` 的生命周期借用在线程内自洽；`LlamaMtmdRuntime` 只持有通道与 `JoinHandle`。
- **加载语义**：`LlamaMtmdRuntime::load` spawn 线程后**同步等待 LoadResult 握手**（成功/失败经通道返回），失败时 join 线程并返回 `Err`——与现有 `load` 的失败语义一致。
- **生成语义**：`generate_with_progress` 克隆调用方的 `CancellationToken`（`Arc<AtomicU8>` 支持，`ocrx-core/src/lib.rs:16-19`，克隆共享取消状态）随请求发送；随后在 handle 侧转发 progress 流直至收到 `Result`。worker 侧取消检查点与现状完全一致（M9）：eval 前、eval 后、每 token 之间；`eval_chunks` 仍是不可中断的单次阻塞 FFI 调用，行为不变。
- **心跳**：每秒 progress 事件由 worker 线程按既有契约产生（`Instant` 计时在线程内，语义不变），handle 原样转发。
- **错误与 panic**：worker 内 `Err` 作为 `Result` 消息返回；worker 线程 panic 表现为通道断开，handle 侧转为 `Err("inference worker terminated")`——fail-loud，不得静默。
- **遥测**：worker 线程内累加（`record_generation` 逻辑原样迁移）；handle 的 `telemetry()` 在 `&self` 上做请求/响应往返。`generate_with_progress` 是 `&mut self`，天然串行，无并发访问。
- **单飞约束**：handle 的 `&mut self` 方法串行化请求；worker 顺序服务，本阶段**不引入任何推理并发**（P4 在此协议之上扩展窗口）。
- **Drop**：handle Drop → 请求通道关闭 → worker 完成当前生成后退出并释放全部 llama.cpp 资源；进程级二次 Ctrl+C 强退契约（130/立即退出）不变。
- **trait 不变**：`Runtime` trait、`FakeRuntime`、ocrx-output/ocrx-ovisocr2/CLI 全部不动，改动封闭在 `ocrx-runtime` crate 内。
- **战略价值**：本 actor 是 P4 合批的执行模型——合批是线程内消息循环的多槽位扩展（§7），handle 协议平滑演进；后端可扩展性重构启动时以该 runtime 为迁移输入（§7 决策记录）。

**实施状态（v3）：** P4 已排期（§7 决策记录），actor 为其必选前置，本项必做，并**先于 P4.2–P4.4 单独合并**（此阶段仍为单流解码，验收走 §2.3 第一档逐字节门禁）。R0.5b 的页级开销归因保留为收益度量输入，不再是去留开关；若 P4 后续被上层决策收缩，本项自动回退为 R0.5b 判定的条件项（页级固定开销占单页耗时 ≥2%，数据口径见 §3 R0.5b）。

**已考虑并否决的备选：**
- *Session trait 扩展*（runtime 暴露借用式会话对象，调用方跨页持有）：会波及 `ocrx-output` 的 `PageRecognizer<M>` 泛型、loader 与全部测试的签名；P4 排期后其"多请求接口"诉求同样要动 trait，但 actor 方案把变更封闭在 runtime 内、trait 演进推迟到 P4.4 一并设计，波及面更小。
- *self_cell/ouroboros 自引用 crate*：新增依赖换取页级开销一个次要量级的收益，与仓库钉死依赖的纪律不相称。
- *每页新建 context（现状）*：即回退方案，零成本。

**验收（若实施）：**
- 金集逐字节不变（§2.3 第一档）。
- 页级开销下降的度量口径：**单页 fixture 独立扫描 30 次，取每次运行 `first_token_ms` 的均值**（每次单页运行中 `first_token_ms` 即该页首 token；现有跨页聚合是 min 口径，不能直接当均值用——评审 MINOR-1 整改）。对比实施前后均值。
- 取消契约：`windows-ctrl-c-smoke.ps1` 冒烟不变（130、checkpoint 保留、无 `.part` 残留）；worker panic 注入测试（线程内 `panic!` → handle 得到 `Err` 而非死锁）。
- 与复读退化方案 D2（环检测）的共存适配见 §8.3。

### T2.4 flash attention（不变；主要惠及 prefill/mmproj 编码段）

- 方案：`LlamaContextParams` 增加 `with_flash_attention_policy(AUTO 起步)`（`context/params/get_set.rs:313`，sys 枚举 `llama_flash_attn_type`）。AUTO 交由 llama.cpp 按后端能力自行决定，规避内嵌 `9e3b928` 的 Vulkan 后端 FA 支持范围未知导致的初始化失败。
- 决策规则：AUTO 实测无收益或异常 → 保持默认并记录；CUDA 落地后在 P2 环境重测更强档位。**不**为追求 FA 顺带引入 KV 量化（`type_k`/`type_v`）等关联变更（超出非目标）。
- 验收：金集逐字节不变；`first_token_ms`（min 口径即可作方向性参照）与 R0.4 二段归因变化记入报告。

### P1 合并点门禁

`cargo fmt --all -- --check`；clippy 零警告；`cargo test --workspace`（默认 CPU 特性）；`--features vulkan` 构建通过；`OCRX_E2E_MODEL=1` 真实 E2E；30 页金集（§2.3 第一档）；§3 基准对照报告（含 R0.5 归因）。

---

## 5. P2 CUDA 发布链路闭合（分析 §3.4 = W4.3 执行）

**toolkit 已核验在机（v3.2，2026-10-31）**：CUDA 12.4 / nvcc V12.4.131 / `CUDA_PATH` 已设 / 驱动 610.88 未变（证据见 §1.3 修正 5），W4.3 的外部阻塞解除，本阶段立即可执行，**无安装步骤**；按 W4.3 既有门禁执行，本节为其执行细化。P2 与 P4 正交（合批在后端之上）；批深收益曲线应在 Vulkan/CUDA 分别实测（CUDA 削减每 token 同步开销后，合批的相对增益构成不同）。

### T1.0 构建环境核验（v3.2 新增；原"安装 toolkit"前置步骤取消）

- 记录版本矩阵并落 W4.3 证据链：nvcc 12.4.131 / `CUDA_PATH` / 驱动 610.88 / MSVC 17.14（cl 14.44）/ CMake 版本。
- **已知兼容性风险（本任务首个验证项）**：MSVC 17.14 高于 nvcc 12.4 官方支持的 VS 版本范围（约至 17.9），构建可能在 nvcc 探测宿主编译器时报 unsupported compiler。处置预案按优先级：① 升级 toolkit 至官方支持 17.14 的版本（12.6+，仍是本机可逆安装）；② 经构建系统向 nvcc 注入 `-allow-unsupported-compiler`（需先验证 `llama-cpp-sys-2` 构建脚本有无注入通道，无通道即不可行，不得为此改 vendor 源）；③ 为 VS 17.14 并行安装 17.9 工具集组件（MSVC 支持多工具集共存）。任一处置都必须以 T1.1 构建通过 + 对应档位金集门禁为准，不得静默跳过。

### T1.1 构建与运行验证

- `cargo build --release -p ocrx --features cuda`（`CUDA_PATH` 约定路径）。沿用短 `CARGO_TARGET_DIR` 纪律（已知问题 §0.5 的 250 字符嵌套路径上限对 CUDA 构建同样适用）。
- 运行验证：`doctor --report json` 报 `backend=Cuda`（对照质量门报告 §6 的 `backend=Vulkan` 形态）；`--device cuda` 从 "requested device is unavailable"（`main.rs:1962-1969` 的 fail-loud）变为可用；`auto` 在编译集含 cuda 时按 `metal > cuda > vulkan` 优先序选择（`ocrx-runtime/src/lib.rs:57-79` 契约已就绪，无需改动）。
- Red：无 toolkit 环境构建失败为预期；CI 无 NVIDIA GPU 时 CUDA 路径标注 `DeviceUnavailable` 而非构建失败（W4.3 既有约定）。

### T1.0/T1.1 执行记录（2026-10-31，本机实测）

**T1.0 版本矩阵（已核验）**：CUDA Toolkit 12.4（nvcc V12.4.131，`CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4`）｜驱动 610.88（未动）｜VS 2022 BuildTools 17.14.13（MSVC 工具集 14.44.35207）｜CMake 3.31.6（VS 自带）｜`cuda` feature 映射 `llama-cpp-sys-2/cuda`（`ocrx-runtime/Cargo.toml:18`）。

**T1.1 构建验证：通过**（第 3 次尝试，14m 14s，`BUILD_EXIT_CODE=0`，产物 `D:\ocrx-tgt\release\ocrx.exe` 536 MB 静态链接）。

- **预判的 unsupported-compiler 风险未发生**：nvcc 12.4 在 MSVC 17.14（cl 14.44）下完成全部 CUDA kernel 编译，无告警——§5 T1.0 的三条处置预案均不需要。
- **实际暴露并解决的是另一个问题（新发现，进入 W4.3 证据链）**：CMake 的 Visual Studio 生成器报 `No CUDA toolset found`——toolkit 安装时未把 VS 集成组件（`extras/visual_studio_integration/MSBuildExtensions/` 下的 `CUDA 12.4.props/.targets/.xml` + tasks dll）拷入 BuildTools 的 `BuildCustomizations\`（该拷贝步骤需管理员权限）。**绕开方案（已验证，无需提权）**：`CMAKE_GENERATOR=Ninja` + vcvars64 环境（VS 自带 ninja.exe；Ninja 生成器直接驱动 nvcc，不做 VS toolset 查找）。注意失败重试必须先清 `CARGO_TARGET_DIR`（VS 生成器残留的脏 CMake 缓存会让后续 `cmake --build` 立即失败，本次第 2 次尝试即踩此坑）。
- **运行验证：通过**——`doctor --report json` 报 `"backend":"Cuda"`、`"recommended_device":"cuda"`（对照质量门报告 §6 的 `backend=Vulkan` 形态）；`--device cuda` 不再返回 "requested device is unavailable"；真实 CUDA E2E 冒烟通过：单页 fixture（`figure-1-page.pdf`，compact，`--max-output-tokens 512`）exit 0、`result.json` `status=success`/`backend=cuda`/`elapsed_ms=3667`/0 failed 0 truncated、`document.md`/`page-0001.md`/checkpoint/`images/` 产出完整。
- **结论**：CUDA 工具链路径在本机完全可用；对发布链路的含义见 T1.2/T1.3（打包与安装脚本仍是待办）。T1.0 的处置预案 ②③ 未被触发，预案 ①（toolkit 升级）也不需要。

### T1.4 前置数据：CUDA vs Vulkan 单流基线（2026-10-31，**本机 4090 D 预研口径**，非发布基线）

**方法**：30 页门禁集（`ovisocr2-draft-v1/inputs/source.pdf`），同一构建参数，CUDA（`D:\ocrx-tgt`）与 Vulkan（`D:\ocrx-tgt-vulkan`）二进制各跑 reference 与 compact 两档，`OCRX_GPU_REPORT=1` 采遥测；每格连跑两遍（间隔约 1 小时），`elapsed_ms` 偏差 <3%，取第二遍为记录值。PDFium 300 DPI、像素预算缩放等条件与质量门报告 §6 相同。**数据为单机预研，正式发布基线仍按 W4.2/W4.4 纪律在登记基准机采集。**

| 档位 | 后端 | 墙钟（30 页） | 解码 tok/s | load_ms | first_token_ms | VRAM 峰值 | **CUDA/Vulkan 加速比** |
|---|---|---:|---:|---:|---:|---:|---:|
| reference | CUDA | **57.3 s** | 283.9 | 1446 | 115 | 6.3 GB | **1.26×** |
| reference | Vulkan | 71.9 s | 219.0 | 1711 | 136 | 5.9 GB | — |
| compact | CUDA | **43.7 s** | 368.8 | 1126 | 119 | 5.3 GB | **1.38×** |
| compact | Vulkan | 60.5 s | 252.1 | 1192 | 136 | 5.3 GB | — |

**判读（对照 §5 前置语的 1.5–3× 假设）**：

1. **实测加速比 1.26–1.38×，低于分析假设区间（1.5–3×）**。解码段 tok/s 提升明显（+30% 至 +46%），但被端到端中的渲染/编码/后处理段摊薄——R0.4 口径下 CUDA 只压缩解码段，全页时间还有约 40% 不受后端影响（compact 档 43.7 s vs 解码 34.8 s 可推得非解码段 ≈9 s 量级，与 P3 的目标段吻合）。
2. **历史基线自洽**：2026-10-30 质量门报告的 Vulkan reference 58.4 s 与本次 71.9 s 差异显著——本次未复现该数字，差异来源未隔离（驱动状态/后台负载/该报告的构建差异均可疑）；**不影响本次对照的有效性（两组同条件同时段互测）**，但提示 W4.2 的"登记基准机复跑"前不应把 58.4 s 当作可复现常数。CPU 对照未采（562.6 s 历史值，本次无 CPU 构建）。
3. **对 P2 立项的含义**：CUDA 收益真实但有限（墙钟 1.26–1.38×），"安装即 1.5–3×"的预期应按实测下调；与 P3（隐藏非解码段 ≈9 s）叠加后，端到端 57.3 → 约 48 s（reference）可期——**P2+P3 合计 ≈1.5×，其中 P3 的边际收益与 P2 相当**，这与 §10 的"预期待实测"形成了第一组实测锚点。
4. **对 P4 的含义**：解码段在 CUDA reference 下为 ≈45 s（12840 tok / 283.9 tok/s），占墙钟 79%——满足 §7 P4 启动条件的"解码主导"形态（85% 阈值附近）；合批在 4090 D 上的上界收益空间仍由 R0.6/P4.4 实测。

### T1.2 打包脚本扩展

- `scripts/package-windows-x64.ps1:110-116`：`-GpuPackages` 增加 `cuda` 分支——检查 `CUDA_PATH`（类比 `:112` 对 `VULKAN_SDK` 的检查与报错形态）→ `cargo build --release --locked -p ocrx --features cuda` → 产物 `bin/ocrx-cuda.exe`。CPU 基线先拷贝的既有顺序（`:105-109`）保持。
- **构建环境承接（v3.10，重审 MAJOR-1 整改）**：T1.1 已实测 VS CMake 生成器因缺 CUDA toolset 集成而失败（§5.5），仅 `CMAKE_GENERATOR=Ninja` + vcvars64 环境可用——cuda 分支必须把该环境写成脚本内可执行步骤，不得假设调用方环境已初始化：① 校验/设置 `CMAKE_GENERATOR=Ninja`（ninja.exe 可取自 VS 自带路径）；② 脚本初始化 MSVC x64 环境（调用 `vcvars64.bat`）或在缺失时 fail-loud 提示"须从 VS x64 开发者环境运行"；③ 设定短 `CARGO_TARGET_DIR`（嵌套路径 250 字符上限，W4 已知问题）；④ 重试前清理目标目录（VS 生成器残留的脏 CMake 缓存使构建立即失败，§5.5 踩坑记录）。脚本烟测把这四项作为 cuda 分支的断言条件。
- Red：脚本级测试断言 cuda 分支在 `CUDA_PATH` 缺失、非 Ninja 生成器、或 MSVC 环境未初始化时均 fail-loud；对未知枚举值 `:111` 的硬拒行为保留。

### T1.3 安装器 GPU 变体优先级链

- `scripts/install.ps1:153-177` 的 vulkan 提升逻辑**泛化为优先级链**：探测结果为 cuda（`nvcuda.dll`，`:27-32`）→ 依次尝试 `ocrx-cuda.exe` → `ocrx-vulkan.exe` → CPU 基线；每级做 launch 验证（`version` 退出码 0），失败降级并输出明确日志。现有"探测到 cuda 但只有 vulkan 工件 → 提升 vulkan"（`:161`）成为链中一环而非唯一分支。
- 验收：工件组合 × 探测机器的安装烟测矩阵全绿——{cuda+vulkan+cpu, vulkan+cpu, cpu-only} × {nvcuda 存在, 仅 vulkan-1, 均无}；升级安装（bin 目录 swap）语义不变；`Assert-ArchiveHashes` 覆盖新增工件。

### T1.4 门禁与基线（W4.3 纪律）

- E2E：CUDA 路径 model/mmproj 加载、MTMD 编码、生成、裁图全链路；与 CPU 输出**结构化比对**（§2.3 第二档：跨后端浮点差异预期存在，门禁是结构/质量相似度而非逐字节）；显存不足安全失败注入（不得 OOM 崩溃）。
- 基线：同 fixture 的 CUDA vs Vulkan vs CPU 三方 `elapsed_ms`/tok/s 对照，落 `docs/develop/windows-gpu-baseline-*` 增量记录。分析预期"小模型单流 1.5–3×"为假设，实测为准；R0.5 的每 token 附加开销口径同步复测，验证"Vulkan 每节点同步提交开销高于 CUDA"（分析 §3.4）的具体占比。
- 发布规则：通过 → `ocrx-cuda.exe` 进入发布工件（NVIDIA 专用加速）；未通过 → 仅阻断 CUDA 工件，Vulkan/CPU 发布不受影响（`windows-platform-development-plan.md:164`）。

---

## 6. P3 渲染/解码重叠（分析 §3.3；v2 按 MAJOR-2/3 重设计）

### 6.1 安全不变式（v2 重写：覆盖全部进程内 PDFium 入口）

v1 以"全部 PDFium 调用发生且只发生在渲染线程内"作为线程安全依据，但该声明未覆盖现状：CLI 在构造任何 `PageRenderer` **之前**已在主线程执行两段 PDFium 路径——页数检查（`main.rs:1910-1917`）与 `analyze_selected_pages`（`:1985-1994`）；且 pdfium-render 绑定是**进程全局**的 `OnceLock`（`ocrx-pdf/src/lib.rs:1514-1519`），并非每线程独立。

v2 的不变式升级为：**本进程在 scan 命令期间的全部 PDFium 调用（页数、分析、渲染、缩放）由且仅由一个专用 PdfiumOwner 线程执行，主线程经通道请求。** PdfiumOwner 在 `run_scan` 进入输入校验阶段（`main.rs:1910` 之前）创建，先服务页数与分析请求，再作为渲染流水线运行——前置阶段与渲染阶段同线程，不存在跨线程交接绑定的问题。

已考虑并否决的备选：保留主线程前置分析、事后交接绑定给渲染线程。否决理由：其安全前提是"同一进程全局绑定可跨线程先后复用"，PDFium 明确不支持多线程访问，而"先后跨线程"的安全性无廉价验证手段，与仓库 fail-loud 纪律不相称。

### 6.2 默认路径：PdfiumOwner 线程 + 预取（`ocrx-cli`）

- **线程与协议**：PdfiumOwner 线程持有 `PdfiumRenderer`（线程内构造，绑定初始化发生且只发生在此线程）；通道请求 `{ PageCount(input), Analyze(input, pages, tier), Render(page), RenderPerturbed(page, numerator, denominator) }`，响应携带结果或错误。`run_scan` 的页数检查与分析阶段改为同步请求-等待，`ProgressStage::Analyzing` 等进度事件仍由主线程在等待前后发出（UX 不变）。
- **扰动渲染转发（v3.10，重审 BLK-1 整改）**：`PageRenderer::render_perturbed` 的默认实现忽略缩放比例并回落 `render()`（`ocrx-output/src/lib.rs:404-417`），复读恢复的 attempt 1/2 依赖 `PdfScanRenderer` 的 `render_with_scale` 覆盖（`main.rs:1667-1673`）——包装器若只实现 `render()`，恢复重试会静默退回原图。因此包装器**必须实现 `render_perturbed`** 并转发 `RenderPerturbed(page, numerator, denominator)` 请求；owner 线程内走与 `PdfScanRenderer` 相同的 `render_with_scale` 路径，保证 RGB 像素、`RenderAudit`（scaled 与比例如实记录）、失败语义与串行路径一致。扰动请求**按需渲染、不经预取窗口**（重试页通常已被消费出窗口，预取无意义）；预取窗口不含该页时不产生额外清理。Red/验收：默认路径与 worker 路径各加集成用例——attempt 0/1/2 的模型输入与 `RenderAudit` 确实不同（0.94/1.06 比例生效），不能只测 `render(page)`。**另登记主干现存缺口**：`WorkerScanRenderer` 当前未覆盖 `render_perturbed`（worker 模式下的扰动重试已静默失效），随 §6.3 worker 路径一并补齐。
- **渲染流水线**：owner 线程在返回页 N 的 `RenderedPage` 后继续预取渲染至 N+K（K=2 起步，内部冻结常量，不暴露参数）；现有 `render()` 内的 CPU 后处理——像素预算缩放（`fit_pixel_budget` + Triangle resize，`main.rs:1544-1553`）——在 owner 线程完成，重叠窗口覆盖"渲染+缩放"全程。主线程实现 `PageRenderer` 的包装器向 owner 发 `Render` 请求。
- **顺序性**：owner 线程严格按页序；扫描循环页序与 checkpoint 提交顺序不变（`ocrx-output/src/lib.rs:607` 循环不感知重叠）。P4 合批落地后，渲染预取窗口与推理页窗口的衔接由 P4.4 编排统一设计（渲染侧只需保证"页可提前可用"，不假设消费节拍）。
- **失败语义**：渲染错误**作为值**经通道携带页号回传（通道不断开），在对应页 `render()` 处走现有 `render_failed` 诊断路径（`lib.rs:637-653` 不变）；"失败继续后续页"语义要求包装器在错误后仍可接收后续页请求。
- **取消与失败语义（v3.10 按 BLK-2 收紧）**：cancellation → 主线程停止请求新页 → 包装器 Drop → 请求通道关闭 → owner 线程退出。进程内路径无子进程，无孤儿进程风险。**owner 悬挂/超时/通道断开一律 fail-loud**：同步请求超时或通道断开 → owner 标记 poisoned → 本次 scan 以结构化错误终止（已完成页 checkpoint 保留、可 resume），**主线程绝不接管 PDFium 渲染**——超时只证明等待到限，不证明 owner 的 PDFium 调用已结束，此时主线程直接渲染既跨线程复用进程全局绑定、也可能与在飞 owner 并发进入 PDFium（§6.1 不变式）。串行回退**只存在于代码 revert（编译期选择）**，不存在运行时降级。
- **资源**：峰值内存增量 = K 页 RGB8（每页 ≤2880×2880×3 ≈25 MB，K=2 即 ≤50 MB）。100 页资源回归门禁必须复跑（§6.4）。P4 落地后预取深度与批深 N 的内存叠加需在 P4.4 资源回归中合并复测。
- **不变式测试**：以构造方式保证（PdfiumRenderer 只能在线程内创建/使用，主线程不接触 pdfium-render 类型）；worker 模式不受影响（渲染在独立进程，进程内仅有前置分析，顺序单飞）。

### 6.3 worker 路径（`OCRX_PDF_WORKER` 显式启用时；v2 按实际目录模型重写）

现状目录模型（v1 表述有误，评审 MAJOR-3）：`.worker-{start:04}-{end:04}` 只是 scheduler 的**临时**工作目录——segment 成功后 PNG/audit 被逐一 rename 到共同的 `output_root`（`ocrx-pdf/src/lib.rs:1455-1469`），临时目录随即删除（`:1471-1474`）；`WorkerScanRenderer` 从共同根目录读页（`main.rs:1519-1524`）。可供"双缓冲"长期共存的 segment 目录**并不存在**。

v2 选用布局：**共享根目录 + 按段页号范围清理**（scheduler 的提交模型保持不动）：

- **预取**：`WorkerScanRenderer::render()` 返回当前段页图后，在后台线程（clone `PdfWorkerScheduler`——其 `run(&self)` 为共享语义，`lib.rs:1348-1356`；取消回调绑定 `CancellationToken`）启动下一段渲染。预取深度 = 1 segment，任何时刻至多一个 worker 进程在飞。
- **无冲突依据**：各段页号互不重叠，后台段提交时把 `page-NNNN.png/audit` rename 进同一根目录，与主线程正在读取的当前段文件互不干扰。
- **清理规则收窄**：`main.rs:1510-1512` 的"段推进时删除整个根目录"改为**仅删除已消费完毕段 `page-{start..end}` 的页文件**；预取段已提交的页文件保留待消费。临时 `.worker-*` 目录维持现状自清理。取消/失败路径 best-effort 清理预取未消费页与在飞临时目录（沿用 m1 纪律）。
- **扰动渲染补齐（v3.10）**：为 `WorkerScanRenderer` 补 `render_perturbed` 覆盖（把比例参数传入 scheduler 渲染或对已提交页按同一算法重采样，RenderAudit 如实记录）——主干现状该覆盖缺失，worker 模式扰动重试静默失效（§6.2 已登记）；同样加入 attempt 0/1/2 输入与 audit 不同的集成验收。
- Red 用例：预取段失败在对应页暴露 `render_failed`（不提前、不错页）；取消后根目录无预取残留、无孤儿 worker 进程（`run_with_cancel` 的 `child.kill()`，`lib.rs:1425-1431`）。

### 6.4 验收（不变，新增目录/不变式项）

- 30 页金集：输出与串行基线**逐字节一致**（§2.3 第一档；渲染确定性 + 推理确定性 ⇒ 重叠只改时序不改结果）。
- 墙钟：`elapsed_ms` 下降、`generation_time_ms` 不变（R0.4 归因），下降幅度 ≈ 渲染+缩放段占比。
- 资源：100 页资源回归无随页数线性增长回退；worker 模式无双 worker 进程并存、根目录无预取残留。
- 扰动渲染（v3.10，BLK-1）：默认与 worker 路径各验 attempt 0/1/2 模型输入与 `RenderAudit` 确实不同（0.94/1.06 生效）；30 页金集 attempt 0 输出仍逐字节不变。
- owner 故障注入（v3.10，BLK-2）：注入请求悬挂 → 断言主线程零 PDFium 调用、无并发渲染、scan 以结构化错误终止、已完成 checkpoint 可 resume。
- 取消：`windows-ctrl-c-smoke.ps1` 冒烟行为不变（130、checkpoint 保留、无 `.part` 残留、`--resume` 复用）。
- 回滚：**仅限代码 revert（编译期选择）**——revert 后恢复串行渲染，运行中不存在"切回主线程渲染"的降级路径（§6.2 BLK-2 语义）；worker 预取可单独关闭（同为编译期/配置选择，不涉及线程回退）。

---

## 7. P4 多页 continuous batching（v3：排期项——优先于后端可扩展性重构）

### 7.1 决策记录（2026-10-31）

P4 由"条件启动"提前为**排期项**，先于 `model-backend-extensibility-refactoring-development-plan.md` 实施。依据：该重构尚未启动（待实施），而两者都要重塑推理 API 层——先做 P4，其多请求接口落在现有 `Runtime` trait 与 P4.1 actor 之上；重构启动时以"已具备合批能力的 ocrx-runtime"为迁移输入，在其新推理 API 设计中把多请求作为既有能力承接，而不是反过来让 P4 等待并适配新 API。代价与对冲：重构计划的 §2.1"多页串行复用"保留项与 §2.2"不引入并发 OCR"冻结项需在其实施前修订（彼文档头部已加协调注记）；P4 的窗口协议设计（P4.4）须给重构后的 API 留出映射空间（多请求能力收敛为 runtime 内部实现细节 + 最小接口面）。

### 7.2 定位与预期

- 定位：唯一能显著提升 GPU 利用率读数与吞吐数量级的路径（分析 §5#4）。解码是带宽瓶颈型负载，合批把多页的权重读取摊薄为一次批量 GEMM，吞吐随批深近似线性增长直至算力/其他瓶颈；**预期解码段 2–4×，端到端累计 3–8×**（58.4 s → 10–20 s 量级，§10）。均为待实测假设，上界预估见 R0.6。
- 与 P2 的叠加关系：CUDA 已削减每 token 同步开销，P4 在 CUDA 上的相对增益构成不同于 Vulkan（Vulkan 下合批摊薄 WDDM 同步的收益更大）；批深-吞吐曲线须分别实测后冻结默认值。
- 硬件边界：批深收益在带宽饱和后终止；R0.3 口径下可观测到瓶颈从"带宽空转"移向"带宽饱和"。

### 7.3 执行模型与任务分解

执行模型：**在 P4.1 actor 内实现**——合批是线程内消息循环的多槽位扩展，handle 协议从单请求 `Generate` 演进为页窗口提交（`Submit(window)` / 逐页完成流 / 窗口结果）。接口与批次策略细节由 P4.2 的设计文档冻结后方可实施 P4.3+；本文只登记已确定的分解与设计约束。

| 任务 | 内容 | 前置 |
|---|---|---|
| **P4.1 推理线程 actor** | = §4 T2.3（必选），规格、验收、回滚均按该节；**先于 P4.2–P4.4 单独合并**，验收走 §2.3 第一档 | P1 的 T2.2/T2.4 先合并（同文件冲突规避） |
| **P4.1.5 de-risk 实验（v3.3 新增；可与 P4.1 并行）** | **Spike A 批分叉定量**：同一批页面分别以单流与合批（N=2/4）解码，量测输出分叉的频率（多少页出现 token 分叉）与深度（分叉页的结构化相似度分布），产出定线数据——这是 §2.3 第二档 P4 阈值的唯一依据（2026-10-31 决策：不拍脑袋、不沿用跨后端先例数字）。**Spike B 多序列解码组合验证**：以最小验证程序走通绑定层多序列组合（多序列 `LlamaBatch::add_sequence`、逐序列 `get_logits_ith`、`kv_cache_seq_rm` 槽位回收、`eval_chunks` 按 `seq_id` 逐槽编码——签名已核实存在，`mtmd.rs:730-738`，组合行为未实测），产出可行/不可行结论与关键 API 行为记录。两实验均不改生产路径（env-gated 测试或临时 example 承载），数据落基线报告 | P0（模型/fixture）；结论是 P4.2 的强制输入 |
| **P4.2 合批设计文档（评审门禁）** | 交付 `docs/develop/batched-inference-<date>.md` 并评审通过。必须冻结：① *上下文预算*：n_ctx 为多序列共享，每页预算 ≈ 图像 token（1024）+ 提示 + 输出上限，双重约束 **N × 每页预算 ≤ n_ctx ≤ n_ctx_train**（n_ctx_train 为模型训练窗口硬上限，超出需 RoPE 外推、属质量变更，本方案不做——v3.5 评审 MAJOR 整改）；**设计基线取复读退化方案冻结值（默认输出 16384 + 配套上下文提升，Q4 决策见 §8.3）**——每页 ≈17.6k token，N=2 即需 ~35k n_ctx。**n_ctx_train 实测（2026-10-31）**：reference GGUF（revision `8a22290a`）`qwen35.context_length = 262144`，按 17.6k/页 得 N 上限 14。**KV 显存亦经元数据核算不构成约束（v3.6，修正 v3.5 的"KV 绑定"误判）**：模型为混合注意力架构（`full_attention_interval = 4`，24 层中仅 6 层全注意力、其余 18 层线性/SSM 常数状态；GQA `head_count_kv = 2`、`key_length = 256`）→ KV ≈ 12 KB/token，每页 17.6k 预算 ≈ 212 MB，N=14 全量预留 ≈ 3 GB（另加 SSM 逐序列常数状态 ≈9 MB × N），4090 D 24 GB 无压力。**故批深天花板即训练窗口推出的 14；默认值的冻结准则见 P4.4——吞吐曲线与窗口代价之外，另有 6 GB 显存硬约束（口径为 8 GB VRAM 标准设备 − ~2 GB 系统应用预留，见 P4.4；默认值不能对齐 4090 D，多数机器达不到）；预期落点 4–8+，N=2 仅为 P4.3 的集成起步档，不是预期默认值**。混合架构（SSM 层逐序列状态）在 llama.cpp 的多序列并行行为属 Spike B 验证范围；KV/SSM 实际占用仍须以 P4.1.5/P4.4 实测为准（本段为元数据推算）；复读方案冻结值若有变，以实际值为准并回写 §8.3。② *确定性弱化*：batched GEMM 与单流 GEMV 浮点归约顺序不同，同一页在批内/单流下、以及不同批组合下输出可能分叉；确定性从"逐字节可复现"弱化为"**相同（输入、批组合）下可复现**"。准入策略必须确定性：固定批深、按页序填充、**不做动态回填**；checkpoint 兼容键纳入批深；**resume 语义冻结（2026-10-31 决策）：中断时在飞窗口的页不提交 checkpoint、恢复时整窗重扫**，使同页断点前后的批组合一致（确定性优先，接受重扫时间代价）；金集验收切换到 §2.3 第二档（结构化相似度），**阈值不预设、由 P4.1.5 Spike A 的分叉实测数据定线**，在本设计文档中冻结。③ *页级语义*：窗口内逐页心跳/取消/EOS/终止原因；复读退化方案的环检测（D2）需**逐序列实例化**（每序列独立 K=256 窗口）；单页推理失败不得拖垮同批其他页，llama.cpp 批级错误（OOM/上下文溢出）的隔离与降级策略（含批深回退 1 = 等价单流）；产物与 checkpoint 仍按页序提交。**提交原子性（v3.5 评审 MAJOR 整改；本设计评审冻结）**：决策②的"整窗重扫"与逐页 checkpoint 提交存在衔接缺口——窗口中途取消时部分页已提交、部分未提交，若仅重扫未提交页，其批组合与原窗口不同，确定性即破。两候选机制：*机制 A 窗口原子提交（建议）*——窗口内全部页到达**终态（成功或失败）**后才逐页提交 checkpoint，取消的在飞窗口零提交；与"失败继续"兼容（失败亦终态，不阻塞同批其他页），代价是断点丢失粒度从 1 页扩大到 1 窗（N 页）。*机制 B 重处理规则*——维持逐页即时提交，checkpoint 携带窗口标识，resume 时发现所属窗口不完整则作废该窗口全部提交、整窗重处理；代价是重做已完成页且作废逻辑更复杂。两机制均满足"同页断点前后批组合一致"；未冻结前不得实现逐页即时提交版的"整窗重扫"。④ *遥测*：`RuntimeTelemetry` 按序列拆分或聚合+批深标注，兼容 `OCRX_GPU_REPORT=1` schema。⑤ *接口面*：多请求能力收敛为 runtime 内部实现 + 最小 trait 扩展（一个可选的窗口方法），为重构后的新推理 API 留映射空间（§7.1） | P0（R0.6 上界预估）+ P4.1 合并 + P4.1.5 两实验结论（定线数据与多序列可行性） |
| **P4.3 runtime 合批实现** | actor 消息循环多槽位：`n_seq_max`、多序列 `LlamaBatch::add_sequence`、逐序列 `kv_cache_seq_rm` 完成清理、mtmd 多图像 chunk 逐槽编码、逐序列 EOS/环检测/终止原因；批深从 N=2 起步按实测曲线爬坡 | P4.2 评审通过 |
| **P4.4 扫描侧编排与门禁** | `ocrx-output`/CLI 页窗口提交：提交顺序、逐页完成回调、失败继续、取消排水（drain 在飞页，不提交 checkpoint）；渲染预取窗口（§6.2）与推理窗口的衔接；30 页金集结构化相似度门禁 + 100 页资源回归（含预取×批深内存叠加）+ Ctrl+C 冒烟 + resume 复扫一致性（同输入+同批深下逐字节自一致）；批深-吞吐曲线落报告并**按以下准则冻结默认批深（2026-10-31 决策）**：硬约束为 **6 GB 显存预算**——预算口径：以 **8 GB VRAM 为标准设备**规划，预留 ~2 GB 给系统其他应用（桌面合成器/浏览器等，避免整机跑炸），应用侧可用即 6 GB；断言项 = reference 权重 1.7 GB + 计算缓冲 + N × 每页 KV ≈212 MB 合计 ≤ 6 GB（按元数据估算 N=4 增量 ≈0.85 GB、N=8 ≈1.7 GB；标准设备假设若变更，预算随推导式联动调整）——4090 D 的实测曲线用于判断"更大 N 是否仍有收益"，**不直接决定默认值**；最终默认 N = min(曲线饱和点, 6 GB 安全上限, 窗口代价可接受点)。验证方式：**预算断言门禁**——以 llama.cpp 加载时报告的实际分配量（权重 + KV + 计算缓冲）合计断言 ≤ 6 GB，在发布后端上测（分配量由 n_ctx/N/profile 决定，不依赖真实 6 GB 设备），配合既有的显存不足安全失败注入；更高批深是否随显存探测自动提升归 P4.2 设计（可复用 W1.2 探测注入模式，不新增用户旋钮） | P4.3 + P3 |

### 7.4 门禁补充（P4 专属）

- 金集：§2.3 第二档（结构化相似度），阈值由 P4.1.5 Spike A 实测定线（2026-10-31 决策）；另加**自一致性用例**——同一输入以相同批深连跑两遍，输出逐字节相同（确定性准入策略的验证）。
- resume（2026-10-31 决策：确定性优先）：中断时在飞窗口的页不产生**最终提交**、恢复时整窗重扫，保证同页断点前后批组合一致；提交原子性采用机制 A（窗口原子提交，建议）或机制 B（重处理规则），于 P4.2 设计评审冻结（§7.3 ③）；兼容键含批深，跨批深不互恢复。
- 失败注入：窗口内单页推理失败 → 该页 `recognize` 诊断、同批其他页正常完成；批级故障 → 窗口内全部在飞页按现有失败语义处理并触发批深降级。
- 默认配置显存预算：默认 N 冻结值在发布后端上的实际分配总量（权重 + KV + 计算缓冲）断言 ≤ 6 GB（口径：8 GB VRAM 标准设备 − ~2 GB 系统应用预留，见 P4.4）；超限属发布阻断，不得以"实测机器更大"为由放宽。
- 回滚：批深回退 1（协议保留，行为等价单流）即恢复 P4.1 形态；P4 整体可关闭回 actor 单流。

---

## 8. 实施顺序、依赖与合并纪律

### 8.1 顺序与并行

```text
P0 基准协议（先行，含 R0.5/R0.5b 归因 + R0.6 合批上界）
  ├─ P1 运行时清理（T2.1 决策冻结 + T2.2/T2.4 实施；先行合并，为 P4 腾出 llama_mtmd 文件面）
  ├─ P2 CUDA（toolkit 已核验在机；T1.0 环境核验 → T1.1 构建验证；与 P4 正交）
  ├─ P3 渲染/解码重叠（与 P1/P2 文件面不同，可并行；P4.4 与其衔接）
  └─ P4 合批（P4.1 actor 与 P4.1.5 de-risk 实验并行 → P4.2 设计文档评审 → P4.3/P4.4；依赖 P1 合并 + P0 数据）
后端可扩展性重构（另一计划，v3 起排于 P4 之后启动，以其落地形态为迁移输入）
```

### 8.2 每合并点门禁

- `cargo fmt --all -- --check`；clippy 零警告；`cargo test --workspace`（默认特性）。
- `--features vulkan` 构建通过（P1/P3/P4）；P2 追加 `--features cuda` 构建与真实 E2E。
- `OCRX_E2E_MODEL=1` 真实模型 E2E；30 页金集（§2.3 对应档位）；§3 基准对照。
- 涉及安装/打包脚本：复用 W5.3 脚本烟测矩阵并扩展 cuda 分支用例。

### 8.3 与复读退化方案的合并顺序（v3.4 冻结；**v3.10 更新：复读分支已合并，等待解除**）

**排序决策（2026-10-31）：复读退化方案先行，本方案迭代排在其后。** **v3.10 实况更新：该分支已合并进主干**（`85cc1a3 Merge branch 'feature/repetition-degeneration'`，含输出预算 16384/20480 对齐 `b2bfcce`）——下述适配方向已从"待其合并后执行"变为"立即生效"，**P0/P1/P4 的排序等待解除，本方案的基线须直接在当前主干上采集**（输出预算已变更，58.4 s 双重过时）。原适配方向全文保留作为契约依据：

- 复读方案 D2 的环检测（K=256 环形窗口、早停、`finish_reason: "repetition_loop"`）随其合并进入 `llama_mtmd.rs` 解码循环，成为本方案的**改造基线**：T2.2/T2.4 实施时保持其环检测与 `finish_reason` 语义不变；P4.1 把解码循环迁入推理线程时环检测随迁，其"取消时保留遥测"路径（m14）在 actor 遥测往返下语义不变；P4.3 合批后环检测**逐序列实例化**（每序列独立 K=256 窗口）。
- 环检测以**已生成 token 流**为输入，与 batch 分配方式、执行线程（主线程或 actor）解耦——上述适配是工程性的，无规格冲突。
- 两方案不得在同一提交混叠；本方案各阶段 rebase 到复读方案合并后的主干。
- **预算基线（Q4 决策，2026-10-31）**：复读方案先冻结——其默认输出预算 16384 与配套上下文提升即本方案的设计基线。P4 的 N × 每页预算 ≤ n_ctx 按冻结值计算（每页 ≈ 1024 图像 + ~0.2k 提示 + 16384 输出 ≈ 17.6k token；N=2 需 ~35k n_ctx，N 上限由 KV 显存实测决定，初始默认批深可能即 N=2）。复读方案最终冻结值若与 16384 不同，P4.2 以实际冻结值为准并回写本节。
- **P0 基线随排序重置**：复读方案的输出预算变更会改变逐页解码时长分布，R0.1 的对照基线必须在复读方案合并后的构建上重测（§2.1、§3 R0.1 已同步）；58.4 s 降为历史参照。

---

## 9. 风险与回滚

| 风险 | 缓解 | 回滚 |
|---|---|---|
| flash attention 语义差异导致输出漂移 | 30 页金集逐字节门禁兜底 | T2.4 单独 revert（去掉参数即回默认） |
| actor 线程 panic / 通道死锁 | panic → 通道断开 → handle `Err`（fail-loud）；panic 注入测试；`&mut self` 串行消除并发访问 | P4.1 revert（回到每页 context） |
| PdfiumOwner 请求等待悬挂 | 请求超时/通道断开 → owner poisoned → 本次 scan fail-loud 终止（checkpoint 保留可 resume）；**主线程零 PDFium 调用**（BLK-2 整改，无运行时降级） | 串行回退仅限代码 revert（编译期）；故障注入用例断言超时后主线程零 PDFium 调用、无并发渲染 |
| 重叠后 RSS/磁盘峰值上升 | K=2 冻结 + 100 页资源回归 + worker 清理范围收窄到已消费段 | 代码 revert 回串行（编译期，非运行时降级）；worker 预取关闭 |
| worker 清理范围收窄后残留页文件 | 取消/失败路径 best-effort 清理预取未消费页（m1 纪律）+ 布局 Red 用例 | — |
| ~~MSVC 17.14 高于 nvcc 12.4 官方支持范围（§5 T1.0）~~ **已排除（T1.1 实测，见 §5.5 执行记录）**：nvcc 12.4 在 MSVC 17.14 下构建通过、无告警 | 实际风险为 VS 集成组件缺失（`No CUDA toolset found`），绕开方案 `CMAKE_GENERATOR=Ninja` 已验证 | — |
| P4 合批破坏逐字节确定性/复扫一致性 | 确定性准入策略（固定批深、页序填充、无动态回填）+ 兼容键纳入批深 + 自一致性门禁（同输入同批深两遍逐字节相同） | 批深回退 1（等价单流，协议保留） |
| P4 上下文预算不足/KV 显存增长超预期 | N × 每页预算 ≤ n_ctx 联动设计 + KV 增量实测记录 + 与复读方案预算变更联动（§8.3） | 批深回退；n_ctx 回默认 |
| P4 批级故障拖垮整批在飞页 | 逐序列错误语义 + 批级故障降级策略 + 故障注入用例 | 同上 |

---

## 10. 措施-分析报告映射与预期

| 本方案 | 分析报告 §5 | 预期（待实测） | 状态 |
|---|---|---|---|
| P1（T2.1 归因 + T2.2 + T2.4） | #3 运行时热点清理 | **v2 下调**：解码段每 token 开销主要由后端决定；P1 可兑现部分集中在编码段（FA）与批复用，预期个位数百分比 | 排期（无外部依赖） |
| P2（T1.0–T1.4） | #1 CUDA 缺口闭合（=W4.3） | 小模型单流 1.5–3×；R0.5 口径验证 Vulkan 同步提交开销占比 | **T1.0/T1.1 完成 + T1.4 本机预研数据已采（2026-10-31）：加速比实测 1.26–1.38×，低于 1.5–3× 假设（§5.5）**；T1.2/T1.3 待做，正式基线待登记基准机 |
| P3 | #2 渲染/解码重叠 | 隐藏渲染段空闲，总时长趋近解码时长下限 | 排期（可与 P1/P2 并行） |
| P4（P4.1–P4.4，T2.3 归入 P4.1） | #4 多页 continuous batching | 解码段 2–4×；端到端累计 3–8×（58.4 s → 10–20 s 量级）；利用率读数显著抬升但非 KPI | **排期（v3：优先于重构，§7）** |

与 §1.1 口径一致：P1/P2/P3 提升**速度**但任务管理器利用率读数仍偏低；P4 是该读数显著抬升的唯一路径。验收只认 §3 基准协议的墙钟与 tok/s。分析报告 §3.2 的"argmax/拷回"热点表述已由 §1.3 修正 3 更正，其 §5#3 的收益预期据此打折。

---

## 11. 参考

- 诊断输入：`docs/analyze/gpu-underutilization-analysis-2026-10-31.md`
- 评审输入：`docs/review/scan-throughput-optimization-development-plan.md`（2026-10-31，BLOCK；BLK-1/MAJOR-1/2/3/MINOR-1/2 的整改落点分别为 §4 T2.3、§3 R0.5+§4 T2.1、§6.1–6.2、§6.3、§4 T2.3 验收口径、§2.3）
- 代码：`src/ocrx-runtime/src/llama_mtmd.rs`（解码循环）、`src/ocrx-runtime/src/lib.rs:57-79`（Backend 编译集契约）、`src/ocrx-cli/src/main.rs:1910-1917,1985-1994`（主线程 PDFium 前置入口）、`main.rs:2135-2166`（渲染路径选择）、`main.rs:1496-1569`（WorkerScanRenderer）、`src/ocrx-output/src/lib.rs:588-720`（扫描主循环）、`src/ocrx-pdf/src/lib.rs:1317-1480`（PdfWorkerScheduler 与段提交模型）、`lib.rs:1514-1519`（进程全局 pdfium-render 绑定）、`src/ocrx-core/src/lib.rs:16-19`（CancellationToken 共享语义）
- 发布链路：`scripts/package-windows-x64.ps1:103-116`（GPU 工件构建）、`scripts/install.ps1:27-32,153-177`（GPU 探测与变体提升）
- 绑定与内嵌 llama.cpp 源码核对：`llama-cpp-2 =0.1.152` 的 `context.rs:237-243`（get_logits 借入切片）、`context.rs:25-30`（LlamaContext 借用 model）、`model.rs:803-807`（new_context 生命周期）、`llama_batch.rs:34,113`（clear/add_sequence）、`context/params/get_set.rs:313`、`context/kv_cache.rs:103`、`sampling.rs:30,595`；`llama-cpp-sys-2 =0.1.153` 内嵌 `llama.cpp/src/llama-context.cpp:3563-3579`（get_logits 同步语义）、`llama.cpp/src/llama-sampler.cpp:806-872`（sampler 采样路径与候选拷贝）
- 数据与门禁：`docs/develop/windows-gpu-baseline-2026-08-18.md`、`docs/test/reports/quality-gate-ovisocr2-draft-v1-2026-10-30.md` §6、`docs/develop/windows-platform-development-plan.md` §0.4/§10（W4.3）

---

## 12. 修订记录

**v1（2026-10-31）**：初稿。P0–P4 五阶段；T2.1 为 sampler 替换、T2.3 为"runtime 持有 context + clear_kv_cache"、P3 以"渲染线程亲和"与"segment 目录双缓冲"为设计依据。

**评审（2026-10-31）**：`docs/review/scan-throughput-optimization-development-plan.md` 结论 BLOCK——BLK-1：`LlamaContext<'a>` 借用 `LlamaModel`，T2.3 现稿形成自引用结构无法编译；MAJOR-1：sampler 并未避免整表拷回与同步，反而新增全词表候选拷贝；MAJOR-2："全部 PDFium 调用只在渲染线程"未覆盖主线程页数/分析入口与进程全局绑定；MAJOR-3：segment 目录是临时目录（提交后即删），双缓冲目录模型与现状不符；MINOR-1：`first_token_ms` 跨页聚合是 min 口径，"均值"无数据源；MINOR-2：P2 的 backend 字段变化与"全阶段逐字节"门禁冲突。

**v2（2026-10-31）**：全部评审意见并入——

| 评审项 | 整改 |
|---|---|
| BLK-1 | T2.3 重构为**条件项**并给出推理线程所有权（actor）完整设计：所有权/加载/生成/取消/心跳/错误/遥测/Drop/单飞语义（§4 T2.3），触发条件挂 R0.5 或 P4；备选方案（session trait、self_cell）记录并给出否决理由 |
| MAJOR-1 | T2.1 重写为"归因与决策冻结"：取消 sampler 替换默认项，保留 argmax；新增 R0.5 每 token 开销归因任务（§3、§4 T2.1）；§1.2/§1.3 修正 logits 路径表述 |
| MAJOR-2 | P3 不变式升级为 **PdfiumOwner 线程拥有 scan 全程全部 PDFium 调用**（含页数/分析前置，`main.rs:1910-1917,1985-1994`），主线程经通道请求；"主线程前置+事后交接"备选记录并否决（§6.1–6.2） |
| MAJOR-3 | §6.3 按实际目录模型重写：共享根目录 + 按段页号范围清理，scheduler 提交模型不动；v1 的"双 segment 目录共存"表述作废 |
| MINOR-1 | T2.3 验收口径改为"单页 fixture 独立扫描 30 次的 `first_token_ms` 均值"；R0.4 注明跨页聚合为 min 口径 |
| MINOR-2 | §2.3 门禁拆分为同后端档（逐字节）与后端身份档（结构化/质量相似度 + `backend` 字段预期变化）；T1.4 引用对应档位 |

**v3（2026-10-31）**：范围决策——P4 由条件启动提升为**排期项**，优先于后端可扩展性重构实施（§7.1 决策记录：重构未启动、时序可调，P4 先行则重构以已具备合批能力的 runtime 为迁移输入；彼文档头部已加协调注记）。同步变更：

| 项 | 变更 |
|---|---|
| T2.3 | 条件项 → 必选前置，编号 **P4.1**，先于 P4.2–P4.4 单独合并；R0.5 归因降为度量输入（§4 T2.3） |
| §7 | 重写为排期项：决策记录、定位与预期（解码段 2–4×，端到端累计 3–8×）、任务分解（P4.1 actor / P4.2 设计文档评审门禁——上下文预算、确定性弱化与准入策略、页级语义、遥测、接口面五项冻结约束 / P4.3 runtime 合批 / P4.4 扫描侧编排）、P4 专属门禁（自一致性、resume、批级故障注入、批深回退 1 回滚） |
| §2 | 目标增加 P4（§2.1.5）；非目标改写（合批限单 job 页窗口）；§2.3 第二档扩至 P4 合批阶段 |
| §3 | 新增 R0.6 合批收益上界预估 |
| §8.1 | 顺序图更新：P1 先行合并腾出文件面，P4 依赖 P1+P0，重构排于 P4 后 |
| §8.3 | 增加与复读退化方案的预算联动（其输出预算拟提至 16384，影响 N×每页预算 ≤ n_ctx 组合） |
| §9/§10 | 增补 P4 三项风险行（确定性/上下文预算/批级故障）与批深回退路径；P4 状态改"排期" |

**v3.1（2026-10-31）**：评审补充意见 2 项并入——

| 评审项 | 整改 |
|---|---|
| MAJOR（R0.5 差分口径错配） | R0.5 的 (t2048 − t32)/(n2048 − n32) 差分按设计消去一切不随 token 数缩放的项（加载、渲染、图像编码、**页级 context 创建**、后处理），v3 稿却声称据此量化"页级 context 创建占比"——数据源与 T2.3 的 ≥2% 判定用途错配。整改：R0.5 更名"每 token 边际开销归因"并显式声明差分消去项、限定服务 T2.1/T2.2；新增 **R0.5b 页级固定开销归因**（k 页同内容 fixture 的页数差分 → 每页全成本，减去每 token 边际 × cap token 数与渲染段得页级固定开销，以单页独立运行 `first_token_ms` 为交叉校验）；T2.3/P4.1 的收益预期与回退条件数据源全部改为 R0.5b（§3、§4 T2.3、§8.1） |
| MINOR（结构体字节数） | `llama_token_data` 为 12 字节（int32 id + float logit + float p，`include/llama.h:207-211`），修正 §1.3 修正 3 原稿的"16 字节"；论证结论（sampler 替换引入每 token 全词表拷贝、属净开销）不变 |

**v3.2（2026-10-31）**：实测核验修正——CUDA Toolkit 12.4 已在本机（nvcc V12.4.131、`CUDA_PATH` 已设、驱动 610.88 未变），W4.3 与分析报告 §3.4 的"本机缺 toolkit"阻塞记录（2026-08）过时（§1.3 修正 5）。P2 的"安装 toolkit 前置依赖"取消，改为 **T1.0 构建环境核验**（含 MSVC 17.14 高于 nvcc 12.4 官方支持范围的 unsupported-compiler 风险与三条处置预案）；§5/§8.1/§9/§10 状态由 ⛔ 改排期。`windows-platform-development-plan.md` §0.2 W4.3 行与 §0.4 依赖清单已同步加注。

**v3.3（2026-10-31）**：P4 风险讨论决策两项并入——① **质量阈值实测定线**：§2.3 第二档的 P4 结构化相似度阈值不预设，以 P4.1.5 Spike A 的批分叉实测数据定线，在 P4.2 设计文档冻结；② **resume 确定性优先**：中断时在飞窗口不提交 checkpoint、恢复时整窗重扫（同页断点前后批组合一致），接受时间代价。新增 **P4.1.5 de-risk 实验**（Spike A 批分叉定量 + Spike B 多序列解码组合验证，可与 P4.1 并行，结论为 P4.2 强制输入）；§7.3/§7.4/§8.1 同步。注：Spike B 为本方案建议项（决策讨论中未单独表态），若收缩以 Spike A 为先。

**v3.4（2026-10-31）**：排序决策——复读退化方案已在分支开发中，本方案迭代排在其合并之后。§8.3 由条件式（"先合并者冻结"）改为固定顺序：复读方案的环检测/`finish_reason`/m14 语义成为本方案改造基线（T2.2/T2.4 保持、P4.1 随迁、P4.3 逐序列化）。Q4 预算联动随之落定：复读方案先冻结（默认输出 16384 + 配套上下文提升为设计基线），P4.2 ① 按冻结值计算（每页 ≈17.6k，N=2 需 ~35k n_ctx）；P0 基线须在复读方案合并后的构建上重测，58.4 s 降为历史参照（§2.1、§3 R0.1、§7.3 同步）。

**v3.5（2026-10-31）**：重审 UNBLOCK/PASS，3 项非阻断 MAJOR 并入（闭合时点均为 P4.2 设计评审）——① R0.5b 扣减基数改为**实际生成 token 数**（`RuntimeTelemetry.generated_tokens`），cap 仅为控制变量，按 cap 扣减会在页提前 EOS 时多扣、低估页级固定开销（§3）；② resume **提交原子性**：登记"整窗重扫 vs 逐页提交"的衔接缺口与两个候选机制（A 窗口原子提交/建议、B 重处理规则），冻结于 P4.2（§7.3 ③、§7.4）；③ **n_ctx_train 硬上限**：约束式改为 N × 每页预算 ≤ n_ctx ≤ n_ctx_train；实测 reference GGUF（revision `8a22290a`）`qwen35.context_length = 262144`，按 17.6k/页 得 N 上限 14，训练窗口不构成实际约束、KV 显存仍为绑定约束（§7.3 ①）。

**v3.6（2026-10-31）**：批深约束核算修正——按 GGUF 元数据（`full_attention_interval = 4`：24 层中 6 层全注意力 + 18 层线性/SSM；GQA `head_count_kv = 2`、`key_length = 256`）核算 KV ≈ 12 KB/token、每页 ≈212 MB、N=14 全预留 ≈3 GB：**VRAM 不构成约束，批深天花板即 n_ctx_train 推出的 14**；v3.5 的"绑定约束仍是 KV、初始默认可能即 N=2"表述作废——N=2 仅为 P4.3 集成起步档，默认值由 P4.4 曲线与窗口代价（断点粒度/resume 整窗重扫随 N 线性变差）共同决定，预期落点 4–8+。混合架构多序列行为仍归 Spike B 验证（§7.3 ①/P4.3）。

**v3.7（2026-10-31）**：Spike B（多序列解码组合验证）经用户确认**保留**为 P4.1.5 正式任务，v3.3 修订记录中"Spike B 为本方案建议项、决策讨论未单独表态"的注记由本条取代。

**v3.9（2026-10-31）**：T1.4 前置数据提前采集（不改代码，用户授权）——CUDA vs Vulkan 单流基线（30 页门禁集 × reference/compact × 两遍）：**加速比 1.26–1.38×（reference 57.3 s vs 71.9 s；compact 43.7 s vs 60.5 s），解码 tok/s +30–46%，低于分析假设 1.5–3×**；发现 2026-10-30 报告的 Vulkan 58.4 s 本机未复现（71.9 s，差异来源未隔离，登记基准机复跑前不作可复现常数）；非解码段 ≈9 s 与 P3 目标段吻合，P2+P3 合计端到端 ≈1.5×。新增 §5.5 执行记录与基线表（§5.5、§10）。

**v3.10（2026-10-31）**：重审 v3.9 **BLOCK**（阻断 P3）后修订，3 项全部并入——

| 评审项 | 整改 |
|---|---|
| BLK-1（P3 遗漏 `render_perturbed`） | §6.2 协议增加 `RenderPerturbed(page, numerator, denominator)` 请求：包装器实现 `render_perturbed` 转发、owner 线程内走与 `PdfScanRenderer` 相同的 `render_with_scale` 路径（RGB/audit/失败语义与串行一致）；扰动请求按需渲染不经预取；默认与 worker 路径各加 attempt 0/1/2 输入与 audit 不同的集成验收（§6.2/§6.4）。**顺带登记主干现存缺口**：`WorkerScanRenderer` 未覆盖 `render_perturbed`，worker 模式扰动重试静默失效——随 §6.3 补齐 |
| BLK-2（主线程 PDFium 回退违反单线程不变式） | 移除 §9"降级透传（主线程直接渲染）"回退；冻结 fail-loud：请求超时/通道断开 → owner poisoned → scan 以结构化错误终止（checkpoint 保留可 resume），主线程零 PDFium 调用；串行回退仅限代码 revert（编译期）；新增 owner 悬挂故障注入用例（§6.2/§6.4/§9） |
| MAJOR-1（T1.2 未承接已验证构建环境） | T1.2 增加四项脚本内可执行步骤（Ninja 生成器、vcvars64 初始化、短 `CARGO_TARGET_DIR`、重试前清缓存）并列为脚本烟测断言；Red 扩展至环境缺失 fail-loud（§5 T1.2） |

另：修复编辑事故——T1.2 小节标题在 v3.9 插入 T1.4 前置数据节时被吞（内容孤悬），已恢复标题并重写全节。**实况更新（§8.3）**：复读分支已合并进主干（`85cc1a3`，含预算对齐 `b2bfcce`），v3.4 的排序等待解除，P0/P1/P4 立即可开工；P0 基线直接在当前主干采集（输出预算已变更，58.4 s 双重过时）。

**v3.11（2026-10-31）**：重审 v3.10 **UNBLOCK/PASS**（BLK-1/2 与 MAJOR-1 确认解除，复读分支合入确认）。MINOR 收尾：§6.4 回滚行与 §9 风险表残留的"降级透传"措辞统一改为"仅限代码 revert（编译期），无运行时回退"，与 §6.2 BLK-2 语义对齐，消除被误读为运行时回退的可能。方案定稿，无未决评审意见。——硬约束为 **6 GB 显存预算**（口径：以 8 GB VRAM 为标准设备规划、预留 ~2 GB 给系统其他应用避免整机跑炸；默认值不能对齐 4090 D，多数机器达不到）：默认 N = min(吞吐曲线饱和点, 6 GB 安全上限, 窗口代价可接受点)；4090 D 实测曲线仅用于判断更大 N 的收益，不直接决定默认值。验证采用**预算断言门禁**：以 llama.cpp 加载时报告的实际分配量（权重 + KV + 计算缓冲）合计断言 ≤ 6 GB，不依赖真实 6 GB 设备（分配量由 n_ctx/N/profile 决定，须在发布后端上测）；更高批深是否随显存探测自动提升归 P4.2（复用 W1.2 探测注入模式，不新增用户旋钮）（§7.3 ①/P4.4、§7.4）。
