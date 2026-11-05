# 引擎对比报告：llama.cpp CUDA vs Vulkan（qwen35/OvisOCR2，2026-11-05/06）

**目的**：回答"换 vLLM/LMDeploy 是否能进一步提速"——先测 llama.cpp 内部的后端/kernel 上限，再据此外推引擎切换的收益空间。
**方法**：① vendored 同源（9e3b928）llama.cpp 的 CUDA 后端构建，隔离批解码 bench 对比 Vulkan；② ocrx CUDA feature 构建，金集两遍集成实测对 Vulkan 34.02 s 基线；③ CUDA graph 生效性判别（`GGML_CUDA_DISABLE_GRAPHS=1` 对照）；④ 跨后端输出相似度（第二档口径）。
**数据**：`llamacpp-cuda-vs-vulkan-bench-2026-11-05-data/`（`-text`）。

## 0. 架构事实（实读 vendored 源码 + 加载日志）

- OvisOCR2 = `qwen35` arch（general.base_model = Qwen3.5 0.8B，752M），24 层中 6 层 KV + 18 层 GDN 递归态（ssm_d_state=128, d_inner=2048）。
- GDN 解码 kernel：**CUDA 与 Vulkan 均有融合实现**（`gated_delta_net.cu` warp 级 / `gated_delta_net_*_kda` shader）。chunked-scan 上游仅列为 **prefill TODO**——单 token 解码本质是 O(1) 递归更新，chunked 是 prefill/训练技术。
- CUDA graph：按 graph-key 启用，仅 pre-Ampere 禁用，无 SSM/GDN 排除项；wrapper 未暴露但经 C 层默认生效。

## 1. 隔离批解码 bench（llama-batched-bench，-npp 0 -ntg 256，warm 记录 ×2）

| 批深 | Vulkan（T0.2 记录） | CUDA | 差异 |
|---|---:|---:|---|
| B=4 | 1023.4 tok/s | 1021.7 / 1032.6 | 持平 |
| B=8 | 1527.6 tok/s | **1714.7 / 1725.2** | **+13%** |

## 2. 集成实测（金集 30 页，reference，depth 8，两遍 + 诊断遍）

| 项 | Vulkan 基线 | CUDA（run2 记录） | 差异 |
|---|---:|---:|---|
| 墙钟 | 34.02 s | **36.66 s** | **−7.8%（更慢）** |
| 解码 tok/s | 527–529 | 465.1 | −12% |
| decode_wait | 11.46 s | 14.39 s | +26% |
| logits_ffi | 7.90 s | 8.81 s | +12% |
| argmax | 1.80 s | 1.80 s | 持平 |
| window_total | 24.27 s | 27.61 s | +14% |
| VRAM（进程） | ≈6.4–7.1 GiB | 7.05 GiB | 相当 |
| 自一致性 | ✓ | ✓（run1==run2） | — |
| vs Vulkan 冻结参照 | — | 相似度 **0.999773** ≥ 0.999 | 第二档可过（1 字符差） |

**关键矛盾**：隔离 +13% → 集成 −7.8%。graph 判别实验定位了原因的方向——

### CUDA graph 生效性（判别实验）

| 条件 | elapsed | decode_wait |
|---|---:|---:|
| graphs 启用（默认） | 36.66 s | 14.39 s |
| `GGML_CUDA_DISABLE_GRAPHS=1` | 39.86 s | **21.69 s** |

**结论：CUDA graph 在集成里确实生效**（关闭后 decode_wait +51%、墙钟 +8.7%）——且是 CUDA 后端没有整体落败的原因；但开启后集成 decode_wait 仍比 bench 等效值（7.4 s）高 7 s，CUDA 特有的集成开销（每步 host 侧同步 + DtoH 尾部的组合）大于 Vulkan。Vulkan 的集成/隔离比（11.46/8.4 ≈ 1.36×）优于 CUDA（14.39/7.4 ≈ 1.94×）。

## 3. 判定

1. **CUDA 后端：NO-GO**（集成墙钟 −7.8%，远低于 ≥5% 合入线；数据归档）。保持 Vulkan 为主后端。第二档相似度门槛本可过（0.999773），但性能不成立。
2. **vLLM/LMDeploy：按本轮证据降级为"暂不跟进"**——引擎级解码 kernel 差距实测有界（CUDA/Vulkan 同为融合实现，差距 ≈13%）；chunked-scan 不适用于单 token 解码；vLLM 剩余优势 = 服务端采样消 host 路径 + graph/kernel 融合残余，综合估算金集 ≈18–22 s（−35~47%），对比其平台代价（CUDA-only、Python/torch GB 级足迹、服务化架构、权重分发换轨、质量门禁全套重跑）**性价比不足**。此前"10~15 s"的估算依据（kernel 量级差距）被本轮实测否定。
3. llama.cpp 内部路线的剩余空间与既有裁决一致：logits_ffi（绑定固有同步）需 wrapper 升级独立提案；host-path 其余项已由 T1 数据关闭。

## 4. 附注

- CUDA 构建配方：vcvars64 + CUDA 12.4 + Ninja + `CARGO_TARGET_DIR` 短路径（`D:\ocrx-tgt-p2-cuda`），14 min；隔离 bench 构建于 `D:\llamacpp-bench\build-cuda`（CMAKE_CUDA_ARCHITECTURES=89）。
- CUDA 集成 run1 VRAM 11.02 GiB（首遍含分配/JIT 预热）→ run2 7.05 GiB 稳态。
- 跨后端浮点发散极小（30 页仅 1 字符 prose 差）——若未来任何场景需要 CUDA，质量门禁无需重跑即可按第二档通过。
