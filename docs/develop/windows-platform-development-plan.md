# OCR CLI Windows 平台开发计划

状态：执行中（分支 `windows-platform-dev`，详见 §0 执行状态：WP/W1 完成，W2/W5 CPU 侧完成，W0/W3/W4 剩余项阻塞在外部依赖）  
日期：2026-08-11（创建）/ 2026-08-15（状态更新）  
依据：`docs/requirement/requirements-v0.1.md`、`docs/develop/technical-design-v0.1.md`、`docs/plan/v0.1-tdd-development-plan.md`、`docs/develop/runtime-macos-candidate-2026-08-02.md`  
需求追踪：`docs/requirement/requirements-v0.1.md` 第 3、13、15、16、17、18 节  
对应 macOS 基线：`docs/develop/p0-validation-macos-2026-08-02.md`、`docs/develop/local-machine-validation-2026-08-06.md`

## 0. 执行状态

更新：2026-08-15，分支 `windows-platform-dev`（worktree `.worktree/windows-tdd`）。图例：✅ 完成｜◐ 部分完成｜⛔ 阻塞（外部依赖）｜⬜ 未开始。每个合并点均通过门禁：`fmt --all -- --check`、`test --workspace`、clippy 零警告（Windows 实机）。

### 0.1 里程碑总览

| 里程碑 | 状态 | 已冻结/已交付 | 待做/阻塞 |
|---|---|---|---|
| WP 公共契约补齐 | ✅ | WP.1 缓存布局、WP.2 runtime 对齐冻结 | — |
| W0 可行性/对齐/冻结 | ◐/⛔ | W0.2 渲染大部通过 | W0.1、W0.4 缺权重；W0.3 缺基准设备 |
| W1 跨平台抽象与设备探测 | ✅ | Backend、探测、路径、recommend_device | — |
| W2 PDF 渲染、缩放与 worker | ◐ | W2.1 加载渲染、W2.3 原子写 | W2.2 真实复杂 PDF 资源采集 |
| W3 CPU 推理与裁图接入 | ◐ | 编译/纯逻辑/CLI 取消骨架 | 生成 heartbeat、页面状态机、真实 E2E |
| W4 GPU 后端 | ⛔ | — | Vulkan SDK、CUDA toolkit 缺失 |
| W5 诊断、Skill、安装包 | ✅ | doctor、Skill、package/install 脚本 | — |
| W6 发布验收 | ⬜ | — | 待前序完成 + 干净环境 |

### 0.2 详细任务状态

| 任务 | 状态 | 提交 | 说明与证据 | 下一步 / 解除条件 |
|---|---|---|---|---|
| WP.1 缓存布局对齐 | ✅ | `424d9cd` | `cache_paths(cache_root, revision, profile)` 布局 + 平台标准缓存根；`ocrx_core::RUNTIME_COMPATIBILITY` 共享契约；macOS 同步 | — |
| WP.2 runtime 对齐冻结 | ✅ | `f56f225` | llama-cpp-2 =0.1.152 / llama-cpp-sys-2 =0.1.153 + `mtmd` 在 Windows x86_64 构建通过（MSVC BuildTools 2022）；内嵌 llama.cpp `9e3b928`（文件 SHA-256 与 upstream 比对验证）；B1 guard 测试；B3 回退未触发。冻结记录：`docs/develop/runtime-windows-alignment-2026-08-14.md` | — |
| W0.1 CPU runtime 对齐 | ⛔ | — | 绑定/构建已由 WP.2 冻结 | 取得 OvisOCR2 GGUF 权重后，完成 model/mmproj 加载、生成、`llama-mtmd-cli` 结构化比对 |
| W0.2 PDFium 实机验证 | ◐ | `18e7004` | DLL 加载防护（缺失/路径逃逸/错误架构不静默成功）+ 实机渲染验证全绿：页数、300 DPI RGB8、审计尺寸、选页映射、像素预算等比缩放、损坏/非 PDF 分类 | 补加密/密码恢复实机用例；与 macOS 像素/hash 级比对 |
| W0.3 资源基线 | ◐ | `8b811b0` | 跨平台 RSS 采样器（Windows tasklist / Unix ps，纯安全实现）+ 100 页回归骨架；本机试运行 43 样本/12.4s，RSS 回落基线（峰值 40 MB / 末值 12 MB） | 16 GB CPU-only 基准设备官方测量（真实复杂 PDF、worker 页段峰值、退出状态）；与 macOS 曲线对比记录 |
| W0.4 质量回归 | ⛔ | — | — | 取得 OvisOCR2 GGUF 权重后，按需求 17.2.3 跑 BF16 对照与四 profile 评分 |
| W1.1 Backend/feature 扩展 | ✅ | `23c9cfd` | `vulkan`/`cuda` cargo 特性（保留 `mtmd`）；`Backend::is_compiled()` 编译集契约；快照与编译集求交（实测：CUDA 机器 + CPU-only 构建 → `cuda_available:false`，auto 选 CPU）；macOS 行为不变 | — |
| W1.2 探测/ResourceSnapshot | ✅ | `424d9cd` | `SystemProbe` trait 注入 + GPU 字段；doctor Windows 报告离线、不下载模型 | — |
| W1.3 平台路径解析 | ✅ | `424d9cd` | `platform_paths`（home 回退链 HOME→USERPROFILE→HOMEDRIVE+HOMEPATH、缓存/配置目录） | — |
| W1.4 recommend_device 分支 | ✅ | `424d9cd` | 优先级表数据化；显式不可用 device 拒绝；auto 回退 CPU 可审计 | — |
| W2.1 PDFium 加载与渲染 | ✅ | `18e7004` | 加载防护 + 渲染/缩放/分类（实机验证同 W0.2） | — |
| W2.2 worker 进程隔离 | ◐ | `731dae4` | 跨平台 worker 协议/调度器 + 子进程级测试（`pdfium-fake-worker`）完成 | 真实复杂 PDF 资源采集随 W0.3 测量同步补全 |
| W2.3 临时目录与原子写 | ✅ | `731dae4` | `temporary_path` 同目录原子写、`.part` 清理、宽目录删除防护 | — |
| W3.1 llama_mtmd 跨平台化 | ◐ | `f56f225`(编译) / `6444518`(契约) / `b6c9280`(CLI un-gate) | `llama_mtmd.rs` cfg 放宽至 any(macos, windows)，Windows 编译通过；CLI 层 Ctrl+C→CancellationToken、二次 130、`run_with_checkpoints_cancelled` 已存在；`Runtime::generate_with_progress` 生成循环取消轮询 + 每秒进度回调 + 共享 `CancellationToken` 已落地；Windows 真实 scan 入口已 un-gate，compact CPU 单页 E2E 冒烟通过（model/mmproj 加载、MTMD 编码、生成、result/checkpoint 输出） | 与 W0.4 质量回归一致需 reference；W3.2/W3.3 real 待图例/故障注入 fixture |
| W3.2 bbox 裁图 | ✅ | `731dae4` / `90e74ba`(Vulkan) | 纯逻辑完成（floor/ceil 映射、clamp、IoU 去重、稳定程序名原子写）+ fake 测试；真实模型 E2E 已在 Windows CPU/Vulkan 跑通，`figure-1-page.pdf` 产出 `images.count=1` | — |
| W3.3 取消/部分完成/恢复 | ◐ | 既有 / `6444518`(runtime 契约) / `e0ad31f`(状态机) | 失败继续、`partial`/10、130、checkpoint/恢复在 CLI 层已有（`failed_pages`、checkpoint 提交）；runtime 内取消契约已由 W3.1 落地；真实 scan 路径已改为显式 `PageLifecycle` 状态机，并接线 `recognize_artifact_cancelled` 与取消清理 | 真实 E2E 待权重 |
| W4.1 Vulkan 构建 | ✅ | `4017d1f`(Red) / `90e74ba`(enum) / `8622114`(fallback) | Vulkan SDK 1.4.357.0 已安装；`--features vulkan` 短 `CARGO_TARGET_DIR` 构建通过；`RealSystemProbe` 已用 `vulkano` 做安全 instance + physical-device 枚举；`run_scan` 已实现 GPU 初始化失败自动回退 CPU 并写入 warning；doctor 报告 `backend=Vulkan`、`vulkan_available=true`、`recommended_device=vulkan`；无 GPU 回退分支由注入式 `SystemProbe`/`ResourceSnapshot` 测试覆盖 | — |
| W4.2 Vulkan E2E | ◐ | `90e74ba`(Vulkan) | 本机 RTX 2070 真实 E2E 冒烟通过：`normal-1-page.pdf` partial/truncated、`figure-1-page.pdf` success 且 `images.count=1`；Vulkan 编码约 2.2s，单页总耗时约 57s（CPU 同参数约 388s） | 与 CPU 输出的结构化质量对照仍待 W0.4/质量基准 |
| W4.3 CUDA 构建 | ⛔ | — | — | 升级 CUDA toolkit 至 ≥12.x |
| W4.4 GPU 资源报告 | ◐ | `83066f6`(schema) | 本机 RTX 2070 Vulkan 基线：`figure-1-page.pdf` 总耗时约 58.5s、峰值显存 862 MB、`images.count=1`、后端 `vulkan`；`GpuResourceReport` schema/golden、`RuntimeTelemetry` 与后台 `nvidia-smi` 显存采样已接线，GPU 扫描会写 `gpu-resource.json`（load/first-token/tokens/s/vram_peak） | 官方基准设备数据仍待补 |
| W5.1 日志/隐私/doctor | ✅ | `f20ba05` | 脱敏统一收敛到 `ocrx-output::redact_sensitive`（单一来源，无平台分支调用点）；doctor 离线 | — |
| W5.2 Skill Windows 安装 | ✅ | `83da793` | `SkillScope { User, Project }`，用户级/项目级与待决策项 5 冻结结论一致；原子写 + 卸载边界测试 | — |
| W5.3 Windows 安装包 | ✅ | `c1f3ef1` | `package-windows-x64.ps1` + `install.ps1` + 脚本测试；烟测矩阵 7 项（安装 0/哈希不符 5/缺档 4/offline 6 等）、归档 20 文件 `sha256sum -c` 全过；干净环境安装烟测属 W6 | — |
| W6 发布验收 | ⬜ | — | — | 待前序完成 + 干净 Windows 环境 |

