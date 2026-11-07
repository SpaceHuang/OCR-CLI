# Q1.2 采样逃逸实验 · scratch spike 记录（0.1.161 构建 + 机制骨架 + 扫参协议）

**日期：** 2026-11-07　**分支：** `scratch/q12-sampling-escape`（worktree `.worktrees/q12-sampling`）　**前置裁决：** Q1.1 候选 D GO（36.2%），剩余 136 页退化需要采样档

---

## 1. 已完成

1. **pins 升级**：`llama-cpp-2` / `llama-cpp-sys-2` = 0.1.161（S0 审计认定对；crates.io 拉取成功）。
2. **机制骨架**（commit `fa073ba`）：
   - `RetrySampling`（milli 整数、Eq 安全）+ `InferenceRequest.retry_sampling`；
   - 识别 trait 的 retry seam（仅 attempts ≥ 2 路由，default 委托保持旧行为）；
   - adapter 采样变体 + request 捕获测试；
   - CLI 扫参旗标 `--degeneration-repeat-penalty-milli`（1000..=2000）/ `--degeneration-penalty-window`（默认 64）；
   - worker 重试 context（`new_context_with_samplers`，链 `penalties → greedy`，链序 D-13）+ `sampled_token_ith(0)` 带 argmax 兜底。
   - wrapper 无关部分已在 feature worktree（0.1.152）全量测试验证后移植（77 tests）。

## 2. 阻塞：0.1.161 sys 构建（嵌套 vulkan-shaders-gen configure 失败）

**现象**：`cargo build --release -p ocrx --features vulkan` 在 `llama-cpp-sys-2 0.1.161` 的 build script 失败——llama.cpp 的 `ggml-vulkan` 通过 `ExternalProject_Add` 配置 `vulkan-shaders-gen` 子项目时，`project()` 报 **No CMAKE_C/CXX_COMPILER could be found**。

**已排查**（CMakeConfigureLog.yaml 证据）：
- 主 configure 正常（VS generator、SDK 10.0.26100 选中）；
- 嵌套 id 编译结果**跨次运行不稳定**（failed → failed → succeeded 同日志可见）：ClCompile 用 BuildTools 的 CL.exe（14.44）编译成功，Link 步骤退出码 1；伴随 MSBuild "批处理标签 VCEnd 找不到"（MSB8066）。
- 机器上同时装有 VS2022 **Community** 与 **BuildTools**；cmake crate 取到 Community 的 cmake.exe，而父构建由 BuildTools 的 MSBuild 执行——嵌套 ExternalProject 的 generator/实例选择在两套 VS 之间漂移，是该失败的最高概率根因。
- 已尝试无效：vcvars64 包裹、`CMAKE_GENERATOR`/`CMAKE_GENERATOR_PLATFORM` 环境变量、`MSBUILDDISABLENODEREUSE=1`、关闭沙箱、多次重试。
- **0.1.152 主树构建成功过同一 ExternalProject 结构**（`target/release/build/llama-cpp-sys-2-cd143ee2f4206b1e`，嵌套 generator 同为 VS 17 2022）——证明机器能力具备，失败与本 shell 会话/工具链选择相关。

**下次会话的修复路径（按优先级）**：
1. 在 **Developer PowerShell for VS 2022（Community）** 里跑构建（完整 VS 环境消解实例漂移）：`$env:VULKAN_SDK="D:\VulkanSDK\minimal"; cargo build --release -p ocrx --features vulkan`。
2. 或清理嵌套构建缓存后以单一实例重跑：删除 `target/release/build/llama-cpp-sys-2-1f17b6c52fb6e58f/`（scrub），显式 `CMAKE_GENERATOR`。
3. 或 vendor-patch sys crate 的 `ggml-vulkan/CMakeLists.txt`，给 `ExternalProject_Add` 显式传 `CMAKE_GENERATOR "Visual Studio 17 2022" CMAKE_GENERATOR_PLATFORM x64`（最后手段）。

## 3. 构建通过后的验证序列（gold-set 前置，plan Q1.3 纪律）

1. `cargo test --workspace`（scratch 上全绿；机制部分在 0.1.152 已验证）。
2. **金集 30 页逐字节比对**（0.1.161 行为漂移检查，Q1.3 契约）：基线 `ocrx scan` 30 页金集 vs 新二进制同参扫描，page.json/document.md 逐字节 diff 必须为零；出现任何字节差异 → 按三期规则 3 追加 `;decode=<gen>` 代次键 + 新旧不混扫回归。
3. `sampled_token_ith(0)` 的 V1 验证：首扫单页日志确认 mtmd 分块 eval 后首个采样 token 非 None（否则 argmax 兜底降级为"仅 token 步惩罚"）。

## 4. 扫参协议（报纸子集先行，plan Q1.2）

- **扫参集**：语料中报纸退化残余页（2048 后仍未逃逸的 69 页报纸 + 4 页 table_html 环）为第一轮；`--degeneration-repeat-penalty-milli ∈ {1100, 1200, 1300} × --degeneration-penalty-window ∈ {64, 256}`，共 6 轮 × ~70 页 ≈ 15-20 min/轮。
- **评估**：每轮跑 `scripts/evaluate-degeneration-experiment.py` 需扩展 `--scan` 指向采样扫描目录（逃逸率公式不变）；行片段环（Q2.1 主体）允许中高惩罚档，table_html 环页仅接受 1100 低档结果（MinerU 风险点）。
- **质量门禁**：逃逸页过官方子集评分（escaped-subset 流程同 Q1.1）。
- **裁决**：语料逃逸率 ≥ 50% 且子集评分改善可复现 → Q1.3 GO；否则 NO-GO 归档。
- **确定性**：链无随机源；固定种子仅扫参含随机变体时需要（本协议不含）。

## 5. 里程碑衔接（Q7.2）

Q1.2 GO → Q1.3 落地（pins 修订 v2 计划 §2.2）→ Q1.5（D-14）+ 场景 C 测试更新 → 全量 balanced 复评对照 §2.1 门槛（GO ≥ 85 / 进取 ≥ 88 / 满收参考 91.1，projection 脚本口径）。
