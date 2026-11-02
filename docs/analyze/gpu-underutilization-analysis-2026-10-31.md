# GPU 算力利用率偏低分析报告（4090D ≈5%）

日期：2026-10-31
范围：ocrx 推理链路（`ocrx-runtime` / `ocrx-cli` / `ocrx-pdf` / `ocrx-models`）的 GPU 占用形态分析。
结论状态：**诊断性分析，非决策冻结**。改进路径为建议项，未排期；涉及后端切换的建议需按 W4 门禁纪律另行实测冻结。

## 1. 现象

用户观察：OCR 推理期间，RTX 4090 D 的推理算力利用率仅约 5%（任务管理器 GPU 视图），
显存占用正常（权重确已驻留显存），整体速度可用但远低于对该卡的直觉预期。

初始疑问是"GPU 是否被正确启用"。经代码与既有实测数据核对，**GPU offload 工作正常，
低利用率是该工作负载的固有形态，非配置故障**。具体依据见 §3、§4。

## 2. 运行环境与既有实测证据

### 2.1 本机（非登记基准机）

- GPU：NVIDIA GeForce RTX 4090 D，驱动 610.88（见
  `docs/test/reports/quality-gate-ovisocr2-draft-v1-2026-10-30.md` §6）
- 后端：**Vulkan**（"最小 Vulkan 工具链"构建：系统 `vulkan-1.dll` + MSYS2 shaderc glslc）。
  CUDA feature 虽在 CLI 中声明但整条构建/发布链路无法编入——这不是本机一次性的构建
  选择，而是发布链路的结构性缺口，完整事实链见 §3.4。
- 模型：OvisOCR2 社区 GGUF，revision `8a22290a…`

### 2.2 模型规格（`src/ocrx-models/src/lib.rs:25-34`）

| profile | 量化 | 模型体积（mmproj 另加 205 MB） |
|---|---|---:|
| compact | Q4_K_M | 529 MB |
| balanced | Q5_K_M | 578 MB |
| quality | Q8_0 | 812 MB |
| reference | F16 | 1 517 MB |

即全部档位均为**亚 1B 参数量级**的小模型。这是后文分析的量级前提。

### 2.3 既有吞吐基线

| 机器 / 后端 | 场景 | 结果 | 出处 |
|---|---|---|---|
| RTX 2070 Max-Q / Vulkan | 单页 fixture，128 token 上限 | 76.2 tok/s，7.0× vs CPU | `docs/develop/windows-gpu-baseline-2026-08-18.md` |
| RTX 4090 D / Vulkan | 30 页回归集，reference 档 | 58.4 s（≈2 s/页），9.6× vs CPU 562.6 s | 质量门报告 §6 |
| RTX 4090 D / CPU | 同上 | 562.6 s | 同上 |

对亚 1B 模型而言，≈2 s/页、70+ tok/s 的单流解码速度偏低（llama.cpp CUDA 后端同量级
模型单流通常可达数百 tok/s），与"算力远未吃满"的观察自洽。

## 3. 根因分析（按影响排序）

### 3.1 解码是显存带宽瓶颈，不是算力瓶颈（负载形态，占主导）

自回归解码 batch=1 时，每生成一个 token 需将全部权重从显存读取一遍（本质 GEMV），
算术强度仅约 2 FLOP/byte。瓶颈在显存带宽（4090D ≈1 TB/s），而非算力（≈95 TFLOPS
FP16）；Tensor Core 在此负载下基本闲置。模型越小越严重：单 token 的 kernel 均为
微秒级小算子，而任务管理器的 GPU% 统计的是"有 kernel 在飞的时间占比"，kernel 之间
的空隙全部计入空闲。

**推论：即使模型换成 70B 级，单流解码的任务管理器读数通常也只有 20–60%；亚 1B 模型
读到个位数百分比是正常现象。**该指标衡量的不是"算力压榨程度"。

### 3.2 每个 token 存在 CPU 同步点，GPU 被串行化（代码热点）

解码循环位于 `src/ocrx-runtime/src/llama_mtmd.rs` 的
`LlamaMtmdRuntime::generate_with_progress`：