### 0.3 部分完成项细分

#### W0.2 PDFium 实机验证（◐）
- ✅ DLL 加载防护：缺失、错误架构、ABI 不兼容、路径逃逸均不静默成功。
- ✅ 实机渲染：页数、300 DPI RGB8、审计尺寸、选页映射、像素预算等比缩放、损坏/非 PDF 分类。
- ✅ 加密/密码恢复实机用例（`4017d1f`）：新增 `encrypted-1-page.pdf`，无密码失败、带密码 `pages=1`。
- ⬜ 与 macOS 在受控 fixture 上的像素/hash 级比对。

#### W0.3 资源基线（◐）
- ✅ 跨平台 RSS 采样器（Windows `tasklist` / Unix `ps`，纯安全 Rust 实现）。
- ✅ 100 页回归骨架与本地试运行（43 样本/12.4s，峰值 40 MB / 末值 12 MB）。
- ⬜ 16 GB CPU-only 基准设备官方测量窗口。
- ✅ 真实复杂 PDF 的 worker 本机采集：`complex-20-pages.pdf` exit 0、峰值 106,652 KB、末值 35,448 KB、54.1s；`normal-100-pages.pdf` exit 0、峰值 40,444 KB、末值 18,924 KB、54.1s。
- ⬜ 与 macOS 资源曲线对比记录。

#### W2.2 worker 进程隔离（◐）
- ✅ 跨平台 worker 协议/调度器实现。
- ✅ 子进程级测试（`pdfium-fake-worker`）覆盖空/倒序/越界段、异常退出、超时、取消、缺页、PNG 损坏、`.part` 残留及 hash 不符。
- ✅ 真实复杂 PDF 上的 RSS/footprint、页段峰值与退出状态本机采集（同 W0.3）。

#### W3.1 llama_mtmd 跨平台化（◐）
- ✅ `llama_mtmd.rs` cfg 放宽至 `any(macos, windows)`，Windows 编译通过。
- ✅ CLI 层 Ctrl+C→`CancellationToken`、二次 130、`run_with_checkpoints_cancelled` 已存在。
- ✅ 生成循环内取消轮询 + 每秒进度回调（`Runtime::generate_with_progress`，`6444518`）；`CancellationToken` 提升至 `ocrx-core` 共享；`FakeRuntime` 与 `LlamaMtmdRuntime` 覆盖契约并补充测试。
- ✅ Windows CPU 真实生成路径冒烟（`b6c9280`）：compact + mmproj 加载、MTMD 图像编码、贪婪生成、result.json/checkpoint/document.md 输出全通；单页正常 fixture 为 partial/truncated，质量门禁仍归 W0.4。

#### W3.2 bbox 裁图（✅）
- ✅ 纯逻辑：floor/ceil 映射、clamp、面积阈值、IoU/完全重复去重、源顺序保留、恶意 `src` 拒绝、路径逃逸防护、无效 bbox 占位移除。
- ✅ fake 测试覆盖。
- ✅ 真实模型 E2E 裁图通过（`90e74ba`/Vulkan）：`figure-1-page.pdf` 产出 `images/page-0001-figure-001.png`，result JSON `images.count=1`。

#### W3.3 取消/部分完成/恢复（◐）
- ✅ CLI 层失败继续、`partial`/10、130、checkpoint/恢复已有实现（`failed_pages`、checkpoint 提交）。
- ✅ runtime 内取消契约（同 W3.1，`6444518`）。
- ✅ 显式页面状态机复核（`e0ad31f`）：真实 scan 路径用 `PageLifecycle` 替代散落计数器，所有选页均落 `Pending/Processing/Completed/Truncated/Failed/Cancelled` 终态；取消中的当前页不提交。
- ✅ 跨平台 checkpoint 兼容键校验（`4017d1f` 后续）：Windows CPU 与 macOS Metal 的 checkpoint 不互相恢复，输入/模型/profile 相同也按 platform/backend 参数隔离。
- ✅ 真实模型 E2E 冒烟：compact CPU 两页 `varied-page-sizes.pdf` 成功；同输出 `--resume` 复用 checkpoint；单页 `--max-output-tokens 128` 得到 partial/truncated。
- ⬜ 真实模型渲染/推理失败注入与真实 Ctrl+C 信号自动化（当前由 fake 状态机/取消契约测试覆盖）。

### 0.4 待解除的外部依赖（按解锁任务数排序）

1. **OvisOCR2 GGUF 权重** → W0.1、W0.4、W3.1-Green、W3.2-real、W3.3-real。（本机已通过 hf-mirror.com 下载并校验 `compact` 与 `reference`；`balanced`/`quality` 尚未下载。）
2. **Vulkan SDK ≥1.3**（本机安装即可）→ W4.1-Green，进而 W4.2/W4.4。
3. **CUDA toolkit ≥12.x**（升级或换机）→ W4.3。
4. **16 GB CPU-only 基准设备官方测量窗口** → W0.3-Green（及 W2.2 资源采集）。
5. **干净 Windows 10/11 环境** → W6。

### 0.5 已知问题

- ocrx-pdf `real_pdf_*` 测试在 `--workspace` 全量运行中间歇性失败（`PdfiumLibraryInternalError(FormatError)`，与本次改动无关，孤立重跑稳定通过）；疑似多测试二进制并发下的 DLL 加载/页缓存竞争，若频率上升需专项调查。
- W4.1 Red 测试（未编译 Vulkan 时 `--device vulkan` 返回 `InvalidDevice`）尚未补写，可在无 SDK 环境下先行落地。
- Windows Vulkan 构建的嵌套 shader CMake 在默认 worktree 路径下触发约 250 字符路径上限；已用短 `CARGO_TARGET_DIR=E:\ocrx-target` 绕过。后续 CI/干净环境应使用短目标目录或缩短 checkout 路径。

## 1. 背景、范围与目标

### 1.1 计划产生的原因

需求文档将 Windows 列为 v0.1 首发平台之一：

- 第 3.1 节：Windows 10/11 x86_64，CPU 必须可用。
- 第 3.2 节：Windows 可选加速包含 NVIDIA CUDA 与通用 Vulkan；GPU 后端不可用或初始化失败时允许回退 CPU 并明确警告。
- 第 13.1 节：提供 Windows PowerShell 安装脚本 `install.ps1`。
- 第 15.1 节：基准设备包含 Windows x64 16 GB CPU-only 与 Windows x64 NVIDIA GPU。
- 第 16 节：`doctor` 必须报告 CUDA/Vulkan 可用性。
- 第 17.1 节：能在 macOS Apple Silicon 与 Windows x64 启动。
- 第 18 节第 4 项：P0 必须在 Windows 至少验证 CPU，优先补充 Vulkan/CUDA。

但技术方案与 TDD 开发计划为控制 v0.1 发布风险，明确把 Windows 推迟到 v0.1 之后：

- 技术方案第 1.1 节：“v0.1 仅支持 macOS 14+ Apple Silicon；Windows 与其他平台延期，不纳入本版本发布承诺和冻结门禁。”
- 技术方案第 12 节：“Windows x86_64、CUDA/Vulkan 及其他跨平台工件不属于 v0.1；后续版本重新建立独立验证和发布支持矩阵。”
- TDD 开发计划：“Linux/Windows 正式工件……明确不排入任何迭代、测试 fixture、命令或隐藏开关；macOS 以外只能保留为不执行的兼容性边界测试。”

