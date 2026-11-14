# OvisOCR3 接入验收报告（O3-T29）

> 计划：`docs/develop/ovisocr3-integration-development-plan.md` v1.5
> 日期：2026-11-14；工作树 `.worktree/ovisocr3-integration`（分支 `feat/ovisocr3-integration`）
> 证据链：spike 记录（含全部探针与两次生产证据修订）、实施日志（逐任务）、各提交的判别测试、
> `docs/develop/spike-assets/`（供应链/网格/对照样本 JSON）。

## §11 完成定义逐项核对

### 1. G1 — `--model ovisocr3` 全链路 ✅（能力面）/ ⚠️（质量面，见 §差距）

- ✅ install → verify → scan（native/empty/OCR 路由、续扫、取消 130、多文档一次加载、
  退化阶梯、GPU 回退语义）全链路走通：真实资产双模型验证（C7/C14 e2e：prepare→窗口→
  checkpoint→零加载续扫→多文档共享 session；C11 故障注入双模型 4 用例全绿）。
- ✅ `doctor --model ovisocr3` 新面（golden 创建即冻结）。
- ✅ result.json ovisocr3 路径已从 `DocumentOutcome.identity` 接线（N1；真实资产 e2e 验证
  派生对象 id/name/adapter/runtime/revision 全对）。
- ⚠️ **质量差距（本机 30 页复合样本）**：scan 产物显著差于 OvisOCR2 同页同参
  （输出量 ~21.0KB vs ~47.0KB；17/30 页相似度 <0.9；14/27 OCR 页近空）。根因分析见 §差距。
  **墙钟：ovisocr3 819s vs ovisocr2 528s**（串行地板 + 2048 网格的成本）。

### 2. G2 — 字节不变集 ✅

§3.3 全项复验通过（每提交门禁 + 专项 golden）：DEFAULT_MODEL 不变、OvisOCR2 四档 manifest
pin 不动、`OCR_MARKDOWN_V1` 字节（pack 私有化 + 判别测试）、DECODE_GENERATION=3、
model/doctor/scan 缺省形态输出逐字节保持、页产物/v6 checkpoint/v5 迁移/StepProfile/
CommitProfile 黄金串、result.json ovisocr2 形态（含历史 `llama.cpp@0.1.152`）。
**预期差异仅 §8.3 登记四项**：① `model list` 无参新增 ovisocr3 行；② model 子命令
`--model` 参数面；③ doctor `--model` 参数面（无参不变）；④ help 文本。全部有 golden
断言钉住。

### 3. G3 — 跨模型 checkpoint 否决 ✅

双方向判别测试钉死（pre/final compare；同模型通过、混合 profile 保持既有否决）。
e2e：零加载续扫双模型验证（空 capture 文件 = 工厂零触达）。

### 4. G5 — 缺口最小闭环 ✅

- per-model prompt 身份：pack 注册表 + 全读取点贯穿 + **实际请求 prompt 逐字节捕获断言**
  （C7，双模型真实 e2e）+ checkpoint 三元组一致。
- per-model 绑定：三读取点一致（session/static/checkpoint；C9/C14）。
- 网格裁定：D-5 经生产证据两次修订后落地——`SUPPORTED_IMAGE_TOKEN_GRIDS=[2048]` 不变、
  阶梯零改、**基础预算 per-model（ovisocr3=2048 / ovisocr2=1024 不变）**、**混合 SSM
  串行窗口地板（ovisocr3 window=1）**；全部有判别测试。

### 5. 门禁 ✅

`cargo fmt`/`clippy -D warnings`/`cargo test --workspace --locked`/
`validate-architecture-boundaries.py`/`test-release-scripts.sh`/`bootstrap-check.sh`
全绿（最后全量门禁于 T21/T22-T24 提交前执行；负向测试：架构白名单增补后 unknown-crate
与越界依赖即违规的既有机制未削弱）。

### 6. G4 — 质量证据 ⚠️ 部分交付（定界诚实）

- ✅ 30 页复合样本同 harness 对照（`spike-assets/ovisocr3-comparison-sample.json`，双模型
  Q8_0 同参 CPU）+ 资源观测（§资源）。
- ✅ 全量 OmniDocBench 复评 runbook 固化
  （`docs/test/reports/omnidocbench-comparison-runbook.md`——基线复跑对齐 84.92 → 全量
  → 逐页归因的三步程序）。
- ❌ **全量 OmniDocBench 分数对照未在本窗口执行**（CPU 全量 ≫ 8 小时算力，超出本会话
  窗口；runbook 可直接续作）。官方模型卡分数仅背景引用（96.74），未与本仓口径混比。

### 7. 合规 ✅

