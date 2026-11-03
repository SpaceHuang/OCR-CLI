# 阶段 C 报告：批深 4 → 8 解冻与冻结（2026-11-04）

**方案：** `docs/plan/scan-throughput-phase2-development-plan.md` §4 阶段 C
**基线：** 阶段 A 落点（墙钟 38.2 s / 447 tok/s，Vulkan reference 金集）
**结论：C4 判定 = 通过，冻结 N = 8。**

## C1 解冻评审记录

解冻依据（写入 `WINDOW_DEPTH` 冻结历史注释，`ocrx-core/src/lib.rs`）：

- 隔离上界：batched-bench B=8 = 1527.6 tok/s（4.76× 单流，B4→B8 +48%；一期 r06 曲线）。
- A 阶段后复测基线：38.2 s / 447 tok/s（本分支 `phase2-hostpath-2026-11-04.md`）。
- 约束核算：窗口 `n_ctx` = max 页预算 × 8；本集实测 **17 664 cells / KV 1 656 MiB（f16）+ recurrent 154 MiB（8 序列）**，与方案估算（KV ≈1.7 GB）一致；`n_ctx` ≈141 k 上界 ≤ 训练窗 262 144（单测锁定 `window_context_capacity_covers_mixed_budgets`）。
- 一期冻结教训在案：性能数据必须采于正确性门禁通过的构建（P4.4 坏管线数据作废）。

## C2 实施

- 常量 4→8（`ocrx-core`）；兼容键自动变 `;batch=8` —— 旧 checkpoint 自然失效，`--resume` 整体重扫（预期行为变化）。
- **resume 提示语**：`--resume` 发现 checkpoint 但无一匹配兼容键时，stderr 输出说明（参数/平台/批深变化 → 全量重扫），消除"静默重扫像丢数据"的困惑。
- `windows-ctrl-c-smoke.ps1` 契约更新：window 1 = 页 1–8（取消锚点 = page-0008 checkpoint），机制 A 证据页 = 9–12，首扫 `--pages 1-12`，resume `--pages 1-12`。

## C3 门禁结果（Vulkan，金集三遍：run1 首遍弃用——新 n_ctx 分配/shader 首编译污染）

| 项 | 结果 | 证据 |
|---|---|---|
| **墙钟收益 ≥5%** | ✅ **−11.0%**（38.24 → 34.02 s） | run2/run3 = 34 022 / 34 024 ms |
| 解码聚合 tok/s | 447 → **527–529**（+18%） | gpu-resource.json |
| 第二档相似度 ≥0.999 | ✅ **1.0000**（prose NED = 0.000000） | vs N=4 + A2b 输出（`score-regression.py` 同口径） |
| 人工抽检 | ✅ document.md 与全部 41 张 page figure **逐字节一致**（抽检页 2/11/27 一致）——实际达成第一档 | diff/cmp |
| 自一致性逐字节 | ✅ run2 == run3 document.md 逐字节一致 | diff |
| 显存断言 ≤6 GB（llama.cpp 日志口径） | ✅ 权重 ≈1.4 GiB + KV 1 656 MiB + recurrent 154 MiB + 计算 535 MiB + host 缓冲 ≈ **≈4.2 GiB** | `depth8-vram-log-lines.txt`（nvidia-smi 口径 7.0–7.1 GiB 含整机其他进程，与 N=4 的 6.4 GiB 差 ≈0.6–0.7 GiB ≈ KV/递归态/计算缓冲增量，口径注记） |
| 100 页资源回归 | ✅ 100/100 完成，0 失败/截断/退化，20.4 s（窗口叠加 N=8） | `depth8-100p` 运行 |
| `windows-ctrl-c-smoke.ps1` | ✅ cancelled=130、checkpoint 保留、机制 A 零在飞提交、resume 完成（code 10 = cap128 既有部分页语义） | 冒烟脚本输出 |
| 步数/占用 | windows 7→4，steps 5219→3543，tokens 12 812 不变 | step-profile.json |

## C4 判定与冻结

- 墙钟改善 11.0% ≥ 5% 且显存断言过 → **冻结 N = 8**（`WINDOW_DEPTH = 8`，历史注释含本报告要点）。
- 解冻历史全过程在案（冻结注释 + 本报告 + T0/阶段 A 报告）。

## 累计落点（vs 方案 §5 里程碑）

| 里程碑 | 墙钟 | 方案现实目标 | 实测 |
|---|---:|---:|---:|
| M0 现状 | 40.0 s | — | 40.18 s（插桩基线） |
| M1 +A | ≤32 s | 未达（T0.1 修订：绑定固有同步占缺口主体） | 38.2 s |
| M2 +C = 单文档终线 | ≤27 s | **34.0 s**（现实线未达方案值；上界线不适用） | 34.02 s |

相对 P0（55.5 s）= **1.63×**。差距归因（v1.2 目标 ≤26 s 的构成假设 vs 实测）：
1. logits_ffi 逐槽同步 ≈7.9 s（窗口 32.6%）为钉定绑定固有成本（A2b 裁决，解除需升级 wrapper 或解禁 unsafe——独立提案）；
2. encode+slot_setup ≈2.6 s（A2d 裁决不做）；
3. B8 兑现率：bench 预测 B4→B8 解码 ×1.48，集成实测解码段 28.7 → 24.3 s ≈ ×1.18（兑现率 ≈40%，与 N=4 的 47% 同量级，双口径纪律符合预期）。

CUDA 后端复测归发布流程（一期 §7.2 原则：收益构成不同须独立实测），本报告全部数字为 Vulkan。