因此当前仓库的 Windows 支持处于“需求已承诺、设计与实现已延期”的状态。本计划承接这部分被延期的 Windows 工作，把它组织成一条独立、可审计、与 macOS v0.1 冻结契约对齐的开发轨道。本计划不是对需求首发承诺的改写，而是把已确认的延期工作显式化为后续版本的可执行计划；当本计划完成时，需求第 3.1 节的 Windows 首发承诺才具备兑现证据。

### 1.2 本计划交付范围

- 在 Windows 10/11 x86_64 上以进程内 `llama.cpp` Rust binding/`libmtmd` 运行唯一模型 `ovisocr2`，CPU 为强制发布基线。
- Windows 上的 PDFium 动态库加载、逐页渲染、`pdfium-worker` 进程隔离与错误分类。
- 跨平台抽象扩展：`Backend` 增加 `Cuda`/`Vulkan`、`ResourceSnapshot` 增加 GPU 探测、`recommend_device` 增加平台分支。
- GPU 后端：Vulkan 为首选通用 GPU 路径（覆盖 AMD/Intel/NVIDIA），CUDA 为 NVIDIA 专用可选路径。
- Windows 平台路径解析（配置、缓存、Skill 目录）、`doctor` Windows 报告、`install.ps1`、Windows 安装包与发布归档。
- Windows 上的真实模型 E2E、质量回归、100 页资源回归、安装烟测与发布证据包。

### 1.3 明确不排入本计划

与 v0.1 一致，以下事项不进入本计划的任何迭代、测试 fixture、命令或隐藏开关，避免范围漂移：

- GUI、云 OCR、训练/微调、第二模型（含 HunyuanOCR）、动态 adapter/模型注册。
- 页面并发、跨文档合并、OCR 文本层回写、`ocrx clean`、语义图片命名/alt text、pipe table、遥测。
- Windows on ARM（需求第 3.3 节暂不承诺）。
- Linux 正式安装包（架构允许后续增加，但不属于本计划）。
- macOS/Windows 正式发布包的“联合签名与公证”法律流程，许可证法律最终确认仍属发布阶段外部依赖。

### 1.4 完成定义

只有同时满足以下条件，才可标记 Windows 平台支持完成：

1. 需求第 17 节的 Windows 范围内条件全部有可复现证据。
2. Windows CPU 路径须通过 W0 质量门禁与 W3 功能门禁：CPU-only 设备可完成完整扫描、质量回归与 100 页资源回归；CPU 为强制发布基线，GPU 任何情况不得替代 CPU 门禁。
3. Vulkan 路径若发布则必须通过真实硬件 E2E 与 CPU 输出一致性比对；未通过的 Vulkan 仅阻断 Vulkan 自身发布，不阻断 CPU 发布。
4. CUDA 路径为可选：通过则发布，未通过仅阻断 CUDA 自身发布，不阻断 Vulkan/CPU 发布。
5. 所有 macOS v0.1 已冻结的公共契约（`PageDocument`、manifest、错误 JSON、checkpoint/result、JSONL、stdout/stderr 纪律、退出码）在 Windows 上行为一致；跨平台 checkpoint 不得互相恢复的设计在 Windows 上被验证。
6. 自动化门禁、真实模型 E2E、质量基准、资源报告、安装包烟测在 Windows 干净环境均通过；任何 P0 硬门禁失败均停止进入后续阶段，而不是以降级功能绕过。

## 2. 当前基线与代码现状

### 2.1 已存在的可复用工程基线

- Rust workspace 及 8 个 crate（`ocrx-cli`、`ocrx-core`、`ocrx-runtime`、`ocrx-pdf`、`ocrx-models`、`ocrx-output`、`ocrx-ovisocr2`、`ocrx-skill`）。
- workspace `version = "0.1.0"`、`edition = "2024"`、`rust-version = "1.85"`、`rust-toolchain.toml` 锁定 `1.97.1`。
- `ocrx-pdf` 的 PDFium RGB8 渲染、错误分类、PNG 落盘、像素预算缩放与独立 `pdfium-worker` 候选（macOS 已冻结协议）。
- macOS Apple Silicon 上的 `llama-cpp-2 =0.1.152` + `llama-cpp-sys-2 =0.1.153`（`mtmd` 特性）runtime 候选，`llama.cpp b10210` 冒烟通过。
- 仓库内确定性 PDF fixture、OvisOCR2 参考 fixture、评分器与 30 页真值集 schema（macOS 上仍待最终冻结，但 fixture 与 schema 可跨平台复用）。
- 规范 Skill 与 `validate-skill.py`、`install.sh`、`package-macos-arm64.sh` 等脚本骨架。

### 2.2 Windows 相关代码现状（缺口）

以下为截至 2026-08-11 仓库的实际状态，是本计划必须补齐的起点：

| 位置 | 现状 | Windows 缺口 |
|---|---|---|
| `src/ocrx-runtime/Cargo.toml` | `metal` 特性映射 `llama-cpp-sys-2/metal`；`llama-cpp-2`/`llama-cpp-sys-2` 依赖被 `cfg(target_os = "macos")` 门控，仅 `mtmd` 特性 | 无 Windows target 依赖、无 `vulkan`/`cuda` 特性、无 Windows 构建路径 |
| `src/ocrx-runtime/src/lib.rs` | `Backend { Cpu, Metal }`；`Runtime` trait；`FakeRuntime` | 缺 `Cuda`/`Vulkan` 变体；无 Windows 真实 runtime |
| `src/ocrx-runtime/src/llama_mtmd.rs` | `#[cfg(target_os = "macos")]` 门控，含 Metal 与 CPU 子路径 | 未跨平台化，Windows CPU/GPU 路径未实现 |
| `src/ocrx-models/src/lib.rs`（结构体）/ `src/ocrx-cli/src/main.rs`（探测） | `ResourceSnapshot { memory_gb, metal_available, cpu_available }`；`recommend_device` 仅匹配 `auto/metal/cpu`，`cuda`/`vulkan` 返回 `InvalidDevice` | 缺 GPU 探测字段与平台分支；`resource_snapshot()` 内存采集在 CLI 层且为 macOS 专用 |
| `src/ocrx-cli/src/main.rs` | `--device` 枚举接受 `auto\|cpu\|metal\|cuda\|vulkan`；`resource_snapshot()`/`doctor` 输出 `metal_available` | 枚举值被接受但运行时无法使用；探测与 doctor 为 macOS 专用 |
| `src/ocrx-pdf` | `pdfium-render 0.9.3` + PDFium 7961，macOS arm64 `libpdfium.dylib` | 无 Windows `pdfium.dll`、无 DLL 加载路径策略、worker 未在 Windows 验证 |
| `scripts/install.sh` | 硬拒绝非 Darwin/arm64 | 无 `install.ps1`、无 `package-windows-x64` 脚本 |
| `scripts/macos-*` | 全部 macOS 专用 | 无 `windows-*` 对应脚本 |
| 缓存/配置路径 | 设计要求“平台标准用户缓存目录” | 需 Windows `%LOCALAPPDATA%`/`%APPDATA%` 解析与测试 |

### 2.3 关键对齐风险

- **Rust binding 跨 commit 对齐**：macOS `llama-cpp-sys-2 0.1.153` 内嵌 `llama.cpp` commit `9e3b928...` 与验证过的 CLI `b10210` commit `000547...` 不是同一 commit，macOS 基线文档已明确该组合只是 candidate、未最终对齐（见 `docs/develop/runtime-macos-candidate-2026-08-02.md`）。Windows 不得直接假设该组合在 Windows 构建等价；WP.2 在 W0 启动前先冻结 Windows 与 macOS 共享的 commit/binding 组合，并预决策 fork/最小内部 FFI 回退，避免把该风险推迟到 W0 失败后再回退。
- **PDFium Windows 工件来源**：macOS 使用 chromium-7961 arm64 dylib。Windows 需等价 revision 的 x86_64 `pdfium.dll`，来源与许可证需在 W0 冻结；`pdfium-render` 本身跨平台，但动态库加载与 DLL 搜索路径策略需 Windows 专门处理。
- **CPU 路径质量一致性**：`temperature=0`、关闭 thinking 理论上使输出确定，但 llama.cpp CPU 后端在不同平台可能因浮点累加顺序产生微小差异。**注意：macOS v0.1 不做 CPU fallback（仅 Metal），不存在可作基准的 macOS CPU reference**；W0.4 的门禁基准是需求 17.2.3 的同 revision 官方 Safetensors BF16 与 Windows CPU 上 GGUF `reference` 的下降（NED `0.01`、公式 `0.02`、TEDS `0.02`、阅读顺序 `0.01`、bbox `0.02`），为单平台门禁；macOS Metal 结果仅作跨平台一致性参考，不作门禁，跨 backend 偏差分析只在 CPU-vs-CPU 或 Metal-vs-Vulkan 对等条件下进行。

## 3. 通用 TDD 与工程规则

### 3.1 测试组织