| 位置 | 行为 | 代价 |
|---|---|---|
| `llama_mtmd.rs:226` | 每步 `context.get_logits()` 将整个词表 logits 拷回 CPU | 每 token 一次强制同步 + DtoH 拷贝 |
| `llama_mtmd.rs:227-231` | CPU 上全词表 argmax（temperature=0，未用 llama.cpp sampler chain） | 每 token 一次 ~150k 浮点扫描 |
| `llama_mtmd.rs:269` | 每 token 新分配 `LlamaBatch::new(1, 1)` | 分配 + 绑定开销 |
| `llama_mtmd.rs:175-182` | 每页新建 `LlamaContext` | 页级开销（次要） |
| `llama_mtmd.rs:184` | prefill `eval_chunks` 批 512 | prefill 已批量化，非瓶颈 |

GPU 每算完一步即停下等待 CPU 完成"拷贝 → argmax → 提交下一批"，CPU 往返时长与
kernel 时长同量级，GPU 空窗占比高。

### 3.3 流水线单页串行，GPU 占空比天然低（结构）

主流程为：pdfium 渲染（CPU，独立 worker 进程）→ tokenize → 视觉编码（mmproj，算力
密集但占墙钟小头）→ 逐 token 解码（大头）→ 适配/裁图/PNG 落盘（CPU）。

渲染侧已做 **segment 批量预取**（`src/ocrx-cli/src/main.rs:1506-1518`，
`WorkerScanRenderer` 按 `segment_pages` 渲染一段），但渲染与推理**同步串行**：
渲染 segment 期间 GPU 空闲，解码 segment 期间渲染进程空闲，两者不重叠。

### 3.4 CUDA 路径缺口：feature 已声明、硬件有能力，但构建/发布链路无法编入（放大 §3.1/§3.2）

事实链（每一环均已核实）：

1. **cargo feature 已落地**：`cuda` 特性在 `src/ocrx-runtime` 与
   `src/ocrx-cli/Cargo.toml` 的 `[features]` 均已声明（W1.1 交付），CLI 的
   `--device` 枚举接受 `cuda`（`src/ocrx-cli/src/main.rs:196`）。
2. **硬件有能力且被正确探测**：本机 4090 D 驱动正常；`scripts/install.ps1:29-30`
   的 GPU 探测顺序为 `nvcuda.dll → cuda` **优先**于 `vulkan-1.dll → vulkan`，
   即 NVIDIA 机器的安装偏好本应是 CUDA。
3. **W4.3 未启动**：CUDA 构建在
   `docs/develop/windows-platform-development-plan.md:48` 状态 ⛔，阻塞项为本机缺
   CUDA toolkit ≥12.x（`:106`）。按计划 CUDA 是"通过则发布，未通过仅阻断 CUDA
   自身发布，不阻断 Vulkan/CPU"的可选路径（`:164`）——缺口是已知且有意的，但对
   NVIDIA 专用机器而言，它是 §5 中最大的单项性能余量。
4. **打包脚本无法产出 CUDA 工件**：`scripts/package-windows-x64.ps1:111` 硬性拒绝
   vulkan 之外的任何 `GpuPackages` 取值（`Fail "unsupported GpuPackages entry"`），
   GPU 工件仅有 `--features vulkan` 构建的 `bin/ocrx-vulkan.exe`（`:113-115`）。
5. **安装器降级安装 Vulkan**：`scripts/install.ps1:161` 在探测到 cuda 偏好的机器上，
   因只存在 `ocrx-vulkan.exe` 工件，直接把 **Vulkan 变体提升为 GPU 构建**安装
   （`:157` 附近注释明确此提升逻辑）。
6. **运行时结果**：编译集求交（`Backend::is_compiled()`，
   `src/ocrx-runtime/src/lib.rs:57-64`）后 `auto` 落到 Vulkan；显式 `--device cuda`
   得到 "requested device is unavailable" 诊断（`main.rs:1962-1969`，fail-loud
   行为本身正确）。

即：**机器是 CUDA 机器，安装器也把 CUDA 列为首选探测目标，但发布链路没有任何一环
能产出 CUDA 构建**，用户最终必然运行 Vulkan。llama.cpp 的 Vulkan 后端按节点同步
提交命令，kernel 提交开销高于 CUDA，小 batch 解码场景下进一步压低利用率与吞吐。

## 4. 判定

- **不是 GPU offload 故障**：显存驻留正常、GPU 加速比 9.6×（4090D Vulkan vs CPU）
  均证明 offload 生效。
- **不是回归**：76 tok/s（2070 Max-Q）与 ≈2 s/页（4090D）与该负载形态的预期一致，
  无既有更快基线被跌破。