README/README.zh-CN Scope 双模型化；THIRD_PARTY_NOTICES(.zh-CN) 增补 OvisOCR3
（Apache-2.0、michalland/OvisOCR3-GGUF @`7be644a3…` provenance + 冻结哈希、灾备仓库
记录）；发布说明模板含供应链章节。

### 8. 边界 ✅

§3.2 非目标未越界（无结构化模式/KIE/双图/动态注册/默认切换的悄悄实现）；O3-T13b 保持
【不触发——D-2 分支 1】注记，未无声实现。

### 9. 可追溯 ✅

spike 记录（D-2 三分支裁定 + 最终 MTMD 输入证据 + 两次生产证据修订）、实施日志（逐任务 +
提交序列）、验收报告（本文档）落 `docs/develop/`；决策表状态：D-1..D-11 全部【已冻结】
（D-2/D-4/D-5 为 spike 裁定 + 生产证据修订，修订过程全程留痕）。

---

## 质量差距分析（D-GATE 关键输入）

**现象**：同页同参（Q8_0、CPU、20480 ctx、2048 图像预算、串行）下，OvisOCR3 scan 产物
输出量约为 OvisOCR2 的 45%（21.0KB/47.0KB），17/30 页相似度 <0.9，其中 14/27 OCR 页
近空（模型立即 EOS）。

**已排除**（全部实验留痕于 spike 记录 §串行地板与修订注记）：

| 假设 | 实验 | 结论 |
|---|---|---|
| 图像 token 预算不足 | 单页矩阵（页 6：1024→0 字节 / 2048→2892 字节） | 部分成立 → 已修（per-model 2048 默认），但全页扫描仍有空页 |
| 窗口批量解码 | batch_depth=8 vs =1 对照 | **排除**（空页集合逐字节相同） |
| 跨页 KV/SSM 状态污染 | 同 runtime 连续生成页 2→页 4 复用探针 | **排除**（直驱路径复用两页全完整） |
| 资产/pin 问题 | sha256 冻结校验 + 冻结 pin 加载/生成 | 排除（直驱完美输出证明模型+pin 本身可用） |

**根因定位（诚实结论）**：OvisOCR3 在 **scan 生产路径**上存在未定位的生成退化（直驱路径
同页同参完美，scan 路径间歇性立即 EOS，空页集合跨运行非确定）。已收敛到 scan 路径特有的
某差异因素（候选：actor Generate 处理与直驱 `generate_with_progress` 的实现差异、渲染/
生成进程级并发的干扰等）。**需独立 root-cause 立项**（建议：在 scan 串行路径中逐段对照
直驱探针的实现——WorkerTask 构造、mtmd tokenize 参数、decode 循环细节——二分定位）。

**正面证据（模型本身可用）**：直驱路径同页输出质量优秀（LaTeX 公式、HTML 表格、标题层级、
中英文混排全部正确；spike 探针 + 归因探针多次复现）——接入的平台面（manifest/绑定/adapter/
网格预算/串行语义）没有问题，缺陷收敛在 scan 路径一处。

**D-GATE 建议**：**不切换默认模型**（质量不及 OvisOCR2 同 harness 基线，R5 口径）；
接入按"能力可用 + 差距分析入档"交付完成。后续顺序建议：① scan 路径 root-cause 立项；
② 修复后按 runbook 执行全量 OmniDocBench 对照；③ 复议 D-GATE。

## 资源观测（O3-T27，CPU 单机口径）

| 项 | OvisOCR2（quality/Q8_0） | OvisOCR3（quality/Q8_0，2048 网格，串行） |
|---|---|---|
| 30 页复合墙钟 | 528s | 819s（+55%） |
| 路由 | native 1 / ocr 27 / empty 2 | 同 |
| 完整/截断/退化 | 30/0/0 | 30/0/0 |
| load 时间 | ~0.5s 量级（历史里程碑口径） | 502ms（spike 实测） |
| batch depth | 8（设备分档） | 1（串行地板） |
| GPU 对照 | 不适用（本机 CPU-only 构建；T107 形态的 GPU 观测留待 GPU 环境） | 同左 |

## 遗留与后续工作清单

1. **scan 路径生成退化 root-cause 立项**（最高优先；证据与候选因素齐备，见 §差距）。
2. 全量 OmniDocBench 对照复评（runbook 就绪；修复后执行）。
3. ~~检空盲区~~ 已修复（评审 BLOCK 响应）：空 OCR 输出进入退化阶梯，耗尽后提交为退化页
   （不再静默 Complete、不再被 --resume 复用）；判别测试钉死。
4. ovisocr3-draft-v1 fixture 升级为 approved truth（人工评审块级内容与 bbox）。
5. 混合 SSM 串行地板的移除条件：上游 pin 修复混合模型批量语义后重评（D-5 修订注记）。