沿用 v0.1 TDD 计划的分层：单元测试覆盖纯逻辑；跨 crate 的稳定 JSON、Markdown、CLI 文本使用 golden；文件系统、子进程、下载与 runtime 使用组件/集成测试；真实模型测试与 fake runtime 测试分层。Windows 专属测试在对应 crate 下以 `#[cfg(target_os = "windows")]` 或独立 `tests/windows/` 目录组织；跨平台契约测试必须能在两个平台运行，断言行为一致而非断言平台特定值。

fixture 复用 macOS 已建立的确定性 PDF fixture、OvisOCR2 参考 fixture 与 30 页真值集；不得为 Windows 单独构造会与 macOS 结果不可比的 fixture。任何 Windows 专属 fixture 必须可离线运行并记录 SHA-256、来源与跳过条件。

### 3.2 每项工作的固定循环

1. 列出涉及的需求章节、输入、预期输出、退出码与非目标。
2. 新增一个最小失败测试（或先更新 golden 使之失败）；先确认其因待实现行为失败，而非测试环境错误。
3. 仅实现让测试通过的最小生产代码；禁止顺手增加未计划参数、模型、平台分支或输出字段。
4. 执行相关 crate 测试、workspace 格式化/lint 与受影响集成测试。
5. 行为稳定后重构，重跑同一测试集合；稳定 JSON/CLI 输出变化必须先更新版本策略并评审。
6. 在计划勾选项中记录测试名、命令、结果与证据路径。

### 3.3 每个合并点的最低门禁

- `cargo fmt --all -- --check`。
- `cargo test --workspace`（Windows target 上运行；真实模型标记测试按显式环境变量运行）。
- 受影响 crate 的 Clippy，新增 warning 不得引入。
- 无网络单元/契约测试。
- 对 CLI/JSON 改动执行 stdout/stderr 捕获测试。
- 对文件输出改动执行临时文件、原子 rename 与路径逃逸测试。
- 跨平台契约测试在 macOS 与 Windows 均通过（CI 双平台矩阵）。

## 4. 依赖顺序

```text
WP 公共契约补齐（W0 前置）
   (cache 布局修正 / runtime commit 对齐冻结)
        │
        v
W0 Windows 可行性/对齐/冻结
   (CPU runtime 对齐 / PDFium / 资源 / 质量基线)
        │
        v
W1 跨平台抽象与设备探测 (Backend / 探测 / 路径)
        │
        ├──────────────┬──────────────┐
        v              v              v
   W2 PDF/worker   W3 CPU 推理    W4 GPU 后端
   (CPU 管线)      (CPU 管线)     (Vulkan/CUDA, 可选)
        │              │              │
        └──────┬───────┘              │
               v                      v
        W5 打包/doctor/Skill      W5.3 GPU-aware
        (CPU 安装包,不依赖 W4)    安装检测(依赖 W4)
               │                      │
               └──────────┬───────────┘
                          v
                    W6 发布验收与证据包
```

WP 先于 W0 补齐 macOS 与 Windows 共享的缓存布局与 runtime 对齐契约（详见 §5）；W0 及后续阶段只能依赖前序已冻结的公共契约。W1 冻结的 `Backend`/`ResourceSnapshot`/`recommend_device` 跨平台契约必须先于 W2/W3/W4 接入；不得让 PDF、输出或 CLI 模块因平台分支而绕过已冻结的领域类型。

**CPU 管线与 GPU 管线解耦**：W2 与 W3 是 CPU 强制路径，二者汇入 W5 的 CPU 相关子项（W5.1 doctor、W5.2 Skill、W5.3 CPU 安装包），这些子项依赖 W1/W2/W3 冻结的契约，**可与 W4 并行**，不依赖 GPU 后端。W4（GPU）从 W1 契约分支，仅 W5.3 中 GPU-aware 的安装检测依赖 W4。因此 W4 延期或 Vulkan/CUDA 失败**不得阻塞 CPU-only 发布包**；W6 在 W5 完成后即可对已通过的 CPU（及任一已通过 GPU）路径执行发布验收。

## 5. 阶段 WP：公共契约补齐（W0 前置）

目的：W0 及之后所有 Windows 阶段都建立在 macOS v0.1 “已冻结”公共契约之上，但有两处共享契约当前在 macOS 侧本身未达标或未最终冻结--缓存布局不符合需求第 13 节/技术方案规定（B4）、runtime commit/binding 组合只是 candidate 未最终对齐（B2）。这两项是 macOS 与 Windows 共享的契约，若不先补齐，W0 的入口门禁（W0.1 对齐、W1.3 缓存路径）将建在不可信地基上。WP 不实现 Windows 专属功能，只把这两处共享契约修正并冻结，且同步更新 macOS 侧代码与测试。**WP 必须先于 W0.1-W0.4 完成；WP 未完成不得启动 W0。**

### WP.1 缓存布局对齐（B4）

- Red：为 `cache_paths` 编写布局测试，断言产出 `<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf` 与 `<cache_root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf`（mmproj 跨 profile 共享，位于 `<revision>` 层）；缺 `ocrx/models` 或 `<revision>` 层即失败；不同 `revision` 产生独立目录、`..`/绝对路径 revision 被拒绝（路径逃逸）。
- Green：修正 `src/ocrx-models/src/lib.rs` 的 `cache_paths` 签名为 `cache_paths(cache_root, revision, profile)`（`revision` 取自已有的 `manifest.identity.revision`，无需新增字段），产出 `<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf` 与 `<cache_root>/ocrx/models/ovisocr2/<revision>/mmproj.gguf`；同步更新全部 8 个调用点：`ocrx-models/src/lib.rs:180/201/224/448/510`、`ocrx-cli/src/main.rs:527/948/1587`，以及对应测试 fixture（`lib.rs:925/927/936`）；仅传 `profile` 的调用点按以下方式取 `revision`：有 manifest 在作用域的（`lib.rs:201/224/448/510`）直接用 `manifest.identity.revision`；`verify_local_state`（`lib.rs:180`）所用的 `LocalModelState`（`lib.rs:128-134`）当前无 `revision` 字段，须为其补 `revision: String` 使本地状态自描述，或改 `verify_local_state` 增参；`main.rs:527/948/1587` 仅持 `&profile`，须先按 profile 解析 manifest 取 `identity.revision`（`main.rs:527` 紧邻的 `community_manifest(&profile)` 即可，调用顺序前移）。上述行号供定位，实施时以编译错误为准。同时将 `OCRX_MODEL_CACHE` 默认值从相对 `.ocrx-model-cache` 改为平台标准缓存根（macOS `~/Library/Caches`、Windows `%LOCALAPPDATA%`；`<cache>` 为根，`cache_paths` 追加 `ocrx/models/...`，`OCRX_MODEL_CACHE` 仅覆盖根），满足需求“不放入当前项目”；同步 macOS 安装/验证测试。
- Refactor：`cache_paths` 保持纯函数跨平台一致，平台差异只在 `cache_root` 的解析（W1.3）。
- 验收：macOS 与 Windows 共用同一 `cache_paths` 实现，布局严格符合需求第 13 节；不同 revision/量化/mmproj 不复用会产生歧义的缓存项；`OCRX_MODEL_CACHE` 默认值为平台标准缓存根（macOS `~/Library/Caches`、Windows `%LOCALAPPDATA%`），`cache_paths` 追加 `ocrx/models/...`，满足“不放入当前项目”。

### WP.2 runtime commit/binding 对齐冻结（B2）

- Red：为 runtime 兼容性校验编写测试，断言 Windows 与 macOS 共享同一 compatibility 类型与 commit/binding 断言，不散落平台专用常量；`validate_runtime_adapter_compatibility` 的硬编码版本检查与最终冻结组合一致；增加“Windows target 未启用 `mtmd` 时不编译”的失败用例（B1，从 W0.1 前移到此处，因 WP.2 先于 W0.1 添加 Windows target 依赖）。
- Green：首先为 `cfg(target_os = "windows")` 在 `ocrx-runtime/Cargo.toml` 添加 `llama-cpp-2`/`llama-cpp-sys-2` 依赖，features 至少含 `mtmd`（这是 WP.2/W0.1 能在 Windows 构建的前置，不得推迟到 W1.1，否则形成 WP->W0->W1 的循环依赖）；随后在 Windows x86_64 上以 `mtmd` 构建通过并记录 wrapper/sys 版本、内嵌 `llama.cpp` commit；形成 `docs/develop/runtime-windows-alignment-<YYYY-MM-DD>.md` 冻结记录，明确 Windows 与 macOS 共享的 commit/binding 组合。
- 决策（预置）：若不存在同时满足 macOS Metal 与 Windows CPU/Vulkan/CUDA 的单一 `llama-cpp-2`/`llama-cpp-sys-2` 版本，WP.2 即启动回退方案并冻结选择，而非把该决策推迟到 W0 失败后回退。两种回退路径的工程约束（B3，workspace `unsafe_code = "forbid"`）：**固定 fork** 作为外部 git/registry 依赖，不引入 workspace `unsafe`；**最小内部 FFI** 必须新建独立 crate（如 `ocrx-llama-sys`），且因 rustc 的 `forbid` 不可被后续同 lint 级别覆盖（“forbid … cannot be overridden to be anything lower”，“forbid still trumps everything regardless of ordering”），源码级 `#![allow(unsafe_code)]` 对 workspace 传入的 `-F unsafe_code` 无效并会报错，member `[lints.rust]` 高优先级 `allow` 同样无法覆盖；唯一可行机制是该 crate 在自身 `Cargo.toml` 以 `[lints] workspace = false` 完全脱离 workspace lint 继承，再设本地 `[lints.rust] unsafe_code = "allow"`（代价：该 crate 不再继承 workspace 其余 lint，最小 FFI 垫片可接受，或按需本地复制所需 lint）。若项目决定任何 crate 均不得使用 unsafe，则删除“内部 FFI”选项，仅保留“固定 fork / 继续寻找对齐版本”。
- Refactor：版本比较收敛到共享 compatibility 类型，macOS 与 Windows 共用 manifest/兼容矩阵，差异只在构建特性与探测。
- 验收：`runtime-windows-alignment-*.md` 冻结记录存在；Windows 与 macOS 共享同一 commit/binding 组合或已冻结 fork/FFI 方案；W0.1 的“同 commit `llama-mtmd-cli` 结构化比对”基准已明确（比对 sys 内嵌 commit `9e3b928...` 的 reference，接受结构化一致而非逐字节相等，与 macOS 候选冻结口径一致）。

