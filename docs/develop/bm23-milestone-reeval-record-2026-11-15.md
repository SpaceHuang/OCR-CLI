# B-M2.3 复评记录：M2 rung 全量 balanced 里程碑（进行中）

**分支：** `feat/next-phase-quality`（worktree `.worktree/next-phase-quality`）
**验收门：** Overall ≥ **89.5**（投影 89.8 ± 0.3）+ 坏页账本复核（68 → ~31 mosaic 残余）
**状态：** 扫描进行中 → 导出/评测/金集门已链为无人值守链

## 1. 构建与身份（Q7.1 manifest 纪律）

| 项 | 值 |
|---|---|
| 里程碑二进制 | `C:/t/m23/tgt/release/ocrx.exe`，sha256 `c1cfe36b258923c782883d6a0489f0016930bc75f317ff6ed550e8e40bb45430` |
| 源 | feat/next-phase-quality @ B4.2 提交后（T-PROJ→B4.1 全链已合入分支） |
| 构建 | `cargo build --release --locked -p ocrx --features vulkan`，vcvars64 + Ninja（C:/t/m23/build-m23.cmd）；**CARGO_TARGET_DIR=C:/t/m23/tgt（短路径）**——worktree 深路径曾把嵌套 vulkan-shaders-gen try-compile 对象目录推过 MAX_PATH-250（C1041，三轮排障记录见构建日志） |
| 基线二进制（金集门用） | `C:/t/m23/ocrx-base-16577de.exe`，main@16577de 同配方（tgt-base） |
| 门禁 | fmt + clippy 0 警告 + workspace tests 全绿（构建前） |

## 2. 命令链（D-30 对照配置）

```bash
# 全量 balanced 扫描（对照配置 = 新默认，M2 rung 已含于阶梯）
ocrx scan pdfs -o scans/m23-m2rung-balanced --model ovisocr2 \
    --model-profile balanced --dpi 300 --non-interactive --progress off --log-level warn
# 导出 + 官方评测（quick_match，all/en/zh）
export-omnidocbench-predictions.py --scans scans/m23-m2rung-balanced ...
run-omnidocbench-eval.py ... --name m23-m2rung-balanced-{all,en,zh}
```

链式执行：`C:/t/m23/post-scan.cmd`（等待 document.md + SCAN-OK → 导出 → 三分评测），
金集门 `C:/t/m23/gold-after-post.cmd`（POST-SCAN-DONE 后执行）。

## 3. 金集门（B-M2.1/2.2 验收：代次内零差异 + 确定性）

- **冻结子集（本轮定义并落档 `C:/t/m23/gold-pages.json`）**：pdfs 排序后排除
  B0a 区域页（744 页）的前 30 页（首：book_en_40...Probability...page_064）。
  说明：原 0.1.152 代次金集清单未随 D-15 跨代次复用（D-15：零差异仅对其自身
  代次成立）；本轮以同代次双构建（feat 分支 vs main@16577de，同 wrapper）
  双跑实现"代次内零差异 + 确定性"两门。
- 判定：run1 vs run2 逐字节一致（确定性）；run1 vs base 逐字节一致
  （正常页零差异——M2 rung 仅存在于重试路径，正常页按构造不变）。
- 比对器：`C:/t/m23/compare-gold.py`（page-*.json / document.md sha256）。

## 4. 结果（待填）

- [ ] 扫描完成时间 / 页数 / 耗时
- [ ] Overall(all) = ____（门：≥ 89.5）
- [ ] 分量：text = ____ / CDM = ____ / TEDS = ____
- [ ] en/zh 切分
- [ ] 坏页账本复核：degenerated ____ / truncated ____ / 交集 ____（landing 121，
  预期 ~89：68 超预算页由 4096² rung 救援后 mosaic 残余 ~31）
- [ ] 金集门：确定性 PASS/FAIL；零差异 PASS/FAIL
- [ ] 对账：官方指标文件名清单入 reports/

## 5. 偏差记录

- 构建排障：C1041 × 3（PDB 并发/路径长度误诊→实为 MAX_PATH-250），解法
  CARGO_TARGET_DIR 短路径；mspdbsrv 僵尸进程清理无效果佐证路径根因。
- 全量扫描实测速率 ~7 页/分钟（vulkan 后端，4090D）——显著慢于 README 的
  20–30 min 估计（该估计来自 CUDA 构建），后续里程碑排期按 vulkan 速率校准
  或改用 CUDA 构建。
