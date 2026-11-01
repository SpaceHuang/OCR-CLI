# P0 扫描吞吐基准报告（2026-11-01，方案 §3 冻结协议）

**方案：** `docs/develop/scan-throughput-optimization-development-plan.md` §3
**目的：** 为 P1/P2/P3/P4 各阶段提供统一口径的对照基线与归因手段。本报告全部数字为实测值，后续阶段报告按同一协议对照。

## 1. 执行环境与版本冻结

| 项 | 值 |
|---|---|
| 机器 | Windows x64，RTX 4090 D 24 GB（非登记基准机，与 T1.4 预研同机） |
| 源码 | `feature/scan-throughput-optimization` worktree @ `7e4ee0d`（主干，复读退化方案合并后）+ 计划文档提交 |
| 二进制 | `cargo build --release -p ocrx --features vulkan`，CMake 生成器 Ninja + vcvars64（§5.5 配方）；sha256 `1876ac97436008fd85bc30a72c86fe6b…` |
| doctor | `backend=Vulkan`，`recommended_device=vulkan` |
| llama.cpp | 0.1.152 / sys 0.1.153（WP.2 冻结），内嵌修订 `9e3b928`（R0.2 工件同源） |
| 模型 | ovisocr2 reference GGUF revision `8a22290a…`（1.52 GB BF16 侧权重，缓存校验通过） |
| 门禁集 | `ovisocr2-draft-v1`，30 页 composite |

**输入钉定（R0.1 命令冻结）**：源 PDF 未随仓库分发（redistributable: false），本次以 `assemble-regression-pdf.py` 的页映射用 pymupdf 重建：
`inputs/source.pdf` sha256 `5d7adf7d9c62f698a2df9778dfd9376309fa99ce397198e759bc3bc5ec4695fb`。
30/30 页与源书页 72dpi 渲染 MD5 一致（rebuild-report 同口径），内容像素级等价；与既有 qpdf 重建 hash（`31fe8671…`）不同属打包器差异，不影响本报告内部对照（前后均用同一文件）。**后续阶段对照必须沿用同一 sha256 的输入。**

**命令**：`ocrx scan <source.pdf> --model-profile reference --output-dir <fresh>`（默认 auto 路由，device auto→Vulkan），`OCRX_GPU_REPORT=1` 采集遥测。由 `scripts/measure-scan-throughput.py` 驱动，每遍独立输出目录。

## 2. R0.1 基线（30 页金集，reference，两遍连跑）

| 遍 | elapsed_ms | 解码段 gen_ms≈ | 生成 token | tok/s（解码段） | load_ms | first_token_ms(min) | 完成页 |
|---|---:|---:|---:|---:|---:|---:|---|
| 1 | 55 774 | 45 572（81.7%） | 12 839 | 281.7 | 1 325 | 118 | 30/30，0 失败/截断/退化 |
| 2 | 54 866 | 45 278（82.5%） | 12 839 | 283.6 | 1 199 | 118 | 30/30，0 失败/截断/退化 |

- **两遍偏差 1.65% < 2% → R0.1 验收通过**，可作对照基线。
- 两遍产物（`page-NNNN.*`、`document.md`，除 `result.json`/`gpu-resource.json` 计时字段）**逐字节一致**；token 数完全相同（12 839）——当前主干确定性解码确认。
- `gen_ms` 由 `gpu-resource.json` 的 `output_tokens / output_tokens_per_sec` 精确反推（该报告无独立 `generation_time_ms` 字段，反推无损）。
- **基线值：54.9–55.8 s 墙钟 / 解码段 ≈45.4 s（≈283 tok/s）**。历史值 58.4 s（2026-10-30）与 T1.4 预研 71.9 s 均未复现（§5.5 已预警"不作可复现常数"）；本报告数字为方案 §8.3 要求的"复读合并后主干重测"基线。

## 3. R0.4 二段归因

| 段 | 30 页耗时 | 占比 | 说明 |
|---|---:|---:|---|
| 解码段（generation clock） | ≈45.3–45.6 s | 81–82% | 含 mmproj 编码/prefill + 逐 token 解码 |
| 非解码段（elapsed − gen） | ≈9.3–10.2 s | 17–18% | 见下分解 |

非解码段 ≈9.9 s/30 页分解（本次新观测，供 P3 定位上界）：