## 6. 阶段 W0：Windows 可行性、对齐与冻结

目的：补齐 Windows 上虽已有候选方向、但尚未达到发布冻结证据的 P0 项。此阶段产出证据和锁定物，不实现完整 CLI 业务功能。**W0 是整个 Windows 轨道的质量门禁（入口）：CPU 路径无法在 Windows 稳定生成合格输出之前，W1 之后的所有阶段不得宣称完成。** WP（§5）补齐共享契约后方可启动 W0；W0 不再承担 macOS 债务补齐，只做 Windows 可行性验证。

### W0.1 Windows CPU runtime 对齐

- Red：为 Windows target 编写解析测试，拒绝不匹配的 `llama.cpp` commit、`llama-cpp-2`/`llama-cpp-sys-2` 版本、`libmtmd` ABI 与 GGUF/mmproj revision；与 macOS 共享同一 compatibility 类型，不散落 Windows 专用常量。（“Windows target 未启用 `mtmd` 时不编译”的失败用例已前移到 WP.2，因 Windows target 依赖在 WP.2 添加。）
- Green：Windows target 依赖与 commit/binding 组合已由 WP.2 冻结，本步只做 runtime 行为对齐：在 Windows x86_64 上以 `mtmd` 跑通 model/mmproj 加载、MTMD 编码、`eval_chunks`、采样、EOS/max-token/取消与 CPU 初始化失败预期；实现只读校验器复用 macOS 兼容矩阵类型。`vulkan`/`cuda` 特性映射由 W1.1 在此基础上增加，不得移除 `mtmd`。
- Green：用锁定输入在 Windows 上跑通 `llama-mtmd-cli` 与 Rust binding 的结构化输出比对 golden；覆盖 EOS、max-token、取消与 CPU 初始化失败预期。
- Refactor：将版本比较收敛到共享 compatibility 类型；Windows 与 macOS 共用同一 manifest/兼容矩阵，差异只在构建特性与探测。
- 验收：Windows CPU runtime 冒烟通过；Rust 路径与 sys 内嵌 commit 的 `llama-mtmd-cli` 输出在结构化比对上一致（接受结构化一致而非逐字节相等，与 macOS 候选冻结口径一致）；`ovisocr2` 以外 ID 无法进入 Windows 下载或 runtime 初始化路径。

### W0.2 Windows PDFium 工件与渲染边界

工件来源已冻结（见 `docs/develop/pdfium-windows-release-baseline-2026-08-11.md`）：bblanchon/pdfium-binaries `chromium/7961` 的 `pdfium-win-x64.tgz`，`bin/pdfium.dll` SHA-256 `d3d9f4b7c9dabe3363f30779c5c3c715c47332749fa590e4b4a2b8b6780cb1c4`，版本 152.0.7961.0，BSD-3-Clause 可再分发。本阶段只做实机验证，不重选来源。

- Red：为 Windows PDFium 加载编写组件测试：DLL 缺失、错误架构（x86 DLL 在 x64 进程）、ABI 不兼容、加载路径逃逸均不得静默成功。
- Green：在 Windows x64 上以 `pdfium_7881` feature 绑定 `bin/pdfium.dll`，复现 macOS 的向后 ABI 兼容使用方式；建立 DLL 随归档发布的策略。
- Green：在 Windows 上验证 300 DPI RGB8 渲染、不同页面尺寸、加密/密码恢复、损坏/非 PDF 分类与 PNG 落盘，结果与 macOS fixture 在像素与 hash 层面可比。
- Refactor：PDFium binding 生命周期与加载策略集中在 `ocrx-pdf`，平台差异不泄漏到模型、checkpoint 或 Markdown 层。
- 验收：Windows PDFium 基础行为门禁覆盖正常、长文档、不同页面尺寸、损坏、非 PDF、加密、密码恢复与缺库；与 macOS 输出在受控 fixture 上一致。

### W0.3 Windows 资源基线

- Red：建立 Windows RSS/footprint 采样测试骨架；断言采样器在 Windows 上能返回进程级内存，而非像 macOS 沙箱曾出现的无法读取 RSS。
- Green：在 Windows x64 16 GB CPU-only 基准设备上对 100 页 fixture 与真实复杂 PDF 采集 RSS/footprint、耗时、worker 页段峰值与退出状态；验证 worker 退出回收内容相关 PDFium 缓存。
- Refactor：资源采样器跨平台抽象，平台专用采集（Windows `GetProcessMemoryInfo`/`GlobalMemoryStatusEx`）只存在于探测层。
- 验收：资源报告证明 100 页处理无随页数线性增长；PDFium 只在 worker 进程内初始化一次；与 macOS 资源曲线的差异被记录但二者均满足“非线性增长”门禁。

### W0.4 Windows 质量回归

- Red：为 Windows 质量回归编写报告 schema golden，断言样本数、逐页结果、模型/runtime/adapter/prompt/基准版本齐全。
- Green：在 Windows CPU 上对 GGUF reference/quality/balanced/compact 在同一回归集、200/250/300 DPI 与固定像素预算下运行评分，并与官方 Safetensors BF16 模型在外部生成的参考输出比对（BF16 参考输出是预先用官方 BF16 权重跑出的基准，非 `ocrx` 运行时目标，CLI 不直接加载 BF16 Safetensors）；记录原始输出与资源数据。
- Green：以需求 17.2.3 为门禁基准：同 revision 官方 Safetensors BF16 输出与 Windows CPU 上 GGUF `reference` 的下降（NED `0.01`、公式完整率 `0.02`、TEDS `0.02`、阅读顺序 `0.01`、bbox `0.02`）；各 profile 相对 `reference` 的下降是否满足阈值。macOS Metal 结果仅作跨平台一致性参考，不作门禁；如需平台间偏差分析，只在 CPU-vs-CPU 或 Metal-vs-Vulkan 对等条件下进行并明确可接受上限。
- Refactor：评分器只接受统一 `PageDocument`/真值，不读取模型原始标签，也不因平台或 profile 改变计算规则。
- 验收：Windows CPU 上 reference、quality、compact 与最终默认 profile 满足需求 17.2；任何未达标结果为 go/no-go 阻断，不能由 GPU 后端或隐藏 fallback 代替。

## 7. 阶段 W1：跨平台抽象与设备探测

目的：在真实 Windows OCR 前扩展已冻结的领域与探测契约，使 W3/W4 能以统一端口接入。此阶段不改变 macOS 已冻结行为，只增加 Windows/GPU 分支。

### W1.1 `Backend` 与 runtime trait 扩展

- Red：为 `Backend` 增加 `Cuda`/`Vulkan` 变体编写枚举与序列化 golden；断言编译进二进制的 backend 集合由 cargo feature 决定，未编译 backend 不得被 `auto` 选中。
- Green：在 `ocrx-runtime` 增加 `vulkan`/`cuda` 特性，分别映射 `llama-cpp-sys-2/vulkan`/`llama-cpp-sys-2/cuda`，且必须保留 `mtmd`（B1：Windows target 的 `llama-cpp-2`/`llama-cpp-sys-2` 依赖在 WP.2 已以 `mtmd` 启用，此处新增 `vulkan`/`cuda` 不得移除 `mtmd`，否则 `llama_mtmd.rs` 在 Windows 不编译）；保留 `metal` 为 macOS 专用。`RuntimeInfo`/`Backend` 在 Windows 上只暴露已编译且初始化可用的 backend。
- Refactor：backend 选择逻辑集中到推荐器，runtime crate 只报告能力，不自行决定优先级。
- 验收：macOS 行为不变；Windows 上 `Backend` 枚举与编译特性一致；未编译 backend 在 `doctor`/`recommend_device` 中显式标记为不可用。

