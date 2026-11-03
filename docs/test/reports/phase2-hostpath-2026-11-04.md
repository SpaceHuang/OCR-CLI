# Phase-2 阶段 A 报告：窗口解码 host-path（2026-11-04）

**方案：** `docs/plan/scan-throughput-phase2-development-plan.md` §4 阶段 A
**前置：** T0.1 拆分（`phase2-t0-2026-11-03.md` §1）已完成，A 目标值按实测修订。
**基线：** `a02d459` 冻结参照（墙钟 40.0 s / 424 tok/s）；本阶段插桩基线复测 40.1–40.2 s / 421–422 tok/s。

## 1. 实施内容与逐项判定（规则 1：<2% 回滚 / A 阶段 ≥5% 建议线）

| 项 | 内容 | 判定 | 依据 |
|---|---|---|---|
| A1 | `OCRX_STEP_PROFILE` 分段计时独立调试通道（`step-profile.json`，opt-in）+ schema 兼容锁测试 | ✅ 合入（诊断基建，非性能项） | `6b15b21`；`RuntimeTelemetry`/`gpu-resource.json` schema 不变（黄金 JSON 测试锁定） |
| A2a | 每步 batch 复用/降分配 | ❌ **不实施（<2% 规则，实施前判定）** | T0.1 实测 batch_build = 0.25 µs/step（1.3–1.5 ms/scan），方案预判的"N 序列构建成本"不成立 |
| A2b | 逐槽 FFI 合并 | ⚠️ 拆两半：FFI 合并**绑定受限不可实施**；argmax 循环外提 ✅ 合入 | 详见 §2/§3 |
| A2c | 环检测热路径 | ❌ 不实施（0.1 ms/scan；且触发点位移破坏第一档语义） | T0.1 |
| A2d | 编码/解码重叠评估 | ❌ 评估结论：不做（见 T0 报告 §1 结论 4；上界 2.5 s 且需 actor 协议扩展） | T0.1 |

## 2. A2b-FFI：逐槽 `get_logits_ith` 摊薄——绑定受限结论（T0.1 核心发现）

T0.1 实测 logits_ffi = 10.5–10.6 s（窗口时间 34.6%）。归因链（sys 0.1.153 / wrapper 0.1.152 实读）：

1. `llama_get_logits_ith` 每次调用内部执行 `ctx->synchronize()`（`llama-context.cpp:3574`）——每步 ×N 槽同步。
2. 首次调用吸收 decode 提交的异步 DtoH 完成等待（`needs_raw_logits` 路径整块 [n_outputs × n_vocab] 拷贝）；compact 档同词表同步步数但 logits_ffi 减半（5.64 s vs 10.62 s），证明其中"图执行尾部溢出等待"占大头、纯 host 开销为辅。
3. 批量消除路径（`llama_set_sampler` backend 采样 + `llama_get_sampled_tokens` 单次读全部 argmax token）在 sys 0.1.153 **C API 已存在**，但 wrapper 0.1.152 未暴露；workspace `unsafe_code = "forbid"` 排除绕过。

**裁决：本阶段不动绑定（WP.2 冻结）。** 解除路径（均超出本方案，另行提案）：升级 llama-cpp-2 / 解禁 unsafe 直连 sys。backend 采样属第二档（argmax 语义交由 llama.cpp，平局行为不再由本仓库锁定），与第一档门禁不兼容，不在 A 实施。

## 3. A2b-argmax：total-order 可向量化 argmax（已合入，`7d06dd5`）

- `argmax_total_cmp`：`f32::total_cmp` 的 i32 总序 key（bits ^ 符号扩展技巧）+ `>=` 保留 last-max 平局，与原 `max_by(total_cmp)` **逐位等价**（含 NaN payload/±0/±inf/子正规）。
- 等价性测试：特殊值全对偶 + 平局行 + 5,000 行伪随机位模式（NaN 按位注入）对拍锁定。
- 三处调用点统一替换（窗口 seed / 窗口每槽 / 单流路径），两路径继续逐位一致。

### 门禁（Vulkan，金集两遍）

| 遍 | elapsed_ms | tok/s | argmax 桶 | window_total | 与 `a02d459` 冻结参照 |
|---|---:|---:|---:|---:|---|
| A 前基线（插桩） | 40 100 / 40 176 | 421.9 / 421.0 | 3.45 s | 30.43 s | document.md 逐字节一致 |
| A2b run1 | 38 259 | 447.8 | 1.80 s | 28.67 s | document.md 逐字节一致 |
| A2b run2 | 38 225 | 447.0 | 1.80 s | 28.73 s | document.md 逐字节一致 |

- 两遍偏差 0.09% < 2% ✓；argmax 桶 3.45 → 1.80 s（−48%）。
- **墙钟 −4.9%（40.18 → 38.24 s）**，解码段 −5.8%（30.4 → 28.7 s），tok/s 424 → 447（+5.4%）。
- **第一档门禁：PASS**（真实模型 `real_model_window_decode_matches_single_stream_prefix` 环境门 + 金集两遍与冻结参照逐字节一致 + 自一致性）。
- **判定**：墙钟 4.9% 略低于 A 阶段 5% 建议线，解码段口径 5.8% 过线。**采纳**，理由：① 语义零变化（逐字节），复杂度低（一个纯函数 + 属性测试）；② 与 C 叠加——批深 8 下步数减半，argmax 桶同比减半，两阶段收益相乘而非相加；③ 每项独立提交独立判定已满足（规则 1）。

## 4. 阶段 A 落点

| 口径 | A 前 | A 后 |
|---|---:|---:|
| 墙钟（金集，Vulkan） | 40.0–40.2 s | **38.2 s** |
| 解码 tok/s | 421–424 | **447** |
| 解码段 | 30.3–30.5 s | 28.7 s |

方案 v1.2 的 M1（≤32 s 现实线 / ≈23–26 s 上界）未达且按 T0.1 裁定撤回（缺口主体为绑定固有同步与 GPU 地板，host 可兑现份额以实测为准）。单文档终线上移至阶段 C 承接。