| 子项 | 量级 | 依据 |
|---|---:|---|
| 模型资产逐遍 SHA-256 校验 + 加载 + 后端初始化 | ≈5.3 s/次（每扫描一次） | 单页对照实验：native 路径扫描 0.13 s vs OCR 路径 6.8 s，差 ≈6.7 s 减去 load_ms 1.1 s 与生成段；`verify_manifest_files` 对 1.52 GB GGUF 全量 sha256（代码路径 `ocrx-models/src/lib.rs:394-408`）；Python SHA-NI 实测下界 ≈1.0 s，Rust 侧实测未拆分 |
| 渲染 + 像素预算缩放 + 提交（页级边际） | ≈81 ms/页 | R0.5b 1 页 vs 8 页差分的非生成段边际（§5） |
| 分析/原生路由/收尾等 | 余量 | — |

**结论**：P3（渲染/解码重叠）在该集上的可隐藏上界 ≈2.4 s（81 ms × 30 ≈ 4.3% 墙钟）；"逐遍模型校验+加载 ≈5.3 s"不随页数摊薄、不受 P3 影响，**不在本方案任务清单内**，登记为后续优化候选（页级到 job 级缓存校验属新方案范围，本方案不做）。

## 4. R0.2 解码上界参照

`llama-bench`（自 vendored sys 同源修订 `9e3b928` 构建，Vulkan，同 GGUF，`-p 0 -n 256 -r 3`）：

- **tg256 = 323.2 tok/s（stddev 0.6）** → 单 token 边际 3.094 ms。
- 数据：`perf-baseline-2026-11-01-data/r02-llama-bench-tg256.json`。
- 流水线解码段 283 tok/s ≈ 上界的 **87.6%**；每 token 边际差见 §5（R0.5）。

## 5. R0.5 每 token 边际开销归因

同一单页 OCR fixture（`dense-text-for-max-tokens.pdf`，`--pdf-mode ocr`）扫描，cap 128 与 2048（CLI 校验下限 128；协议原文 cap 32 低于该下限被 `exit 2` 拒绝——**协议适配：128 替代 32，差分逻辑不变**，按 v3.5 口径用实际生成 token 数），各 3 遍配对：

| 口径 | cap128 中位 | cap2048 中位 | Δt | Δn | **每 token 边际** |
|---|---:|---:|---:|---:|---:|
| elapsed_ms | 7 351 | 8 909 | 1 597 | 517 | 3.09 ms/token |
| generation clock | 575 | 2 260 | 1 690 | 517 | **3.27 ms/token**（主口径：runtime 自有时钟，不受 load 波动影响；配对极差 3.25–3.29） |

- 页面在 cap 2048 下 EOS 于 645 token（差分按实际 n）。
- **对照裸上界**：本仓库解码循环边际 3.27 ms/token vs 裸 llama.cpp 3.094 ms/token → 包装层附加 ≈ +0.17 ms/token（≈ 边际的 5%，≈ 金集墙钟的 ~3–4%）。
- **T2.1 判定输入**：该附加超过 2% 线，但其中 argmax 与每 token batch 分配的占比无法由本差分拆分；greedy sampler 替换已由 §1.3 修正 3 否决（净退化）。可兑现方向即 T2.2（batch 复用），其去留由 T2.2 实施后的同口径差分复测决定。
- 数据：`r05-cap128-clean.json` / `r05-cap2048-clean.json`。

## 6. R0.5b 页级固定开销归因

fixture：`figure-8-page-same.pdf`（figure-1-page 复制 ×8，同内容页）；cap 2048；1 页（`--pages 1`）vs 8 页，各 3 遍：

| 系列中位 | elapsed | gen clock | token | first_token_ms |
|---|---:|---:|---:|---:|
| 1 页 | 6 772 | 246 | 26 | 140 |
| 8 页 | 8 929 | 1 833 | 208（26×8） | 121（min 口径） |

按协议公式（生成时钟口径，与 R0.5 主口径一致）：