### W1.2 `ResourceSnapshot` 与平台探测

- Red：表驱动测试覆盖 `ResourceSnapshot` 新增 `cuda_available`/`vulkan_available` 字段、Windows 内存采集（`GlobalMemoryStatusEx`）、GPU 可用性探测与显式 device 不可用拒绝。
- Green：实现 Windows 内存/磁盘采集、GPU backend 探测（Vulkan instance/CUDA device count）；探测可注入，测试不依赖真实 GPU。
- Green：实现 `doctor --report json` 与 `model recommend` 在 Windows 上的平台/backend/内存/缓存/Skill 报告；测试断言不创建 HTTP client、不下载模型。
- Refactor：以 trait 注入系统信息、文件系统和时钟，使推荐测试不依赖本机硬件。
- 验收：Windows 低于 6 GB 拒绝；8/16/24 GB+ 选择遵循冻结后的质量状态；显式不可用 GPU device 返回 6，`auto` 回退 CPU 的 warning 可审计。

### W1.3 平台路径解析

- Red：为 Windows 配置/缓存/Skill 路径编写测试：`%LOCALAPPDATA%`/`%APPDATA%` 解析、缺失环境变量回退、路径仍由受信类型与程序生成片段构建、模型输出/远端文件名不得作为路径；增加 Skill `home` 解析测试（`HOME` 未设置时从 `USERPROFILE` 解析、`USERPROFILE` 亦缺失时回退 `HOMEDRIVE`+`HOMEPATH`、无任何 home 变量时明确报错而非回退当前目录）与项目级 scope 测试（B5，对应待决策项 5）。
- Green：实现 Windows 平台标准配置目录（配置）与缓存目录（模型）解析，复用 WP.1 修正后的 `<cache>/ocrx/models/ovisocr2/<revision>/...` 布局（macOS 侧同源）；v0.1 只允许创建 `ovisocr2` 目录。同时实现 Skill `home` 跨平台解析（`dirs::home_dir()` 或 `USERPROFILE`->`HOMEDRIVE`+`HOMEPATH` 回退），替换 `main.rs` 仅读 `HOME` 的现状（B5）。
- Refactor：路径解析差异只存在于平台适配层，`cache_paths` 等纯函数保持跨平台一致。
- 验收：Windows 缓存布局与 macOS 一致；损坏、遗漏、错 revision 资产均不可报告为安装完成；路径逃逸测试在 Windows 通过。

### W1.4 `recommend_device` 平台分支

- Red：为 `recommend_device` 增加平台优先级测试：macOS Metal 后 CPU；Windows 可用已发布 GPU backend 后 CPU，显式 GPU 失败返回 6，`auto` 失败回退 CPU 并 warning。
- Green：实现平台分支与 GPU 优先级；`cuda`/`vulkan` 仅在对应 backend 编译且探测可用时可选，否则返回 `InvalidDevice`/`DeviceUnavailable`。
- Refactor：平台优先级表数据化，不在分支里硬编码业务判断。
- 验收：macOS 行为不变；Windows 上 `--device cuda`/`--device vulkan` 在 backend 不可用时返回可诊断错误而非静默成功；`auto` 在无 GPU 时回退 CPU。

## 8. 阶段 W2：Windows PDF 渲染、缩放与 worker

目的：将 macOS 已冻结的 `pdfium-worker` 协议接入 Windows，不改变 P1-P3 的公共契约。

### W2.1 PDFium Windows 加载与渲染

- Red：组件测试覆盖 Windows 实际 PDF 的页数、选页映射、RGB8、300 DPI、原始/渲染/送模尺寸记录，及超过 `2880×2880` 总像素预算时等比缩放、不裁页。
- Green：在 Windows 上以 DLL co-location 或受控搜索路径加载 `pdfium.dll`；`Pdfium` binding 进程内只初始化一次并由扫描任务复用；渲染位图用于后续裁图，送模图单独缩放。
- Green：实现 Windows 上加密/损坏/非 PDF/DLL 加载错误映射，保持 PDFium binding 生命周期边界。
- Refactor：平台加载差异封装在 `ocrx-pdf`，错误分类复用 macOS 已冻结类别。
- 验收：既有 PDF fixture 与真实抽样在 Windows 稳定；缩放 warning、页面尺寸与页段审计进入 checkpoint/result。

### W2.2 Windows `pdfium-worker` 进程隔离

- Red：补齐 worker 协议在 Windows 的测试：空/倒序/越界段、异常退出、超时、取消、缺页、页名错误、PNG 损坏、`.part` 残留及 hash 不符均不得提交页段；Windows 进程退出码与信号语义与 macOS 的差异被显式处理。
- Green：在 Windows 上由主进程按 `[start,end)` 连续半开区间调度默认 20 页的 `pdfium-worker`；worker 只渲染 RGB8/300 DPI 至页段私有临时目录，主进程验证后再提交。
- Green：在 Windows 多份真实复杂 PDF 和 100 页 fixture 上采集 RSS/footprint、耗时、页段峰值和退出状态，验证 worker 退出回收内容相关 PDFium 缓存。
- Refactor：worker IPC/文件协议封装在 `ocrx-pdf`，Windows 与 macOS 共用协议，平台差异只在进程启动与临时目录。
- 验收：资源报告证明 Windows 100 页处理无随页数线性增长；PDFium 只在 worker 进程内初始化一次；错误分类稳定。

### W2.3 Windows 临时目录与原子写

- Red：测试 Windows 上 PNG、page Markdown、page JSON、最终 Markdown/result 的临时文件、原子 rename 与 `.part` 清理；断言 rename 跨卷失败（临时目录与目标不同卷）被处理。
- Green：确保 Windows 临时文件与目标文件同目录以避免跨卷 rename；原子写复用 `ocrx-output` 已冻结实现，必要时为 Windows `MoveFileEx` 语义调整但不改变契约。
- Refactor：原子写与 output-root capability 校验集中在 `ocrx-output`，平台差异不外泄。
- 验收：Windows 上半成品不被视为完成；删除操作拒绝输入路径、缓存根、工作区根和宽泛目录。

## 9. 阶段 W3：Windows CPU 推理与裁图接入

目的：将已冻结的 runtime 基础设施接入 W1/W2 的端口，完成 Windows CPU 强制路径。**这是 Windows 发布的功能门禁（发布前）：CPU-only 设备必须能完成完整扫描；该门禁独立于 W0 质量门禁，二者均为 CPU 发布的必要条件。**

### W3.1 `llama_mtmd` 跨平台化与 CPU 路径

- Red：为 Windows CPU 路径编写 runtime 探测、加载、MTMD 编码、生成与安全释放的预期测试；增加 EOS、max-token、取消、CPU 初始化失败的预期测试。
- Green：将 `llama_mtmd.rs` 的 `#[cfg(target_os = "macos")]` 门控收敛为“Metal 子路径 macOS 专用、CPU/GPU 子路径跨平台”；在 Windows 上实现 model/mmproj 加载、image chunk 编码、`eval_chunks`、采样、stop/EOS、max tokens、关闭 thinking 与 prompt/chat template。
- Green：生成循环向取消令牌轮询，至少每秒发出可见 heartbeat/page_progress；Windows 控制台 Ctrl+C 映射到 `CancellationToken`（第二次立即退出 130），处理 Windows 信号语义与 macOS 的差异。
- Refactor：runtime crate 内 Metal/Vulkan/CUDA/CPU 共享加载与生成骨架，后端差异只在初始化与 offload 选项。
- 验收：Windows CPU 真实生成路径通过；runtime lock 可复现；CPU 路径输出与 W0.4 质量回归一致。

### W3.2 真实 E2E 与 bbox PNG 输出

- Red：以 fake image 和确定 bbox 测试 floor/ceil 映射、clamp、面积阈值、IoU/完全重复去重、源顺序、恶意 `src`、路径逃逸和无效 bbox 占位移除（跨平台契约测试在 Windows 重跑）。
- Green：接入一次任务加载的 Windows CPU runtime，按页执行 encode/generate/parse，完成后释放页面图像、编码上下文与临时 tensor；推理错误记录页面失败。
- Green：仅在 `<output>/images` 以稳定程序名原子写 `page-NNNN-figure-NNN.png`，Markdown 写相对链接。
- Refactor：裁图实现与 adapter 完全解耦，裁图只接收验证后的统一 bbox 和未压缩位图。
- 验收：Windows CPU 上真实模型 E2E 至少成功裁出一个 fixture 插图；全部 Markdown 图片链接存在；无效 bbox 不产生任意路径写入或越界访问。

### W3.3 取消、部分完成与恢复

