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
| 基线二进制（金集门用） | `C:/t/m23/ocrx-base-16577de.exe`，sha256 `a2c77197fb738012515aa0b0b89aa0385bed8e09f05526f5cc3c814dc0994254`，main@16577de 同配方（tgt-base，7m48s） |
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

## 4. 结果（第一轮全量，2026-11-15 判定）

- 扫描：1,651/1,651 页（11:57–14:05，~2h5m；vulkan/4090D）；退出码 10 =
  BLK-7 冻结语义（含退化页的部分成功），非失败。
- **Overall(all) = 88.06 —— 门（≥ 89.5）FAIL**（landing 87.85，+0.21；投影 89.8±0.3 落空）
- 分量：text 0.9114 / CDM 0.9442 / TEDS 0.7862；en 84.45 / zh 89.92
- 坏页账本：degenerated 111 / truncated 7 / 交集 1 → 总坏页 ~117（landing 121，仅 −4）
- attempts 分布：1→1432、2→80、3→4、4→5（M2 rung 逃逸仅 5）、5→130（耗尽）
- **根因链（二进制 A/B，diag1–7 记录在案）**：
  1. spike 只放宽光栅钳制、模型输入保持 2880² 超采样——本实现首轮误把模型
     输入放到 4096²，已修（raster-only）；
  2. spike 的 4096² 光栅覆盖整个重试路径（含切栏档）——本实现已修
     （RETRY_RASTER_BUDGET，attempts ≥ 2 全 budgeted）；
  3. 两修后 68 页语料逃逸仍仅 2/68（spike 38/68），而输入位形已与 spike
     二进制 audit 逐字段一致（rendered=2754x3939;model=2408x3444;scaled=true）
     ——**同输入不同输出 = spike 二进制（40317ebc）含不可从当前源复现的
     构建态**（Nov 14 主树瞬态编辑混入嫌疑）。D-17 的 GO 证据链失效。
- 金集门：第一轮已完成 run1（30/30 提交，修复版二进制）；run2/基线轮因
  批处理退出码误判中断，待修复版终轮一并执行。
- 结论（第一轮）：FAIL。rung 保留。
- **第二轮（修复版 06e8a501，同日）**：根因链补全——spike 的救援还需要
  walk-wide 2048 解码网格（diag8 单页实证 + diag9 68 页验证 37 逃逸/31 残余，
  与 spike 38/30 对齐）。全量重扫 1,651/1,651，超预算人群 37 救援
  （5 M2 + 32 切栏）/ 31 残余，与 68 页验证逐页一致（跨运行确定性确认）。
  D-17 证据链恢复（spike 协议 = retry-wide 超采样光栅 × walk-wide 2048 网格）。
- **第二轮判定：Overall(all) = 88.45 —— 门（≥89.5）仍 FAIL**，但 vs landing
  **+0.60**（第一轮 +0.21）。分量：text 0.9277（vs landing **+1.87pt**——spike
  逐页算术 37×0.0617≈+1.64 兑现 ✓）/ CDM 0.9430（−0.13）/ TEDS 0.7827（+0.04）；
  en 83.90 / zh 91.08。账本 121 → **86**（−35）。
- 净收益分析：retry-wide 2048 改变了全部 attempt-2 逃逸页（~80 页）的解码
  像素——text 大涨的同时 CDM/TEDS 回吐 0.47pt（en TEDS 0.6495→0.6324 为主要
  回吐点）。spike 的 +2.0 投影假设 CDM/TEDS 零影响，实际兑现率 30%（+0.60）。
  即便 CDM/TEDS 回吐全数追回（理论 88.92），仍不达 89.5 门。
- **最终判定：B-M2.3 FAIL（88.45 < 89.5）**；净收益 +0.60 入账；rung 保留。
  后续优化方向（另立文档）：attempt-2 逃逸页的 CDM/TEDS 回吐定位与
  rung 级网格/光栅组合微调；en TEDS 回吐集中点核查。
- 产物命名教训：复评轮次必须隔离命名（本轮 round-1 明细被 round-2 覆盖，
  分量均值留存于本记录，逐键明细不可恢复）。

## 5. 偏差记录

- 构建排障：C1041 × 3（PDB 并发/路径长度误诊→实为 MAX_PATH-250），解法
  CARGO_TARGET_DIR 短路径；mspdbsrv 僵尸进程清理无效果佐证路径根因。
- 全量扫描实测速率 ~13 页/分钟（vulkan 后端，4090D，含重试页 4096² 渲染）。
- 退出码陷阱：`document_exit_code` 对含退化/截断页的文档冻结返回 10——
  批处理链不得把 errorlevel≠0 一律当失败（0/10 均为成功，≥11 才是失败）。