- **每页全成本** = (1 833 − 246) / 7 = **226.7 ms/页**
- 解码边际部分 = 3.27 ms/token × 26 = 85.0 ms
- **页级固定开销（生成路径内：bitmap/tokenize/context 创建/图像编码）≈ 141.7 ms/页**
- **交叉校验**：单页独立运行 `first_token_ms` 均值 140 ms ≈ 141.7 ms（该口径覆盖编码+页级固定之和的上界）——两条独立路径吻合。
- 非生成段页级边际（渲染+缩放+提交）= (7 096 − 6 526) / 7 ≈ **81.4 ms/页**（R0.4 采用）。
- **对 P4.1 的含义**：页级固定开销 ≈142 ms，占金集页均时长（1 859 ms）的 **≈7.6% ≥ 2% 阈值**——P4.1（消除每页 `new_context`）的收益条件成立；若 P4 收缩，回退条件数据源即本节。
- 数据：`r05b-1page.json` / `r05b-8page.json`。

## 7. R0.3 观测口径（nvidia-smi dmon）

金集扫描全程 1 s 采样（`dmon -s um`）：模型加载完成后解码期 **SM ≈80%、显存控制器 ≈61–62%**（p90 ≈81/62），渲染期回落（SM 低至 13–47%）。单流解码**带宽不饱和**（mem-ctrl ~62%，余量 ~38%），形态与"亚 1B GEMV + kernel 间隙"一致；任务管理器 ≈5% 读数与 dmon 口径差异属统计方法不同，不作为 KPI（§1.1）。P4 落地后按同口径复测验证瓶颈推向饱和。数据：`r03-dmon.txt`。

## 8. R0.6 合批收益上界预估

`llama-bench` 在修订 `9e3b928` 已移除并行参数（`-np` 不存在，`error: invalid parameter`）；改用同源 **`llama-batched-bench`**（continuous batching 参照实现）实测批深曲线（`-npp 0 -ntg 256`，warm）：

| 批深 B | 总吞吐 tok/s | 相对单流 | 4090 D 带宽理论（≈1 TB/s，权重 1.52 GB） |
|---:|---:|---:|---|
| 1 | 321.1 | 1.00× | — |
| 2 | 595.5 | 1.85× | 理论上界 ≈2× |
| 4 | 1 030.7 | 3.21× | 理论上界 ≈4× |
| 8 | 1 527.6 | **4.76×** | — |
| 14 | 1 554.0 | **4.84×（饱和）** | — |

- **饱和点在 B≈8–14 之间，实测上界 ≈4.8×**（低于"带宽完全饱和 = 6.6×"的理论值，瓶颈移至算力/其他）。
- 注：首遍冷启动运行 B=2/4/8 行受 Vulkan 管线首次编译污染（非单调），warm 复测曲线如上且 B=4/B=8 重复行稳定（≤0.3% 漂移）。
- **P4.2 设计输入**：起始批深 2、爬坡区间 2→8；结合 §7.3 ① 的每页 ≈17.6k token 预算与 6 GB 显存预算准则（N=4 增量 ≈0.85 GB、N=8 ≈1.7 GB），默认批深的曲线侧依据为"B=8 已获 4.76×、B=8→14 仅 +1.7%"。
- 数据：`r06-batched-bench.txt` / `r06-llama-bench-parallel.json`（空的失败尝试存档）。

## 9. P0 验收小结

| 任务 | 验收 | 状态 |
|---|---|---|
| R0.1 命令冻结 | 两遍 <2% 偏差、报告落档 | ✅ 1.65%，本报告 |
| R0.2 上界参照 | tg256 323.2 tok/s 落档 | ✅（llama-bench 自 `9e3b928` 构建成功） |
| R0.3 观测口径 | 解码期 mem-ctrl ~62% 不饱和结论 | ✅ |
| R0.4 归因规则 | 二段拆分 + 非解码段分解 | ✅ §3 |
| R0.5 每 token 边际 | 3.27 ms/token（gen 口径） | ✅ §5（cap 32→128 适配已记录） |
| R0.5b 页级固定 | ≈142 ms/页（ft 交叉校验吻合） | ✅ §6 |
| R0.6 合批上界 | batched-bench 曲线，饱和 ≈4.8× | ✅ §8（llama-bench 无并行参数，改用 batched-bench 已记录） |

**P1 阶段门禁对照值（冻结）**：金集 `document.md` sha256 `222681b986798edfb4e0a0f9…`（`r01-golden-reference-01/` 为逐字节参照）；墙钟 54.9–55.8 s；解码 tok/s ≈283。

**原始数据**：`docs/test/reports/perf-baseline-2026-11-01-data/`（含各扫描完整 result.json/gpu-resource.json 与 harness JSON 记录）。
