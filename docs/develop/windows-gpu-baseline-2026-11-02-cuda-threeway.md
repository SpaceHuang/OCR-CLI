# T1.4 CUDA 门禁与三方基线（2026-11-02，本机 4090 D）

**方案：** `docs/develop/scan-throughput-optimization-development-plan.md` §5（P2 = W4.3 执行）
**前置：** T1.0/T1.1（构建环境与构建验证，2026-10-31，见方案 §5.5）、T1.2/T1.3（打包/安装链路，本分支 `713f879`）

## 1. 门禁结论

| 项 | 结果 | 证据 |
|---|---|---|
| `--features cuda` 构建（当前分支头） | ✅ | `D:\ocrx-tgt-p2\release\ocrx.exe`（Ninja + vcvars64 配方，见 §5.5）；`doctor --report json` 报 `backend=Cuda` / `recommended_device=cuda` |
| CUDA 真实 E2E 全链路 | ✅ | 30 页金集 scan（reference，`--device auto`→CUDA）：30/30 完成、0 失败/截断/退化，`backend=cuda`，产出 `document.md`/`page-*.json`/checkpoint/images 完整（`t14-threeway-data/t14-cuda-01/`） |
| 结构化比对 vs CPU（§2.3 第二档） | ✅ | `document.md` 相似度 CUDA/CPU 0.9998、CUDA/Vulkan 0.9999、Vulkan/CPU 0.9999；token 数 CUDA 12840 vs Vulkan/CPU 12839（跨后端 1 token 分叉，浮点归约顺序差异，预期内） |
| 显存不足安全失败注入 | ✅ | `--context-size 2000000000` + `--device cuda`：无 OOM 崩溃，页级安全失败（`status=partial, failed=1`），`result.json` 结构化落档，进程正常退出 |
| 打包/安装链路（T1.2/T1.3） | ✅ | `test-release-scripts.sh` 全绿：9 格安装矩阵 {cuda+vulkan, vulkan, cpu-only}×{nvcuda, vulkan-1, 无}、损坏 cuda 变体降级到 vulkan、脚本语法/隐私门禁（`713f879`） |

## 2. 三方单流基线（30 页金集，reference，同机同时段，各一遍）

| 后端 | 墙钟 | 解码 tok/s | 生成 token | load_ms | 备注 |
|---|---:|---:|---:|---:|---|
| CUDA | 52.35 s | 304.8 | 12 840 | 1 153 | `D:\ocrx-tgt-p2`（本分支构建） |
| Vulkan | 55.68 s | 282.0 | 12 839 | 1 276 | `D:\ocrx-tgt-p1`（本分支构建） |
| CPU | 681.9 s | —（OCRX_GPU_REPORT 仅 GPU 后端） | — | — | `D:\ocrx-tgt-cpu`（本分支构建） |

- **CUDA/Vulkan 墙钟加速比 1.06×**（解码段 1.08×），低于 T1.4 预研的 1.26–1.38×（§5.5，构建/时段不同）；与预研一致的结论：CUDA 收益真实但被端到端非解码段摊薄。**正式发布基线仍按 W4.2/W4.4 在登记基准机采集**，本表为本机对照记录。
- 原始数据：`docs/develop/t14-threeway-data/`（各含完整 `result.json`/`gpu-resource.json`）。

## 3. 发布规则（W4.3 既有纪律）

门禁全绿 → `ocrx-cuda.exe` 具备进入发布工件的条件（T1.2 打包分支 + T1.3 安装链已就绪并通过脚本矩阵）。CI 无 NVIDIA GPU 时 CUDA 路径标注 `DeviceUnavailable` 而非构建失败（W4.3 既有约定，构建由 `--features cuda` 显式触发不受影响）。
