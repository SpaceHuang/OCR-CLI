# runtime commit/binding Windows 对齐冻结

日期：2026-08-14  
范围：Windows 平台 `llama-cpp-2`/`llama-cpp-sys-2` 构建可行性与 commit/binding 冻结。对应计划 `docs/develop/windows-platform-development-plan.md` WP.2（B2）的冻结。  
结论状态：**已冻结**--Windows 与 macOS 共享同一 wrapper/sys/llama.cpp commit 组合，无需 fork/内部 FFI 回退方案。

## 1. 冻结结论

Windows 与 macOS 共用同一组绑定，不引入 fork 或最小内部 FFI：

| 项目 | 值 | 一致性 |
|---|---|---|
| wrapper crate | `llama-cpp-2 =0.1.152` | 与 macOS 相同 |
| sys crate | `llama-cpp-sys-2 =0.1.153` | 与 macOS 相同 |
| 内嵌 llama.cpp commit | `9e3b928`（全量 `9e3b928fd8c9d14dbf15a8768b9fdd7e5c721d66`，2026-06-07 upstream） | 与 macOS 相同 |
| feature 集 | `default-features = false, features = ["mtmd"]` | 与 macOS 相同 |
| GPU feature | 本阶段无（CPU-only）；`vulkan`/`cuda` 归 W4 决策 | macOS 另有 `metal`（不影响共享组合） |

单一版本同时满足 macOS Metal 与 Windows CPU 路径（W4 GPU 路径待实测），B3 回退方案（固定 fork / 最小内部 FFI crate）**不启动**。`ocrx_core::RUNTIME_COMPATIBILITY.llama_cpp_commit` 已由空串填为 `9e3b928`，`validate_runtime_adapter_compatibility` 与之共享单一来源。

## 2. 内嵌 commit 验证方法

`llama-cpp-sys-2` 0.1.153 的 crate 内嵌 llama.cpp 源码树，但无 `.git`（CMake `BUILD_COMMIT` 会解析为 `unknown`），commit 以文件哈希比对 upstream 验证：

| 文件 | 本地（crate 内嵌） | upstream `9e3b928` | 结果 |
|---|---|---|---|
| `llama.cpp/CMakeLists.txt` | `ca3796c94d33…` | `ca3796c94d33…` | 一致 |
| `llama.cpp/src/llama.cpp` | `1d9e17bfac74…` | `1d9e17bfac74…` | 一致 |
| `llama.cpp/ggml/include/ggml.h` | `36c1c93e4406…` | `36c1c93e4406…` | 一致 |

crate tarball 与 upstream commit 的对应关系另由 `utilityai/llama-cpp-rs` 仓库历史交叉印证（2026-06-08 `e9885c6 updated llama.cpp` 即此快照）。

## 3. Windows 构建证据

在开发机（Windows 11 x64）以 mtmd 完成 `cargo build -p ocrx-runtime`，llama.cpp 全量编译通过（dev profile，约 3m40s）：

| 环境 | 版本 |
|---|---|
| rustc | 1.97.1 (8bab26f4f 2026-07-14) |
| MSVC | 19.44.35228（VS Build Tools 2022，`E:\BuildTools`） |
| CMake | 3.31.6-msvc6（BuildTools 内置） |
| Ninja | 1.12.1 |
| Windows SDK | 10.0.26100.0 |
| CUDA toolkit（本机） | 11.6（本阶段未启用；GGML_CUDA 属 W4，且当前 llama.cpp 需要更新版本 CUDA） |

验证要点：

1. **sys 构建通过**：`llama-cpp-sys-2 0.1.153` 在 MSVC x64 上以 `mtmd` 编译 llama.cpp 成功，无补丁、无 feature 妥协。
2. **mtmd 适配层可编译**：`ocrx-runtime` 的 `LlamaMtmdRuntime`（`llama_mtmd.rs`）在 Windows 上 `cargo check` 通过，wrapper 0.1.152 API 无平台缺口。模块 cfg 由 `macos` 放宽为 `any(macos, windows)`（CLI 侧 macOS gate 不变，无行为变化；backend 报告的 Metal→CUDA/Vulkan 映射归 W1.1）。
3. **B1 守卫**：`ocrx-runtime` 测试断言 Windows target 必须声明两个绑定且都启用 `mtmd`（丢 crate 或丢 feature 即测试失败），对应"Windows target 未启用 mtmd 时不编译"的失败用例前移。

## 4. 对 W0.1 的基准口径

按计划验收：W0.1 的"同 commit `llama-mtmd-cli` 结构化比对"基准为 sys 内嵌 commit `9e3b928` 的 reference 输出，接受**结构化一致**而非逐字节相等，与 macOS 候选冻结口径一致。

## 5. 风险与开放项

- 本机 CUDA 11.6 过旧，W4 若走 GGML_CUDA 需升级 toolkit；Vulkan 路径不受影响（无需 CUDA toolkit）。
- `9e3b928` 之后的 upstream 已发布 b10200（llama-cpp-rs `790a1ec`，2026-08-03 升级），本冻结**不跟随**，保持与 macOS 一致；升级需两侧同步重冻结。
- 真实模型加载/生成（W0.1）与 GPU feature（W4）在各自阶段验证，本冻结只覆盖编译可行性。