- **是优化空间**：§3.2 的代码热点与 §3.4 的后端选择是可直接兑现的余量；§3.3 的
  流水线重叠与多页并发是结构性余量。

## 5. 改进路径建议（按性价比排序）

| # | 措施 | 预期收益（待实测） | 改动量 |
|---|---|---|---|
| 1 | 闭合 §3.4 的 CUDA 缺口：安装 CUDA Toolkit ≥12.x（解除 W4.3 阻塞）→ `--features cuda` 构建验证 → `package-windows-x64.ps1` 扩展 `-GpuPackages cuda`（现 `:111` 硬拒）→ `install.ps1` 按既有探测顺序优先安装 cuda 变体（探测逻辑 `:29-30` 已就绪，仅缺工件分支） | 单项最大；小模型单流解码通常 1.5–3× | 低–中（工具链一处 + 打包/安装脚本各一小改动，走 W4.3 既有门禁） |
| 2 | 渲染与解码重叠：segment N+1 渲染与 segment N 解码并行（pdfium worker 已是独立进程，仅需调度器异步化） | 隐藏渲染时段的 GPU 空闲；整体接近解码时长下限 | 中 |
| 3 | 运行时热点清理：复用 context/batch；改用 llama.cpp sampler chain（`llama_sampler_init_greedy`）替代手写全词表 argmax；开 flash attention（主要惠及 prefill） | 每 token 固定开销下降； tok/s 中个位数到两位数百分比提升 | 中 |
| 4 | 多页 continuous batching（一次送多页图像/多请求解码） | GPU 利用率与吞吐数量级提升的唯一路径 | 高（架构级，Runtime trait 需扩展为多请求接口） |

注意：#1 与 #3 提升**速度**，但单流解码的任务管理器利用率读数仍会偏低（§3.1 的物理
形态不变）；#4 才是"把卡吃满"的路径。若目标 KPI 是吞吐而非利用率读数，#1–#3 已够。

## 6. 验证方法

1. **拆分阶段耗时**：`RuntimeTelemetry`（`src/ocrx-runtime/src/lib.rs:8-24`）已记录
   `load_time_ms / first_token_ms / generated_tokens / generation_time_ms`；
   `OCRX_GPU_REPORT=1` 输出 GPU 报告。`first_token_ms` 与 tok/s 的对比可分离
   "编码+首 token"与"纯解码"占比，验证 §3.3 的结构性判断。
2. **CUDA vs Vulkan 对照**：同一 fixture、同一 profile 分别以两种 feature 构建跑
   `result.json`，比对 `output_tokens_per_sec` 与 `elapsed_ms`，量化 §3.4。装
   toolkit 后 `doctor --report json` 应报 `backend=Cuda`（对照质量门报告 §6 的
   `backend=Vulkan`），并实测 `--device cuda` 从 "unavailable" 诊断变为可用。
3. **隔离基准**：用 llama.cpp 自带 `llama-bench` 直接测当前 GGUF 的单流 tok/s，
   排除本仓库流水线（渲染/后处理）干扰，得到解码速度上界参照。
4. **观测口径**：以 `nvidia-smi dmon`（或任务管理器按引擎视图）观察解码期间的
   SM/显存控制器占用，确认瓶颈在带宽而非算力。

## 7. 参考

- 代码：`src/ocrx-runtime/src/llama_mtmd.rs`（解码循环）、
  `src/ocrx-runtime/src/lib.rs`（Backend/Telemetry）、
  `src/ocrx-cli/src/main.rs:1467-1596`（WorkerScanRenderer）、
  `src/ocrx-models/src/lib.rs:25-34`（模型规格）
- 发布链路：`scripts/package-windows-x64.ps1:103-115`（CPU 基线 + 仅 vulkan 的
  GPU 工件）、`scripts/install.ps1:29-30,157-175`（cuda 优先探测 + vulkan 变体提升）
- 数据：`docs/develop/windows-gpu-baseline-2026-08-18.md`、
  `docs/test/reports/quality-gate-ovisocr2-draft-v1-2026-10-30.md`
- 相关计划：`docs/develop/windows-platform-development-plan.md`（W4 GPU 契约，
  W4.3 CUDA 构建为 ⛔ 待 toolkit）、
  `docs/develop/model-backend-extensibility-refactoring-development-plan.md`（后端可扩展性，与本报告 §5#4 相关）