- Red：Windows 上 fake/真实 runtime 依次注入渲染失败、推理失败、空输出、max-token、解析失败、写入失败和 Ctrl+C；断言逐页原因/建议、最终 status/计数和退出码。
- Green：默认继续后续页；失败或截断最终为 `partial`/10。第一次 Ctrl+C 停止取新页、清理当前未提交临时文件、保留完成 checkpoint、更新 job 并返回 130；第二次可立即退出。
- Refactor：以显式页面状态机替代散落 bool，确保所有选页最终都可审计；状态机跨平台一致。
- 验收：Windows 集成测试覆盖失败继续、partial JSON、恢复、取消和 stdout 不污染；跨平台 checkpoint 兼容键校验在 Windows 与 macOS 间不互相恢复。

## 10. 阶段 W4：GPU 后端（Vulkan 首选、CUDA 可选）

目的：在 CPU 强制路径之外提供 Windows GPU 加速。Vulkan 覆盖 AMD/Intel/NVIDIA 为首选；CUDA 为 NVIDIA 专用可选路径。两者均为“通过则发布、未通过仅阻断该路径自身”：Vulkan 未通过不阻断 CPU 发布，CUDA 未通过不阻断 Vulkan/CPU 发布。

### W4.1 Vulkan 构建与初始化

- Red：为 `vulkan` 特性编写构建与探测测试；断言未编译 Vulkan 时 `--device vulkan` 返回 `InvalidDevice`，编译但无可用设备时返回 `DeviceUnavailable`，`auto` 回退 CPU 并 warning。
- Green：在 Windows x86_64 上使 `vulkan` 特性构建通过（Vulkan SDK ≥ 1.3，`VULKAN_SDK` 环境变量约定路径）；实现 Vulkan 实例探测、设备枚举与初始化；初始化失败可回退 CPU。CI 无 GPU runner 时 Vulkan 路径标记 `DeviceUnavailable` 而非构建失败。
- Refactor：GPU backend 探测统一到 W1.2 的 `ResourceSnapshot`，Vulkan/CUDA 共用探测接口。
- 验收：Windows 上 Vulkan 可用设备可被 `doctor` 报告；不可用时 `auto` 回退 CPU 的 warning 可审计。

### W4.2 Vulkan 真实 E2E 与质量对照

- Red：用锁定输入建立 Vulkan 路径的结构化输出比对 golden；增加 Vulkan 初始化失败、设备丢失、显存不足的预期测试。
- Green：在 Windows NVIDIA/AMD/Intel GPU 上跑通 Vulkan model/mmproj 加载、MTMD 编码、生成与裁图完整 E2E。
- Green：对 Vulkan 路径运行质量回归，与 CPU 输出做结构化比对；`temperature=0` 下确认输出一致性或记录可解释差异。
- Refactor：Vulkan 与 CPU 共享 runtime 生成骨架，差异只在 backend 初始化与 offload。
- 验收：Vulkan 路径通过真实硬件 E2E 与质量门禁；与 CPU 输出一致或差异可解释；显存不足安全失败而非 OOM。

### W4.3 CUDA 构建（NVIDIA 可选）

- Red：为 `cuda` 特性编写构建与探测测试；断言 CUDA 路径仅在 NVIDIA GPU + CUDA toolkit 可用时启用。
- Green：在 Windows x86_64 NVIDIA GPU 上使 `cuda` 特性构建通过（CUDA toolkit ≥ 12.x，`CUDA_PATH` 环境变量约定路径）；实现 CUDA device 探测、初始化与回退。CI 无 NVIDIA GPU runner 时 CUDA 路径标记 `DeviceUnavailable` 而非构建失败。
- Green：在 NVIDIA 基准设备（需求 15.1）上跑通 CUDA 完整 E2E 与质量回归。
- Refactor：CUDA 与 Vulkan 共享 GPU 探测与生成骨架。
- 验收：CUDA 路径通过则发布为 NVIDIA 专用加速；未通过不阻断 Vulkan/CPU 发布，并在发布报告中明确标注未验证。

### W4.4 GPU 资源与性能报告

- Red：为 GPU 资源报告编写 schema golden；断言报告包含 backend、显存/统一内存、加载时间、首 token、输出 token/s、峰值内存。
- Green：在 Windows GPU 基准设备上采集 Vulkan/CUDA 的显存、耗时与质量数据；与 CPU 路径对比。
- Refactor：资源报告跨平台统一 schema，平台/backend 差异只作为字段值。
- 验收：GPU 路径资源报告完整；任何 GPU 路径未通过质量门禁不进入发布承诺。

## 11. 阶段 W5：诊断、Skill、安装与发布工程（Windows）

### W5.1 日志、隐私与 `doctor`

- Red：日志/result 捕获测试在 Windows 上覆盖 prompt、OCR 正文、token、代理认证和带查询参数 URL；这些值均不得出现。
- Green：实现 Windows 上 `doctor` 的平台/backend/内存/缓存/Skill 报告（不下载模型）；复用 W1.2 探测组件；敏感字段脱敏跨平台一致。
- Refactor：日志字段通过结构化事件统一脱敏，平台差异不引入新的脱敏调用点。
- 验收：Windows 上 doctor 不下载模型；安全/隐私测试在成功和错误路径均通过。

### W5.2 Skill Windows 安装

- Red：为 Windows 上 Codex、Claude、all、未知目标、已有用户文件和卸载边界写临时目录集成测试；增加 `home` 解析测试（`HOME` 未设置时从 `USERPROFILE` 解析、`USERPROFILE` 亦缺失时回退 `HOMEDRIVE`+`HOMEPATH`、无任何 home 变量时明确报错而非回退当前目录）与项目级 scope 测试（写入临时项目根下的 `.codex\skills\ocrx` 与 `.claude\skills\ocrx`）。
- Green：在 W1.3 已冻结的 `home` 解析之上实现 Windows Skill 安装（对应待决策项 5）：用户级默认写到 `%USERPROFILE%\.codex\skills\ocrx` 与 `%USERPROFILE%\.claude\skills\ocrx`，项目级显式写到 `<project>\.codex\skills\ocrx` 与 `<project>\.claude\skills\ocrx`（同 macOS `home.join(".codex/skills/ocrx")`/`home.join(".claude/skills/ocrx")` 布局）；原子写入并保留来源/version 标记，只操作本版本文件，保留目录内其他用户文件。`home` 解析本身在 W1.3 完成，W5.2 只做 Skill 专属安装逻辑。
- Refactor：宿主目录差异只存在于安装目录适配层，Skill 内容不嵌入 prompt、原始标签或量化文件名。
- 验收：Windows 上 Codex/Claude 安装与卸载烟测通过；用户级与项目级安装路径与待决策项 5 冻结结论一致；Agent 按 Skill 可完成一次真实或受控 E2E 调用。

### W5.3 Windows 构建与安装包

- Red：为 `package-windows-x64.ps1`/`install.ps1` 写脚本级 OS/arch/隐私门禁与哈希校验测试；复用 `test-release-scripts.sh` 的等价 Windows 检查。
- Green：实现 `package-windows-x64.ps1`（release 归档 + `SHA256SUMS`）与 `install.ps1`：检测 OS/arch/可用 GPU backend、下载匹配 GitHub Release、验 SHA-256、默认安装用户目录不要求管理员、提示 PATH、可选安装 Skill、不下载全部模型。
- Green：按已冻结策略实现分发（对应待决策项 4）：`pdfium.dll` 随包分发，归档内校验 DLL hash 与许可证集合；模型权重与 GPU runtime 按需下载，`install.ps1` 实现受信 URL + SHA-256 校验与 offline 拒绝。
- Refactor：安装脚本逻辑与 macOS `install.sh` 对齐，平台差异只在包格式（zip/tar.gz）、路径与 shell。
- 验收：Windows 归档与 `install.ps1` 烟测通过；`pdfium.dll`/GPU runtime 分发路径与待决策项 4 冻结结论一致；干净环境安装烟测属 W6。

## 12. 阶段 W6：发布验收与证据包

此阶段不新增功能，只执行冻结测试、处理已发现缺陷并重新跑受影响证据。

1. 在干净 Windows 10/11 x64 环境重跑 workspace、CLI contract、文件安全、恢复/取消、PDF worker、模型事务、offline、隐私、Skill 和安装测试。
2. 使用冻结的 30 页以上回归集在 Windows CPU 全量重跑 BF16 对照与四个 OvisOCR2 profile；如发布 Vulkan/CUDA，则同步重跑 GPU 路径；发布报告保留逐项/逐页评分、样本数、失败页和默认 profile 结论，并与 macOS 同基准结果对比。
3. 执行 Windows 100 页资源回归，报告 RSS/footprint、耗时、worker 页段、runtime/model revision 和环境；任何线性增长、OOM 或无心跳卡死都阻断发布。
4. 执行 Windows 真实 PDF E2E：正常、复杂、损坏、非 PDF、加密/正确错误密码、单页失败、截断、bbox 图像、resume、Ctrl+C、offline 缺模和 JSON report。
5. 复核 release 中 CLI/runtime/manifest/模型来源信息/Skill/基准报告的版本可追踪性，复核只出现 `ovisocr2`；确认 Windows 与 macOS 发布工件版本对齐。

