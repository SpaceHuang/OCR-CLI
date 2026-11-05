# 扫描吞吐优化三期总报告（backend sampling spike + commit-path 插桩与裁决）

**日期**：2026-11-05
**方案**：`docs/develop/scan-throughput-phase3-development-plan.md`（v2.1，复审 PASS）
**分支**：`feature/scan-throughput-phase2`（worktree `D:\Develop\OCR-CLI\.worktrees\scan-throughput-phase2`）；S0 spike 在 `scratch/backend-sampling-spike`（保留不合入）
**执行方式**：TDD + 硬性评审门禁（v2.1 五项阻断复审解除后进入实现）；全部性能数字为同机（RTX 4090 D）Vulkan 实测
**结论**：**BS no-go（−2.5% ≪ 8% 合入线）；fallback P1/P2/P3 全部 no-go（−1.9% / 0 / −3.2%）**——两个决策门均按计划机制以实测数据关闭，未硬凑任何未过线杠杆；已 banked 的 md-small-20 5.9× 不受影响。唯一产品化交付 = T1 commit-profile 诊断通道（两主线通用资产）。

## 1. 落点总览

| 阶段 | 内容 | 结果 | 提交 |
|---|---|---|---|
| S0.1 | wrapper 版本审计 + 可实施性认定 | 候选 0.1.161，四项安全 API 全覆盖，无需 vendor patch | `111c691`（feature） |
| S0.2 | scratch pins 升级（0.1.152/153→0.1.161）+ 双模式采样探针 | Windows 构建配方钉定（Ninja + vcvars64 + `CL=/FS` + 短 `CARGO_TARGET_DIR`）；四契约 + 2-slot 窗口对照全过；金集页 1 divergence=identical | `8076f24`（scratch） |
| S0.3 | 正确性 + divergence（先于性能采集） | 全量 391/0 + 真实模型 gated 3 项全过；金集两页 identical；**平局语义差异钉入测试**（host last-max-wins vs greedy first-max） | `e5e4717`（scratch） |
| S0.4/S0.5 | 金集 A/B 两遍 + md-small-20 + 双条件决策门 | logits_ffi/argmax 精确归零（生效证据）但墙钟仅 −0.78 s（**−2.5% ≪ 8%**）——同步点转移而非消失；**BS no-go** | `2fcbbd5`（scratch） |
| T1 | `CommitProfile` 插桩（generation 键控 + 共享 sink + 双提交路径 + finalize；端到端插桩测试先行） | 合入；workspace 395/0；金集/md-small-20 墙钟闭合校验通过（残差=固定税，精确吻合） | `8fbe735` |
| T1.4 | 实测数据归档 + P2/P3 目标值修订一次 | 金集提交段 2.07 s/28.56 s（7.2%）；**md-small-20 提交段 0.185 s/文档（19.9%）**，encode 实测仅 18 ms/文档 | `8a064d6` |
| 裁决 | 附录 A 模型代入 → P1 −1.9% / P2 0 / P3 −3.2%，全部低于判据线 | **P 杠杆全部关闭归档**（未实施、未硬凑） | `6200ec8` |

## 2. 决策门数字（全部可复算）

- **BS**：同构建 A/B（`OCRX_BACKEND_SAMPLING` 开关）：a=30.84/30.87 s vs b=30.07/30.06 s → **−0.78 s（−2.5%）**；`llama_get_sampled_token_ith` 内部 `synchronize()` 使每步同步点转移而非消失，decode_wait 反升 0.2–0.3 s；md-small-20 无收益。
- **P1**：E=0.018 s/文档 ≪ D=0.67 s → 编码全额隐藏即收益全额 = 0.36 s → **−1.9%**（<5% 线）；金集 −1.4%（<2% 回滚线）。
- **P2**：E 已全额隐藏，max(0, E−D−R)=0，**增量为零**。
- **P3**：可消除对象 encode+write+finalize = 0.030 s/文档 → −3.2%；消除量占比 3.2%/16% **< 30% 判据**。
- render_wait（0.155 s/文档）为 demand 渲染等待，属渲染供给问题（一期 K=2 预取已压至实测水平），非 P 杠杆对象——更深预取列为独立提案素材。

## 3. 交付资产

1. **T1 commit-profile 通道**（唯一产品化交付）：`commit-profile.json` 逐页四段计时 + 终态 totals + finalize，`OCRX_STEP_PROFILE` 同开关，取消/失败零写盘；后续任何提交路径提案的第一测量工具。runtime `StepProfile` 与二期可比性一字未动。
2. **S0 spike 设施**（scratch 分支保留）：双模式采样探针、平局语义测试、wrapper 0.1.161 API 迁移完整提交（4 类破坏面 + Windows 构建配方）——未来任何升级提案的工程底稿。
3. **实测数据集**：`docs/test/reports/phase3-t1-2026-11-05-data/`（金集 + 20 文档逐份 profile）、scratch 分支 probe-run 原始日志。
4. **文档**：`phase3-s0-wrapper-audit-2026-11-05.md`、`phase3-s0-spike-2026-11-05.md`（S0.1–S0.5）、`phase3-t1-2026-11-05.md`、`phase3-fallback-verdict-2026-11-05.md`。

## 4. DoD 对账（计划 §7）

- [x] S0 spike 报告 + 决策记录（双条件裁决：可实施性 PASS / 收益 FAIL → 不做）✅
- [x] T1 数据落档（两主线通用）+ 端到端插桩测试先行 + 墙钟闭合（只用 `render_wait_us`）✅
- [x] BS（若做）——不做；裁决与全部数据归档 ✅
- [x] fallback（若走）：逐项判定以 T1 实测上界裁决（A2a 先例：实施前 no-win 判定），关闭归档 ✅
- [x] 全量 `cargo test --workspace` 395 passed / 0 failed + fmt/clippy 零警告 ✅
- [x] 阶段报告系列 + 本总报告 ✅

## 5. 未尽事项与移交

1. **pins 0.1.161 升级提案**：性能论据被 T1.4 修正推翻（同代码对比 0.1.161 反慢 ≈2.3 s；S0.4"−9.4%"的基数是二期整改前构建）；仅存绑定能力论据，推进与否归独立提案（WP.2 重审纪律 + `;decode=<gen>` 代次键设计已在计划规则 3 冻结）。
2. **更深渲染预取**（render_wait 0.155 s/文档的进一步隐藏）：属渲染供给优化，非本计划范围；以 `commit-profile.json` 为第一测量工具的独立提案素材。
3. **CUDA 后端复测**：延续一期原则归发布流程。
4. **scratch 分支处置**：保留不合入（含 `OCRX_BACKEND_SAMPLING` 开关——该 env 开关仅存在于 scratch 构建，主线无此行为）。
