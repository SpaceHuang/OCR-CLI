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

## 1.1 构建已解决 + 金集漂移实测（2026-11-07 续）

**构建根因链（三层叠加，逐层揭开）**：
1. cl.exe 不在 PATH——CompilerId 的 PostBuildEvent 用 PATH 搜索定位编译器，
   缺失时即使编译成功也报 No CMAKE_C_COMPILER；
2. bash 的 POSIX TMP/TEMP 泄漏给原生 cl（error C1083 空文件名 Invalid argument）；
3. `.worktrees/<name>/target` 嵌套使 ExternalProject 的 TryCompile 路径超 MAX_PATH
   （CARGO_TARGET_DIR=C:/t 短路径绕过）。
另有上游变化：0.1.161 的 vulkan-shaders-gen 新增 find_package(Threads REQUIRED)
（CHECK_LIBRARY_EXISTS 探测里的 LNK1104 pthread.lib 是良性失败记录，win32 分支仍
置 Threads_FOUND）。构建环境配方：MSVC bin + Community MSBuild 前置 + Windows TMP。

**金集漂移实测（Q1.3 关键输入）**：0.1.161 二进制 vs 基线 f3b6e068 同机同参金集扫描：
**38/91 文件内容级漂移**（解码行为改变），样本页（金集 page-0001）新输出含
"经经" 复读伪影——0.1.161 普通路径存在轻微质量劣化迹象。按 D-13 规则 3：
Q1.3 落地必须追加 `;decode=<gen>` 代次键 + 新旧不混扫回归；且 Q1.2 GO 裁决
需把"正常路径漂移"计入收益权衡（逃逸收益 vs 42% 页面输出变化 + 样本劣化）。
**Q1.2 扫参前新增必过门**：0.1.161 普通路径对语料的整体退化率不得高于基线
（待一轮语料级对照扫描后裁决）。

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


## 4.1 漂移门终版数据：0.1.161 内在逃逸 + 机制互补性（213 页全量）

| 机制 | 语料逃逸率 | 说明 |
|---|---|---|
| 0.1.161 普通路径（1024，无采样无 2048） | **51/213 = 23.9%**（全部 attempt=1） | 纯版本升级的"免费"逃逸 |
| token-2048（0.1.152 上实测，E 轮） | 77/213 = 36.2% | 已 GO 的候选 D |
| 两机制逃逸集合 | 重叠 39 / 仅 0.1.161 12 / 仅 2048 38 | **实质互补** |
| 并集覆盖 | **89/213 = 41.8%** | 双双卡死 124 页 = 采样的目标池 |

**裁决形状更新**：
1. 采样机制（Q1.2）必须跑在 0.1.161 上（per-sequence sampler API 仅此版本有）——
   0.1.161 的采用问题由机制可用性决定，不再是一个可选项。
2. Q1.3 的代价随之固化：`;decode=<gen>` 代次键 + 新旧不混扫回归 + **全集正常路径
   核验**（金集样本伪影证明存在正常页漂移风险，语料扫描无法替检——语料全是退化页）。
3. 扫参裁决线维持 ≥50%（分母 213）：采样需在 124 页"双卡死池"里再救出 ≥16 页
   （13%）即整体过线（若 2048 组合扫描则基数另计）。
4. 每格扫参同时实测 0.1.161 上 token-2048 的边际（0.1.152 数字不能直接移植）。


## 4.2 Q1.2 扫参裁决数据（2026-11-09 补全）

| 探针 | 子集 | 卡死页 | 采样边际逃逸 | 边际率 |
|---|---|---|---|---|
| 1200/64 | 报纸 92 | 84 | 1 | **1.2%** |
| 1300/256 | 报纸 92（剂量×2） | 84 | 1 | **1.2%（剂量零响应）** |
| 1200/64 | 学术+试卷 70 | 38 | 8（5 学术 + 3 试卷） | **21.1%** |

- V1 运行时验证：`get_sampled_token_ith` 无效读取每页恰 1 次（prompt eval 后首读，
  argmax 兜底按设计接管）；token 步读取全部有效，惩罚链真实生效（167 警告/92 页）。
- 报纸抵抗的机理解释（Q5.1 分类学印证）：行片段/数字递增环以**变体 token 重组**逃避
  repeat_penalty 的精确历史匹配——惩罚只打"历史中出现过的同名 token"。
- 学术/试卷（公式类环）对惩罚响应良好（21.1%），与 E2 的信息密度敏感性同向。

## 4.3 Q1.2 裁决：类别有效（scoped GO）

1. **采样档作为类别门控阶梯档位 GO**：仅公式/学术/试卷类重试挂 `repeat_penalty`
   （建议 1200/64 起步）；**报纸类排除**（1.2% ≈ 无效，不浪费重试预算）。
2. **全语料 ≥50% 线以当前杠杆不可达**：组合投影（0.1.161 内在 23.9% ∪ token-2048
   36.2% ∪ 类别惩罚 +8/38）≈ 42–46%。缺口集中在报纸残余池（~60 页双疗法卡死）。
3. 惩罚档全语料验证 + 逃逸页官方子集评分留待组合阶梯落地后一轮完成：
   组合的前提是 token-2048 作为重试档位可执行（Q1.4c spike：重试现场重 init
   独立 context 或常驻双 context），当前 CLI 的 --max-image-tokens 是全页全局开关。
4. Q7.2 里程碑投影更新：Overall ≈ 77.99 + 13.15 × (0.42–0.46) ≈ **83.5–84.1**，
   距 GO 线 85 差 1–1.5 分——报纸残余池是唯一剩余杠杆位（候选 C 切栏受 Q2.1
   跨栏图片 91% 限制，收益存疑；或接受残余维持 83.5–84.1）。