最终证据包至少包含：Windows 兼容矩阵、manifest 与 SHA-256、许可证/来源记录、quality 与资源报告（CPU + 已验证 GPU）、测试命令与原始日志、fixture/benchmark 版本、安装归档 hash、已知限制和 go/no-go 结论。

## 13. 里程碑检查表

| 里程碑 | 允许交付 | 必须通过的门禁 | 明确禁止的范围漂移 |
|---|---|---|---|
| M-pre / WP | 共享契约补齐（缓存布局、runtime 对齐） | WP.1 缓存布局、WP.2 runtime 冻结记录 | 借 WP 顺手改 macOS 已冻结行为外的范围、推迟 fork/FFI 决策 |
| M0 / W0 | Windows CPU runtime/PDFium/资源/质量证据 | W0 质量门禁（入口）、worker 资源策略 | GPU 绕过 CPU 门禁、质量阈值下调 |
| M1 / W1 | 跨平台抽象、设备探测、路径解析 | backend 枚举、探测注入、路径安全 | 改变 macOS 已冻结行为、隐式 GPU 选中 |
| M2 / W2 | Windows PDF/worker/原子写 | worker 协议、资源非线性、错误分类 | 平台分支泄漏到模型/checkpoint 层 |
| M3 / W3 | Windows CPU 完整扫描、恢复取消 | W3 功能门禁（发布前）、CPU 真实 E2E、partial/130、串行 | GPU 依赖、提前写最终结果 |
| M4 / W4 | Vulkan/CUDA GPU 后端 | GPU E2E、质量对照、显存安全 | 未验证 GPU 进入发布承诺 |
| M5 / W5 | Skill、Windows 包与安装 | 隐私、安装烟测 | GUI、遥测、未经确认平台承诺 |
| M6 / W6 | Windows 发布候选 | 质量、稳定、安全、可追溯性 | 为发布临时放宽契约/阈值 |

## 14. 变更控制与阻断条件

每个 PR/提交必须关联本计划中的一个阶段和具体测试。若需求、技术方案、manifest revision、runtime 组合、模型权重、prompt/schema、评分器或 fixture 发生变化：先更新相应冻结记录与测试，再将基准版本递增并全量重跑受影响质量/兼容性证据。不得将未验证的候选数据写成发布承诺。

发现以下任一情况应停止当前实施并记录阻断，而不是自行扩展设计：

1. Windows CPU 路径无法稳定生成合格输出（W0.4 质量门禁失败）--阻断整个 Windows 轨道，不得以 GPU 后端替代 CPU 门禁。
2. `llama.cpp`/Rust binding 在 Windows 无法对齐到与 macOS 等价的兼容组合，且 WP.2 预置的固定 fork/最小内部 FFI 方案也无法建立，阻断整个 Windows 轨道。（WP.2 已把该决策前移到 W0 启动前，而非失败后回退。）
3. Windows PDFium 工件来源或再分发资格不清，或无法满足 100 页资源非线性增长门禁--阻断整个 Windows 轨道。
4. Vulkan 路径输出与 CPU 无法对齐且差异不可解释，或显存不足导致不安全 OOM--仅阻断 Vulkan 自身发布，不阻断 CPU 发布；不影响“是否首发包含 Vulkan”的决策（见待决策项 3）。
5. CUDA 路径输出与 CPU 无法对齐且差异不可解释，或显存不足导致不安全 OOM--仅阻断 CUDA 自身发布，不阻断 Vulkan/CPU 发布。
6. 需求与技术方案对 Windows 首发平台或安装脚本形成实质冲突，阻断整个 Windows 轨道，恢复前需产品/技术方案更新范围。**决策触发**：WP 启动时由产品/技术方案 owner 明确 Windows 是否仍属 v0.1 首发承诺，输出物为需求 3.1 节或技术方案 1.1 节的修订（owner = 产品/技术方案负责人，时点 = WP 启动前，输出物 = 范围修订记录）；未明确前 Windows 轨道保持“需求已承诺、实现待交付”状态，不进入 W0。

恢复工作前应由产品/技术方案明确更新范围和验收条件。本计划与 macOS v0.1 TDD 计划共享同一需求与公共契约；任何对 `PageDocument`、manifest、错误 JSON、checkpoint/result、JSONL、stdout/stderr 纪律、退出码的变更必须双向同步，避免 Windows 与 macOS 契约分叉。

## 15. 待决策事项

以下事项需在对应阶段冻结前决策，不阻断 W0 启动：

1. **已重定义（2026-08-11）** Windows CPU 质量门禁基准：原“Windows CPU vs macOS reference 差异上限”框架不成立（macOS v0.1 无 CPU fallback，不存在 macOS CPU reference）。门禁基准改为需求 17.2.3 的单平台阈值（GGUF `reference` 相对同 revision 官方 BF16：NED `0.01`、公式 `0.02`、TEDS `0.02`、阅读顺序 `0.01`、bbox `0.02`），该阈值已由需求冻结，W0.4 只做实测验证而非重新决策。macOS Metal 仅作跨平台一致性参考，不作门禁。**剩余开放问题**（W4 冻结）：是否需要 Windows CPU-vs-Vulkan 跨 backend 一致性阈值及其上限，需 W4 Vulkan 路径实测数据。
2. **已冻结（2026-08-11）** PDFium Windows 工件来源：采用 bblanchon/pdfium-binaries `chromium/7961` 的 `pdfium-win-x64.tgz`，与 macOS 同源同 revision，不自编译。依据见 `docs/develop/pdfium-windows-release-baseline-2026-08-11.md`；W0.2/W2.1 仅剩实机绑定与渲染验证。
3. 首发是否包含 Windows Vulkan，还是仅 CPU/CUDA（对应需求第 19 节待决策项 4 原文框架；W4 冻结）。本计划在 W4 同时实现 Vulkan 与 CUDA 路径以提供决策证据；任一 GPU 路径仅在通过门禁时发布，未通过仅阻断该路径自身，不影响 CPU 发布。
4. **已冻结（2026-08-11）** Windows 安装分发策略：镜像 macOS--`pdfium.dll` 随包分发（归档内校验 hash 与许可证），模型权重与 GPU runtime 按需下载（`install.ps1` 实现受信 URL + SHA-256 校验与 offline 拒绝）。
5. **已冻结（2026-08-11）** Skill Windows 安装目录：用户级默认，显式支持项目级，与 macOS 同布局（`<home>/.codex/skills/ocrx`、`<home>/.claude/skills/ocrx`）；`home` 在 macOS 为 `$HOME`，在 Windows 解析为 `%USERPROFILE%`。
   - 用户级（默认）：`%USERPROFILE%\.codex\skills\ocrx\SKILL.md`、`%USERPROFILE%\.claude\skills\ocrx\SKILL.md`。
   - 项目级（显式）：`<project>\.codex\skills\ocrx\SKILL.md`、`<project>\.claude\skills\ocrx\SKILL.md`。
   - 路径全部经环境变量/参数解析，不写死用户名；`home` 解析改用跨平台方式（`dirs::home_dir()` 或 `USERPROFILE`→`HOMEDRIVE`+`HOMEPATH` 回退），替换 `main.rs` 仅读 `HOME` 的现状（Windows 上 `HOME` 常未设置会回退当前目录）。
   - 依据：Claude Code 在 Windows 的用户目录即 `%USERPROFILE%\.claude\`；Codex `find_codex_home()`（`codex-rs/utils/home-dir/src/lib.rs`）默认 `dirs::home_dir().join(".codex")` = `%USERPROFILE%\.codex\`，project 级为 `./.codex/`（`codex-rs/config/src/loader/mod.rs` 配置层文档第 110-114 行）；Codex 仓库自身 skills 即放在 `.codex/skills/<name>/SKILL.md`。与需求第 19 节待决策项 9、技术方案第 1.4 节对齐。
   - 已知后续（非 v0.1 阻断）：Codex 自身支持 `CODEX_HOME` 覆盖整个 `.codex` 父目录；v0.1 为与 macOS 对齐暂不单独 honors `CODEX_HOME`（macOS 侧亦未 honors），登记为 post-v0.1 跨平台一致性增强。

## 16. 与 macOS v0.1 计划的关系

- 本计划不修改 macOS v0.1 的发布范围与冻结门禁；macOS v0.1 仍按 `docs/plan/v0.1-tdd-development-plan.md` 推进。
- 本计划复用 macOS 已冻结的公共契约、fixture、评分器与 worker 协议；Windows 阶段的“已完成”状态以 macOS 契约为基准。
- macOS v0.1 的 P6 发布证据包与 Windows 的 W6 证据包共同构成需求第 17 节的双平台验收；任一平台的硬门禁失败均阻断对应平台发布，但不允许以另一平台的结果替代。
- 当本计划 W6 完成时，需求第 3.1 节的 Windows 首发承诺才具备兑现证据；在此之前，仓库对 Windows 的支持状态保持为“需求已承诺、实现待交付”。
