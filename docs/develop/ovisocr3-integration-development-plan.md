# OvisOCR3 接入开发计划（基于多模型可扩展架构）

**状态：** 已评审通过，进入实施（v1.5 终稿，2026-11-14 独立重审 PASS；Phase 0 可立即启动）
**修订记录：**
- 评审闭环（2026-11-14）：v1.5 独立重审 **PASS**（`docs/review/ovisocr3-integration-development-plan.v1.4-independent-review.md` 复核轮）——C13/C13b 拆分归属确认一致（T13b 仅 runtime 双策略/串行窗口/OvisOCR2 不变；C13b 实际 adapter 联合 golden 归 O3-T11），可变档位推荐、doctor 任务顺序及全部既有阻断保持闭环；真实模型兼容性、prompt 分支与网格配置由 Phase 0 spike 裁定，属计划内实施门槛、非文档阻断。评审链：v1（2 BLOCK）→ v1.1（1 BLOCK）→ v1.2（5 BLOCK）→ v1.3（3 BLOCK）→ v1.4（1 BLOCK）→ v1.5 **PASS**，累计 12 项 BLOCK 全部修订闭环。
- v1.5（2026-11-14）独立重审响应（`docs/review/ovisocr3-integration-development-plan.v1.4-independent-review.md`——v1.4 三项修订经复核全部闭环，仅剩 1 项阶段序残留）：
  - **BLOCK-1（T13b 联合 golden 反向依赖 T11 的实际 adapter）**：按评审建议拆分 C13——**C13**（O3-T13b）= runtime 双策略 + 串行/窗口两路径 + OvisOCR2 最终 MTMD 输入逐字节不变（本任务时刻实际 OvisOCR3 adapter 尚不存在，不以未来 adapter 的联合断言为完成条件）；**C13b**（O3-T11，仅分支 3 触发）= 以**实际 `ocrx-ovisocr3` adapter** + runtime `EmbeddedInPrompt` 策略验证 OvisOCR3 最终 MTMD 输入 golden（标记位于模板内正确位置、运行时不重复前置）。同步 D-2/O3-T13b/O3-T11/§8.2/§10.2。
- v1.4（2026-11-14）独立重审响应（`docs/review/ovisocr3-integration-development-plan.v1.3-independent-review.md`；**下文 BLOCK-1..3 均指该重审编号**；v1.3 五项修订经复核闭环——scan 阶段序/实际 prompt 与 static identity 验收/doctor 无参冻结/result.json 接线均确认）：
  - **BLOCK-1（可变档位推荐算法未泛化）**：`recommend_profile` 回退从固定 `compact`（recommend.rs:24-41，`["balanced"]`@8GiB 单测固定失败）泛化为**模型声明序首个实际可用档**——OvisOCR2 完整四档下等价现行为（逐字节不变），全部非空子集有确定 auto 行为；6 GiB 资源下限双模型统一；`["balanced"]` 断言有意翻转登记为新语义判别；D-7/§4 行/O3-T07/C15 全用例同步（仅 balanced/仅 quality/仅 reference/缺 compact 且首选缺失等）。
  - **BLOCK-2（doctor 阶段序倒置）**：采纳评审方案 1——O3-T20 收窄为 scan/model 两面收口（§8.3 ①②④）；doctor 的解析（main.rs:649-655 扩展）/实现/help/合法非法用例/golden 全部归 O3-T21 同任务闭环；D-11 明确任务闭环归属；§10.2 同步。
  - **BLOCK-3（D-2 分支 3 条件阶段矛盾）**：O3-T13b 排序前移至 **O3-T11 之前**（分支裁定在 O3-T05 冻结→Phase 2 开工前已知触发状态；分支 3 下 O3-T11 的 `EmbeddedInPrompt` encode 依赖 T13b 策略字段）；O3-T13 不再承担分支判定；C13 扩展 **OvisOCR3 分支 3 最终 MTMD 输入 golden**（adapter 渲染与 runtime 策略联合判别）；D-2/O3-T11/T05/§10.2 同步。
- v1.3（2026-11-14）独立评审响应（`docs/review/ovisocr3-integration-development-plan.v1.2-independent-review.md`；**下文 BLOCK-1..5 均指该独立评审编号**；事实修正 N1 一并采纳）：
  - **BLOCK-1（阶段序依赖倒置）**：O3-T14 收窄为"fake 双模型 service/prepare 矩阵（不含 scan）"；scan 全链路矩阵移 O3-T19 承接；scan `--model` 解析放行前移并入 O3-T16（O3-T20 只留 help/错误串/golden 收口）；C7 任务归属改 O3-T16/T19。
  - **BLOCK-2（chat template 分支不可实施）**：D-2 重写——裁定依据改为**运行时最终拼接后的完整 MTMD 输入文本**（`llama_mtmd.rs:828-834`/`1193-1200` 无条件前置 `mtmd_default_marker()`，adapter 不得单侧承担模板拼接）；三分支唯一落点（原生直喂 / 前置等价复用 / 触发条件任务 **O3-T13b**：`EncodedTask`/`WorkerTask` 媒体标记策略 `BeforePrompt`/`EmbeddedInPrompt`，串行+窗口两路径同改，OvisOCR2 逐字节不变判别 C13）；O3-T03 完成判定同步；R8 登记。
  - **BLOCK-3（模型选择验收未闭环）**：O3-T15 扩为**全读取点贯穿**（`artifacts.rs:191` 请求构造、`app.rs:434-445` 预比较、`scan/compatibility.rs:51-75` checkpoint）+ 无残留全局常量读取点静态检查；O3-T12 扩为**三读取点统一**（`service.rs:219`/`489`/`259`，含 `static_identity` per-model）；C7 终点断言扩为 prepare 身份 + 实际 `TaskRequest.prompt` 逐字节捕获 + checkpoint 三元组一致；新增 C14（static per-model + 同模型零加载续扫复用 + 预比较/终比较判别）。
  - **BLOCK-4（doctor 要求冲突）**：采纳评审方案 1 并固化为 **D-11**——无参 doctor 输出逐字节不变（= 缺省模型就绪度语义，现有 golden 即缺省形态 golden）；新增 `doctor --model` 选择器 + ovisocr3 新面（创建即冻结 golden，schema 2 不动）；聚合双模型概览登记为未来工作；§3.3 第 8 条/§8.3 预期差异增为**四项**。
  - **BLOCK-5（缺档分支不可执行）**：D-7 改**可变档位集**——GO 条件 = 至少一个可用档位（含 mmproj F16）、四档为期望非硬前提；`list/info` 展示元数据源从固定取 `compact`（`lib.rs:142-155`/`183-197`）改为**模型声明序首个可用档**；O3-T01/T06/T07 同步；新增 C15（缺 compact/缺中间档/仅单档判别）。
  - **N1（result.json 并非通用派生）**：O3-T21 重写——主要实现工作 = 从 `DocumentOutcome.identity`（app.rs:141）接线至报告字段（现 `ScanResultFields` 硬编码，`main.rs:1799-1845`）；ovisocr2 缺省形态保持逐字节不变（含历史 `llama.cpp@0.1.152` 值；0.1.152→0.1.161 修正登记为独立契约变更）；§4 增补 result.json 行。
  - 同步修订：§3.1 G1/G2、§3.3 第 8 条、§4（adapter 选择/scan prompt/CLI 门禁行更新 + doctor/result.json/MTMD 三行新增）、§6 D-2/D-6/D-7/D-11、§7 T01/T03/T06/T07/T12/T13/T13b/T14/T15/T16/T19/T20/T21、§8.2 C7/C13/C14/C15、§8.3 四项、§9 R8、§10.2 勾选行（含 T13b，任务总数 30）、§11 完成定义。
- v1.2（2026-11-14）复审响应（`docs/review/ovisocr3-integration-development-plan.v1.1-review.md`——v1 两项 BLOCK 修复核实通过；新发现 1 项阶段序 BLOCK + 2 项措辞非阻断）：
  - **BLOCK-3（阶段序依赖倒置）**：`model verify` = 内容校验 + 绑定校验（`verify.rs:107`），OvisOCR3 manifest 的 `adapter="ovisocr3"` 在 O3-T10（Phase 2，绑定 per-model 泛化）之前必然 `IncompatibleBinding`，与 O3-T09（Phase 1）原完成判定"双模型管理面走通"冲突。按**方案 A** 收窄：O3-T09 ④ 改为 install/remove/list/info/recommend 双模型用例 + verify 双模型**错误路径**用例（预期 `IncompatibleBinding`——即 C1 前置判别断言），完成判定改"**非绑定依赖管理面走通**"，不写 ovisocr3 verify 成功断言；verify 成功路径移至 O3-T10 完成判定承接复核。
  - 非阻断 N1'（D-4 "T26"→"O3-T26" 体例）、N2'（§4/O3-T12 `manifest.adapter_binding.id`→`manifest.adapter_binding.adapter`，实际字段名核实于 `asset.rs:51`）措辞修正。
- v1.1（2026-11-14）评审响应（`docs/review/ovisocr3-integration-development-plan.v1-review.md`，2 BLOCK + 5 非阻断）：
  - **BLOCK-1**：`model` 子命令族解析层/执行层双重硬编码单模型（`parse_model_command` 拒绝 `--model`、`CliCommand::Model` 无 model 字段、handler `DEFAULT_MODEL` 硬编码 + 字面量 `model=ovisocr2` 输出）——§3.3 第 5 条改为"既有缺省形态冻结 + 新增 `--model` 面实施后冻结"；§4 增补扩展点行（中改）；O3-T09 扩为选择器新增 + golden 创建；C6 拆三断言；G2/§8.3 预期差异清单增补。
  - **BLOCK-2**：`ScanRequest` 类型不存在（实际入口 `run`/`run_document` + `DocumentOptions`）、CLI `ScanOptions.model` 为死字段——O3-T16 改为实际落点（模型 id 挂入 `DocumentOptions`）并显式纳入 CLI 接线（main.rs:178 死字段 → main.rs:938 handler → app 入口）；C7 明确从 CLI 解析层驱动的端到端断言。
  - 非阻断 N1（里程碑补全路径 `docs/test/reports/`）、N2（§11 第 6 条标注 G4）、N3（O3-T25 fixture 落点 `assets/regression/` 分册）、N5（O3-T09 ovisocr3 行格式定稿后再冻结）采纳；N4 无需改动（T01 执行期逐文件复核注记已含）。
- v1（2026-11-14）初稿。
**前置条件：** `model-backend-extensibility` 架构重构已完成合并（实施日志 T001–T107 完成、T103/T108 跳过）。
**立项来源：** 重构计划 T108【跳过】原文——"另行评估并接入新 OCR/VLM……§5.8 缺口评估作为该立项的设计输入重新裁决"。
**设计输入：**
- `docs/develop/model-backend-extensibility-refactoring-development-plan.md` §5.8 多模型接入缺口评估；
- `docs/develop/model-backend-extensibility-refactoring-implementation-log.md`（扩展点清单与遗留边界）。

---

## 1. 背景与结论

模型后端可扩展架构重构已交付：`ocrx-inference-api` 稳定公共契约（TaskRequest/TaskResponse/结构化执行身份/manifest v2/ProtocolAdapter 与 RuntimeFactory 端口）、`ocrx-models` 通用资产管理、`ocrx-inference` registry/绑定校验/推理服务、`ocrx-scan` 扫描应用（只依赖 inference-api）、`ocrx-cli` 组合根、以及 `scripts/validate-architecture-boundaries.py` 依赖铁律。当时明确"不接入第二模型"，T108（接入新模型）作为下一个独立立项跳过。本计划即该立项。

**结论：** OvisOCR3（ATH-MaaS，2026-10-20/21 发布）是 Qwen3.5-0.8B 底座的统一 OCR/文档理解模型，与现有 OvisOCR2 同底座、同家族输出约定（bbox `images/bbox_{l}_{t}_{r}_{b}.jpg` [0,1000)、HTML 表格、LaTeX、沿用 OvisOCR2 后处理顺序）、同 MTMD + GGUF + mmproj 推理路径——属**同构升级候选**，不是新模型家族。接入以**新增**为主（新 builtin manifest、新 adapter crate、新 prompt pack、per-model 绑定映射），**修改**为辅（catalog/registry/service/app 的小型泛化），不触碰推理平台冻结契约（§5.1–§5.5 的请求/响应/身份形态）。

三条铁律贯穿本计划：

1. **OvisOCR2 行为零变化**：默认模型、manifest pin、输出、checkpoint、golden 全部保持；`ovisocr3` 仅经显式 `--model` 选用。
2. **跨模型 checkpoint 不复用**：结构化身份已含 model/runtime/adapter 逐字段比较，新增判别测试钉死"换模型 = 全量重扫"。
3. **默认模型切换是独立产品决策**：本计划只交付"能力可用 + 对照证据"，不改 `DEFAULT_MODEL`；是否把 ovisocr3 提为默认由验收后的决策门（§7 D-GATE）裁决。

## 2. OvisOCR3 调研事实（2026-11-14 核实）

### 2.1 模型卡事实（[ATH-MaaS/OvisOCR3](https://huggingface.co/ATH-MaaS/OvisOCR3)）

| 项 | 事实 |
|---|---|
| 形态 | 统一 0.8B 多模态模型：文档解析（Markdown / 结构化 JSON / Layout JSON）、通用 OCR（10 任务）、KIE（open/fields/schema）、图表描述（含 `<caption>`/`<mermaid>`）、跨页表格重建（两阶段、双图输入） |
| 底座 | Qwen3.5-0.8B（与 OvisOCR2 同底座），四阶段训练（CPT → mid-training → on-policy 蒸馏 → RL） |
| 许可证 | Apache-2.0（SPDX: Apache-2.0） |
| 技术报告 | arXiv 2610.33066 |
| 官方分数 | OmniDocBench1.6 **96.74**（OvisOCR2 96.58）、MDPBench 88.80（OvisOCR2 69.00）、PureDocBench 82.31（OvisOCR2 75.10）、Real5 93.63（OvisOCR2 92.29） |
| 推理设置（示例） | vLLM 0.22.1；temperature 0.0；max_tokens 16384；`enable_thinking=False`；`min_pixels=448×448`、`max_pixels=2880×2880`；单请求最多 2 图（跨页表格用） |
| 坐标约定 | `[left, top, right, bottom]`，整数 [0,1000)，左上原点，按所供图像归一化；Markdown 图区引用 `<img src="images/bbox_{l}_{t}_{r}_{b}.jpg" />`——**与 OvisOCR2 约定相同** |
| Markdown 后处理 | 模型卡明示 document_markdown "follow the **OvisOCR2 postprocessing order**：strip 首尾空白 → 可选过滤 imgtag 块 → 对所有响应做 repeated-tail cleanup"——**与现行 scan parser 语义同源** |
| Prompt 供给 | 仓库 `prompts/*.txt`（document_markdown / document_structured / plain_ocr / kie_* 等），经 chat template（`enable_thinking=False`）应用 |

### 2.2 供应链事实（GGUF）

- 官方仅发布 Transformers/Safetensors；**GGUF 为社区转换**。候选：
  - [michalland/OvisOCR3-GGUF](https://huggingface.co/michalland/OvisOCR3-GGUF)（2026-11-14 核实存在；mmproj 保持 F16）；
  - prithivMLmods/ATH-MaaS-OvisOCR3-GGUF（重构计划 §5.8 于 2026-11-11 记录）。
- 现行管线只消费 GGUF+mmproj（llama.cpp MTMD），§5.8 P0-1（量化格式）对 OvisOCR3 **直接消解**。
- 选型与冻结标准见 O3-T01：量化档位对标 OvisOCR2 四档（Q4_K_M/Q5_K_M/Q8_0/BF16），逐文件 sha256 + 字节数 + revision + 许可证/致谢链全部记录后冻结；社区转换属供应链风险（§9 R1）。

### 2.3 与 OvisOCR2 的同构性对照

| 维度 | OvisOCR2（现行） | OvisOCR3 | 判定 |
|---|---|---|---|
| 底座/量化家族 | Qwen3.5-0.8B | Qwen3.5-0.8B | 同家族，内存阶梯可镜像 |
| 资产角色 | model + mmproj（revision 级共享） | 同 | `SUPPORTED_ASSET_ROLES` 不变 |
| 推理路径 | llama.cpp MTMD + GGUF + mmproj | 同 | 同构 |
| 贪心/无思考 | 冻结 temperature 0、max 16384 | 官方示例一致（temperature 0.0 / 16384 / enable_thinking=False） | 生成参数契约可沿用 |
| 像素预算 | 2880×2880 上限、max_image_tokens 1024 默认 | min 448²、max 2880² | 同量级，T04 复核 |
| bbox/表格/LaTeX | `bbox_{l}_{t}_{r}_{b}` [0,1000)、HTML 表格、LaTeX | 同约定，且明示沿用 OvisOCR2 后处理顺序 | parser 可共享 |
| think 泄漏 | 有（Q3.1 剥离 + `think_removed` 警告） | 待核实（Qwen3.5 底座、模板关思考） | T03 裁定 |
| token 网格 | MTMD 网格绑定，`image_tokens_override`（2048 rung） | 待核实（同底座大概率兼容） | T04 裁定 |
| 多图能力 | 不支持（backend 强制恰 1 图） | 支持 2 图（仅跨页表格需要） | v1 不用；登记为已知限制 |

### 2.4 spike 待核实清单（Phase 0 输入）

1. 冻结 pin（wrapper 0.1.161 / upstream `d8123504…`）能否加载 OvisOCR3 GGUF+mmproj 并正确编码/生成；
2. `document_markdown` 原生 prompt 在 llama.cpp MTMD 路径是否需要 chat template 包装（`enable_thinking=False` 在低层 API 的落点）；
3. `<think>` 泄漏行为与现有 strip 语义（Q3.1 计数）兼容性；
4. `image_tokens_override=2048` 网格 rung 是否被 OvisOCR3 mtmd 上下文接受；
5. bbox/表格/公式/退化尾部输出形态逐项比对（表格/公式/图片区域/中英文样本页）。

## 3. 目标、非目标与冻结基线

### 3.1 目标

- **G1** `--model ovisocr3` 全链路可用：`model install/verify/remove/list/info/recommend`、`doctor`（无参缺省形态 + `--model` 新面，D-11）、`scan`（native/empty/OCR 路由、续扫、取消、多文档编排、退化阶梯）端到端走通。
- **G2** OvisOCR2 行为零变化（§3.3 字节不变集全部成立；预期差异仅限 §8.3 登记四项：`model list` 无参输出新增 ovisocr3 行、model 子命令族新增 `--model` 参数面、doctor 新增 `--model` 参数面（**无参输出不变**）、help 文本）。
- **G3** 跨模型 checkpoint 不复用，有判别测试钉死（双方向）。
- **G4** 质量证据：同 harness 对照（fixture gold 集 + OmniDocBench 复评）产出 OvisOCR2 vs OvisOCR3 对照报告。
- **G5** §5.8 缺口的最小闭环：per-model prompt 身份（P1-4）、per-model 绑定（P2-11）、网格能力裁定（P0-3，最小形式）；家族输出契约命名化替代通用 IR（P0-2，见 §5）。

### 3.2 非目标（v1 范围外，逐项登记）

| 不做 | 理由/登记 |
|---|---|
| 结构化 JSON / Layout JSON 解析模式 | scan 无此管线；Markdown 模式已覆盖现行业务语义 |
| 通用 OCR 10 任务 / KIE / 图表描述 / 翻译 / VQA | 结构化抽取与"页→Markdown"不同类；平台不阻断性仅确认：`TaskRequest.prompt` 可承载任意任务 prompt（推理平台不知业务意图） |
| 跨页表格重建（双图请求） | `LlamaBackend::encoded_to_worker_task` 强制恰 1 图——登记为已知平台限制；若未来立项再扩 backend 协议 |
| 通用规范中间层 IR | 见 §5 P0-2 裁定：以"Ovis 家族 Markdown 输出契约"命名化 + 版本化替代；第三模型（异构输出）立项时再评估 IR |
| 动态模型注册 / 第三方 adapter 插件 | 维持静态注册（§2.2 非目标延续） |
| AWQ / 非 llama.cpp runtime | 维持 not-now（§5.8 P0-1 裁定延续） |
| 默认模型切换 | D-GATE 独立裁决，本计划不改 `DEFAULT_MODEL` |
| 单次 scan 混用多模型 | 一条 scan 命令一个模型（D-8） |

### 3.3 必须冻结的 OvisOCR2 行为（字节不变集）

1. `DEFAULT_MODEL="ovisocr2"`（`src/ocrx-cli/src/main.rs:33`）与 `--model` 缺省行为；
2. OvisOCR2 四 profile manifest pin：revision `8a22290a…`、逐资产 sha256/字节、URL 基址（`builtin/ovisocr2.rs`）；
3. `OCR_MARKDOWN_V1` prompt 字节 + `PROMPT_ID="ocr-markdown"` / `PROMPT_VERSION="ocr-markdown-v1"` / `PROMPT_SCHEMA_VERSION=1`（`src/ocrx-scan/src/ocr/prompt.rs:9-17`）——迁移判别测试逐字节断言；
4. `DECODE_GENERATION=3`、wrapper/sys/upstream pin `0.1.161/d8123504…`（前提：O3-T02 裁定 pin 不动；若 NO-GO 则按 D-4 升代并触发 OvisOCR2 全量回归）；
5. 既有 model 命令**缺省形态**输出逐字节保持：`model list`（现单行 `model=ovisocr2 …`）、`model install/verify/remove` 成功串（现字面量 `model=ovisocr2 profile=… …`——参数化后缺省输出不变）、`doctor`（schema 2 字段序）、`result.json` ovisocr2 形态、页产物/v6 checkpoint/v5 迁移、StepProfile/CommitProfile 黄金串；
   （注：`model list/info/recommend --model ovisocr2` 这类**逐模型显式形态今天不存在**——model 子命令族解析层与执行层双重硬编码单模型，`--model` 现报 `unsupported model flag`（见 §4）。该形态属本计划**新增面**，实施后创建 golden 并冻结，见第 8 条。）
6. scan 依赖面：scan 仍只依赖 {core, primitives, inference-api, output, pdf}（架构脚本既有条目不动，只增不改）；
7. 退化阶梯语义（2048 网格 rung / repeat_penalty 1200 milli×64 / 切栏）对 OvisOCR2 不变；
8. **新增面冻结（实施后冻结，防后续漂移）**：model 子命令族新增 `--model` 选择器后，`--model ovisocr2` 形态的 `model list/info/recommend/install/verify/remove` 输出（O3-T09）、scan 的 `--model ovisocr2` 全链路行为（O3-T16）与 `doctor --model ovisocr2/ovisocr3` 新面（O3-T21/D-11）在创建时即冻结为 golden——与既有缺省形态同等对待。

## 4. 重构交付扩展点与本立项落点映射

| 扩展点（重构交付） | 现状锚点 | OvisOCR3 落点 | 幅度 |
|---|---|---|---|
| 内置模型声明 | `src/ocrx-models/src/builtin/ovisocr2.rs`（唯一 builtin） | 新增 `builtin/ovisocr3.rs`（镜像形态）+ `builtin/mod.rs` 注册 | 纯新增 |
| 模型目录 | `src/ocrx-models/src/catalog.rs:12` `ModelCatalogV2` 静态单模型；`require_registered` 字符串等值 `catalog.rs:91-96` | list/profiles/manifest/require_registered 按模型 id 分派 | 小改 |
| 内存阶梯推荐 | `src/ocrx-models/src/recommend.rs:24-41`（回退**固定 `compact`**——仅 `preferred != "compact" && contains("compact")`；单测固定 `["balanced"]`@8GiB 失败，`recommend.rs:63-76`） | 回退规则泛化为**模型声明序首个实际可用档**（OvisOCR2 完整四档下等价现 compact 回退、行为逐字节不变；6 GiB 资源下限双模型统一）；`["balanced"]` 断言随可变档位集语义**有意翻转**（D-7） | 中改 |
| 缓存布局 | `cache.rs:4-5` 冻结注释以 ovisocr2 书写；实现按 manifest.model 参数化的假设需实施验证 | 双模型路径判别测试；若硬编码则泛化 | 验证/小改 |
| registry 绑定 | `registry.rs:12-24` 单组常量；`binding.rs:19-49` `check_binding` 逐字段比较**单组** (runtime, adapter) | model→(runtime, adapter) 映射；`check_binding` 按 manifest.model 查表后逐字段比较 | 中改（本计划最大单点） |
| adapter 选择 | `src/ocrx-inference/src/service.rs:267-269` `ovis_adapter()` 硬编码单例（L219/L489 session 使用；**L259 `static_identity` 亦读 `registry::adapter_identity()` 单组常量**——预比较 `ocr_pre_compare` 消费该字段） | **三读取点**（session ×2 + static_identity）统一按 `manifest.adapter_binding.adapter` 映射（BLOCK-3） | 中改 |
| backend 协议约束 | `src/ocrx-runtime/src/backend.rs:53-91`（恰 1 图/BeforePrompt/贪心）；L249-255 身份硬编码（与 registry 常量重复） | 约束复核（T03 结论）；身份单源化消重 | 复核/小改 |
| MTMD 文本拼接 | `src/ocrx-runtime/src/llama_mtmd.rs:828-834`/`1193-1200` 无条件 `format!("{}{}", mtmd_default_marker(), request.prompt)`（**串行+窗口两路径**）；`backend.rs:76-84` prompt 原样透传进 `WorkerTask` | 仅 D-2 分支 3 触发条件任务 O3-T13b：媒体标记策略（`BeforePrompt` 冻结语义 / `EmbeddedInPrompt`），两路径同改 + OvisOCR2 逐字节不变判别（C13）；其余分支零改造 | 条件任务 |
| adapter crate | `src/ocrx-ovisocr2`（encode 透传 + decode think 剥离） | 新增 `ocrx-ovisocr3`（D-1） | 纯新增 |
| scan 模型硬编码 | `src/ocrx-scan/src/app.rs:449/461/716` `ModelId::new("ovisocr2")`；app 入口为 `run`/`run_document`（app.rs:212/295）+ `DocumentOptions`（app.rs:74）——**不存在 `ScanRequest` 类型**；CLI 侧 `ScanOptions.model`（main.rs:63 声明/139 初始化/178 赋值）为**死字段**（全文件无消费点） | 模型 id 挂入 `DocumentOptions`，`run`/`run_document` 贯穿（默认 "ovisocr2"）；**CLI 接线**：`ScanOptions.model` → `CliCommand::Scan` handler（main.rs:938 `run_scan_via_application`）→ app 入口 → 预比较/终比较/prepare/兼容身份/step-profile | 中改（含 CLI 接线） |
| scan prompt | `src/ocrx-scan/src/ocr/prompt.rs`（冻结字节全局常量）；读取点：`ocr/artifacts.rs:191`（`SessionRecognizer::request`）、`app.rs:434-445`（预比较 `ScanOcrSide`）、`scan/compatibility.rs:51-75`（checkpoint 身份） | prompt pack per-model 注册表 + **全部读取点贯穿**（D-2/O3-T15——BLOCK-3） | 中改 |
| 退化阶梯 | `src/ocrx-scan/src/state.rs:54-79`；网格集 `ocrx-inference-api/src/task.rs:32` `SUPPORTED_IMAGE_TOKEN_GRIDS=&[2048]` | 按 O3-T04 裁定：有效→零改；无效→per-model 配置（D-5） | 裁定后定 |
| checkpoint 兼容身份 | `src/ocrx-output/src/compatibility.rs:46-64` `OcrIdentity`（inference 全身份 + prompt 三元组） | prompt 三元组取所选 pack；跨模型否决判别测试（G3） | 小改+测试 |
| CLI scan 门禁 | `src/ocrx-cli/src/main.rs:172-178` scan `--model` 仅接受 DEFAULT_MODEL | 放开至 {ovisocr2, ovisocr3}——**并入 O3-T16**（CLI e2e 断言的前提，BLOCK-1）；`DEFAULT_MODEL` 不变 | 一行级 |
| model 子命令选择器 | `src/ocrx-cli/src/main.rs:43-48` `CliCommand::Model` **无 model 字段**；`parse_model_command`（main.rs:681-710）对 `--model` 报 `unsupported model flag`；执行层 handler 一律 `ModelId::new(DEFAULT_MODEL)`（main.rs:797/866/885/923），install/verify/remove 成功串为**字面量** `model=ovisocr2 …`（main.rs:873/900/928） | 解析层 + `CliCommand::Model` + handler 全系新增 `--model` 选择器（缺省 `DEFAULT_MODEL`；错误串与 scan 同款形态 `unsupported model: …`）；字面量输出参数化；`--model ovisocr2` 形态创建 golden 即冻结（O3-T09） | 中改 |
| doctor 输出面 | `src/ocrx-cli/src/main.rs:1936-1987`/`2100-2102` doctor 数据与 JSON **硬编码单模型**（四档 profile 数组、`"model":"ovisocr2"`、installed 检查走 `ovisocr2_manifest`） | **无参缺省形态逐字节不变**（= 缺省模型就绪度语义，D-11/BLOCK-4 方案 1）；新增 `doctor --model` 选择器（同款形态）+ ovisocr3 新面 golden | 中改 |
| result.json 模型对象 | `src/ocrx-cli/src/main.rs:1799-1845` `ScanResultFields`/`serialize_scan_result` **硬编码**（id/revision/adapter/`runtime=llama.cpp@0.1.152` 历史值/profile→quantization 映射）——`DocumentOutcome.identity`（app.rs:141）已有身份但**未被消费** | ovisocr3 路径从 `DocumentOutcome.identity` 接线派生（N1：主要实现工作）；ovisocr2 缺省形态**逐字节不变**（含历史 runtime 值；0.1.152→0.1.161 修正登记为独立契约变更） | 中改 |
| 组合根 | `src/ocrx-cli/src/compose.rs:31-65`（顺序组合，本身模型无关） | 预计零改；adapter 注册在 inference 层完成 | 零/验证 |
| 架构白名单 | `scripts/validate-architecture-boundaries.py:20-69`；未知 crate 即违规（L146） | 增补 `ocrx-ovisocr3`（→ primitives+inference-api）与 `ocrx-inference` 白名单增补 | 条目增补 |
| 评估模块 | `ocrx-eval` fixture 仅 ovisocr2；导出归一化在 `scripts/export-omnidocbench-predictions.py`（think regex） | 双模型 fixture + `--model`；导出归一化适用性注记 | 小改 |

## 5. §5.8 缺口再裁定（针对 OvisOCR3）

| 缺口 | 原裁定（重构计划 §5.8） | 对 OvisOCR3 的再裁定（本计划） |
|---|---|---|
| P0-1 量化格式 | not-now，接入前须裁决 | **消解**：社区 GGUF 已有，管线零改 |
| P0-2 规范中间层 | 接入新模型前须定义 ProtocolAdapter 输出规范几何+规范块类型的 IR，否则 parser 逐模型 fork | **以家族契约命名化替代**：OvisOCR3 模型卡明示 Markdown 模式"沿用 OvisOCR2 后处理顺序"，bbox/表格/LaTeX 约定相同——parser 不 fork。将该共享约定落为**命名契约**：`Ovis Markdown Output Convention v1`（文档化：bbox 前缀与缩放、HTML 表格、LaTeX、imgtag 块语义、think 泄漏处理），由两 adapter 的 decode 漂移检测 golden（O3-T11）钉住；`page_parser_version` 双模型共用 `ovis-page-v1`，若 T03 发现 OvisOCR3 需不同处理则仅对 ovisocr3 升版。通用 IR（异构模型）推迟到下一个非 Ovis 家族立项 |
| P0-3 token 网格 | 声明式网格能力（能力枚举），请求引用能力 | **最小形式**：O3-T04 实测；2048 rung 对 OvisOCR3 有效则契约零改；无效则 rung1 网格 per-model 配置（D-5），`SUPPORTED_IMAGE_TOKEN_GRIDS` 仅做记录性扩展（增量、文档化），不引入通用能力枚举框架 |
| P1-4 prompt 身份全局化 | 接入自有 prompt 模型时改 per-adapter 身份 | **采纳**：prompt pack per-model（D-2），各自 PROMPT_ID/VERSION/SCHEMA_VERSION；checkpoint 兼容身份随动。跨模型复用本就被 `OcrIdentity.inference.model` 字段比较阻断，prompt 三元组随 pack 选用即满足语义 |
| P1-5 采样链跨 runtime 映射 | 新增 runtime 时生效 | 不触发（仍单 llama.cpp runtime） |
| P1-6 规范块类型可扩展 | 新块类型须可注册或声明降级 | 不触发：Markdown 模式无新块类型（`<caption>`/`<mermaid>` 属图表描述模式，v1 范围外） |
| P1-7 评估导出归一化 | 各 adapter 自带导出归一化钩子 | 部分采纳：think 剥离 regex 的适用性按 T03 结论记录到导出脚本注释与评估 runbook；不建钩子框架 |
| P2-8/9/10/11 | 已覆盖/默认实现/数据驱动/静态注册扩 adapter | 8/9 直接成立；10 仅在 D-5 需要时做最小配置化；11 由 O3-T10/T12 落地 |

## 6. 关键设计决策（D-1…D-10）

> 标注【已冻结】的决策不得在实施中无声更改；标注【spike 裁定】的决策在 O3-T05 收口后冻结。

**D-1 新 adapter crate `ocrx-ovisocr3`【已冻结】**
镜像 `ocrx-ovisocr2` 形态（identity + encode + decode），依赖面仅 {primitives, inference-api}。家族共性 decode 逻辑（normalize→think 剥离→双文本/warnings）暂不抽共享 crate：以**同输入双 adapter 输出逐字节一致**的漂移检测 golden 钉住重复实现（O3-T11），第三个 Ovis 家族模型立项时再评估抽取 `ocrx-ovis-common`。不选"在 ocrx-ovisocr2 里塞第二个 adapter"（命名错位）与"跨 adapter crate 依赖"（架构面耦合）。

**D-2 prompt 归属与包装【已冻结——分支 1（原生直喂），2026-11-14 spike 裁定（O3-T02/T03，见 `docs/develop/ovisocr3-integration-spike-2026-11.md`）；分支 3 关闭，O3-T13b 不触发】**
OCR 指令（业务意图）归 scan 的 per-model prompt pack；**媒体标记与模板的最终拼接语义归 runtime**——adapter 不得单侧承担模板拼接：运行时在 tokenize 时无条件执行 `format!("{}{}", mtmd_default_marker(), request.prompt)`（`llama_mtmd.rs:828-834` 串行 / `llama_mtmd.rs:1193-1200` 窗口，`backend.rs:76-84` 原样透传），仅改 adapter 会产生"标记位于整条模板之前"或"双标记"的协议错误输入，且 fake 测试无法发现。O3-T03 必须记录**运行时最终拼接后的完整 MTMD 输入文本**（不得以 adapter encode 局部字符串为裁定依据）。裁定三分支：
1. **原生直喂**（预期分支——同家族）：指令可直接作为 prompt → adapter 透传，零 runtime 改造；
2. **需模板包装且图片应位于全部文本之前**：与运行时无条件前置语义**等价** → 复用现状，spike 记录等价性证明，零改造；
3. **需模板包装且图片必须位于消息内特定位置**（如 chat 模板用户消息内部）→ 触发**条件任务 O3-T13b**（runtime 文本协议改造）：`EncodedTask`/`WorkerTask` 增加媒体标记策略（`BeforePrompt`＝现状冻结语义 / `EmbeddedInPrompt`＝adapter 渲染的完整模板已含标记位置、运行时跳过前置），串行与窗口两条路径同改；**OvisOCR2 走 BeforePrompt 原语义，最终 MTMD 输入逐字节不变（C13 判别）**；OvisOCR3 最终 MTMD 输入 golden（实际 adapter 渲染 + runtime 策略**联合判别**）随 **O3-T11（C13b）**——本任务时刻实际 adapter 尚不存在（v1.5 复审 BLOCK-1 拆分）。**O3-T13b 在 Phase 2 内排序先于 O3-T11**（分支 3 下 adapter encode 依赖策略字段；触发状态在 O3-T05 冻结——v1.4 复审 BLOCK-3）。
若 O3-T03/T05 裁定分支 1 或 2，D-2 明示**关闭分支 3**（"无需 runtime 改造"写入 spike 记录），O3-T13b 标注【不触发】。`ovisocr2` pack 现字节原样迁移（判别测试逐字节断言），encode 行为不变。

**D-3 registry/binding per-model 映射【已冻结】**
`check_binding` 从"比较唯一一组 (runtime, adapter)"改为"按 `manifest.model` 查静态映射 → 逐字段比较"，错误组合（ovisocr3 资产 × ovisocr2 adapter 等）必须得到 `IncompatibleBinding`；`RegistryVerifier` 端口形态不变（prepare 与 `model verify` 共用同一语义——RBLK-9 延续）。`SUPPORTED_ASSET_ROLES` 维持 `["model","mmproj"]`。

**D-4 runtime pin 策略【已冻结——GO（pin 不动），2026-11-14 spike 裁定（O3-T02）】**
首选：OvisOCR3 GGUF+mmproj 在冻结 pin（wrapper 0.1.161 / upstream `d8123504…`）上加载、编码、生成全部正常 → pin 不动，`DECODE_GENERATION=3` 不变，OvisOCR2 零回归成本。NO-GO（架构/投影器不被该 commit 支持）：bump wrapper/sys/upstream 至最小可用版本 → `DECODE_GENERATION` 3→4 → 触发 OvisOCR2 全量回归（fixture gold + OmniDocBench 抽样 + CPU/CUDA 对拍）并在 O3-T26 注记"全部 checkpoint 因代次失效属有意识变更"。顺带清理：`LlamaRuntimeFactory`（`backend.rs:249-255`）与 registry 常量的身份重复单源化。

**D-5 网格能力最小配置化【已冻结——阶梯零改 + 基础预算 per-model（ovisocr3=2048/ovisocr2=1024 不变）+ 混合 SSM 串行窗口地板（ovisocr3 window=1，OvisOCR2 不变），2026-11-14 spike 裁定 + 同日生产全页证据两次修订（O3-T04/O3-T18/O3-T26，修订注记见 spike 记录）】**
`image_tokens_override=2048` 对 OvisOCR3 生效 → 阶梯零改。不生效/被忽略 → rung1 网格值随模型配置（scan 侧 per-model config 承载 `ladder_grid: Option<u32>`；`SUPPORTED_IMAGE_TOKEN_GRIDS` 增量扩展并文档化）。禁止为单模型引入通用能力声明框架。

**D-6 默认模型策略【已冻结】**
`DEFAULT_MODEL` 保持 `"ovisocr2"`；`--model` 校验放开至 {ovisocr2, ovisocr3}，未知模型错误串形态不变（`unsupported model: …`）。**选择器族统一形态**：scan、model 子命令族、doctor 的 `--model` 均缺省 `DEFAULT_MODEL`（doctor 无参 = 缺省模型就绪度，见 D-11）。是否切换默认模型 = D-GATE（§7）产品决策，输入为质量/资源/供应链证据；本计划不实施切换。

**D-7 catalog/资产路径泛化与可变档位集【已冻结，v1.3 修订——BLOCK-5】**
catalog 四方法按模型 id 分派；manifest 数据全部来自 `builtin/`（静态注册，无目录扫描）；缓存布局 `<cache>/ocrx/models/<model>/<revision>/[<profile>/model.gguf|mmproj.gguf]` 参数化验证（O3-T08）。**可变档位集**：OvisOCR3 档位集 = O3-T01 冻结表的实际可用子集（**GO 条件 = 至少一个可用档位（含对应 mmproj F16）；四档为期望目标、非硬前提**）；`ModelManagerImpl::list/info` 的展示元数据源从"固定再取 `compact`"（`lib.rs:142-155`/`lib.rs:183-197`）改为**模型声明序首个可用档**——缺档（含缺 compact）不再使 list/info 以 `UnsupportedProfile` 失败；**推荐/回退规则泛化**：内存阶梯首选不可用时，回退档位从固定 `compact` 改为**模型声明序首个实际可用档**（声明序 = 升序尺寸；OvisOCR2 完整四档下等价于现 compact 回退，行为逐字节不变）；6 GiB 资源下限对双模型统一适用（<6 GiB → 不可用，冻结语义不变）；**全部非空子集均有确定 auto 行为**。现有单测 `blocked_first_choice_falls_back_to_compact_only` 中 `["balanced"]`@8 GiB 的失败断言随可变档位集定义**有意翻转**为成功——属新语义判别，非 OvisOCR2 生产行为变化（生产四档不变）。判别测试覆盖缺 compact / 缺中间档 / 仅 balanced / 仅 quality / 仅 reference / 缺 compact 且首选缺失等非空子集（C15）。不虚报：登记的档位集即真实可用集。

**D-8 单次 scan 单模型【已冻结】**
`--model` 作用于该次调用的全部输入（含多文档编排）；scan-summary/result 的 model 对象维持单值假设。混合模型编排属未来独立立项。

**D-9 评估双模型【已冻结】**
ocrx-eval 支持 `--model`（默认 ovisocr2）；fixture gold 双模型分册；导出归一化（think regex）适用性按 T03 结论注记；OmniDocBench 对照 runbook 固化"同 harness、同参、后端与量化档注明"。官方模型卡分数（96.74）**不可**与本仓 harness 基线（84.92，`docs/test/reports/omnidocbench-0161-milestone-2026-11-10.md`）直接比较——对照只在本仓口径内进行。

**D-10 文档与合规【已冻结】**
README/README.zh-CN 的 Scope 从"One supported OCR model"扩展为双模型（`--model` 选择）；THIRD_PARTY_NOTICES(.zh-CN) 增补 OvisOCR3（Apache-2.0）与所选 GGUF 转换仓库的 provenance/致谢；模型卡/技术报告链接入参考资料。实施日志滚动维护于本文档 §10 勾选状态 + 独立日志文件。

**D-11 doctor 输出面【已冻结，v1.3 新增——BLOCK-4 裁定（评审方案 1）；v1.4 明确任务闭环归属】**
**无参 doctor 输出逐字节不变**：现有 doctor JSON（`main.rs:2100-2102`）硬编码 `"model":"ovisocr2"` 与四档 profile 数组、installed 检查走 `ovisocr2_manifest`（`main.rs:1936-1987`）——该形态保持为**缺省模型就绪度**语义（现有 golden 天然成为 `--model ovisocr2` 缺省形态的 golden）。新增 `doctor --model` 选择器（与 model 子命令族同款形态：缺省 `DEFAULT_MODEL`、错误串 `unsupported model: …`），`doctor --model ovisocr3` 为**新面**（ovisocr3 就绪度：实际档位集推荐 + installed 检查按所选模型；schema 2 结构不动；创建即冻结 golden）。**doctor 的解析/实现/help/合法非法用例/golden 全部在 O3-T21 同任务闭环**（O3-T20 只收口 scan/model——v1.4 复审 BLOCK-2）。聚合双模型概览**不引入**默认输出（避免 doctor 必变字节与 G2 冲突），登记为未来工作。预期差异登记入 §8.3（③）。

## 7. 分阶段实施

> 每阶段一个提交序列；每提交前 `cargo test --workspace --locked` 全绿 + clippy `-D warnings`（门禁清单见 §8.1）。任务编号 O3-Txx，勾选状态在 §10 维护。

### Phase 0：调研与真实冒烟 spike（O3-T01–T05）

只读/临时环境，不动生产代码；产出 spike 记录文档（`docs/develop/ovisocr3-integration-spike-2026-11.md`）。

- **O3-T01** GGUF 供应链选型与冻结：候选仓库逐文件核实（量化档清单、mmproj F16、revision、sha256、字节数、许可证/致谢链、下载重试/校验可靠性——逐文件复核含外部声明仓库的存在性与持久性）；对标四档（Q4_K_M/Q5_K_M/Q8_0/BF16）产出冻结表。**GO 条件 = 至少一个可用档位（含对应 mmproj F16）；四档为期望目标、非硬前提（BLOCK-5/D-7）**——冻结表如实记录实际可用档位子集。完成判定：冻结表（含档位子集与缺档说明）入 spike 文档，O3-T06 直接可引用。
- **O3-T02** 冻结 pin 加载 spike：现 pin 的 mtmd 探针加载 OvisOCR3 GGUF+mmproj（CPU 先行），记录加载/图像编码/生成/上下文行为；失败则定位最小 bump 并评估 OvisOCR2 回归影响面。完成判定：GO（pin 不动）或 NO-GO（bump 方案+成本）明确记录。
- **O3-T03** 协议与输出形态 spike：`document_markdown` prompt 的原生格式（chat template 落点、`enable_thinking=False` 表达方式）；think 泄漏与现有 strip 语义兼容性；bbox 前缀/HTML 表格/LaTeX/imgtag 块逐项比对（含表格/公式/图片区域/中英文样本页）；重复退化尾部形态与 `degenerate_tail_crop` 兼容性。**裁定依据必须是运行时最终拼接后的完整 MTMD 输入文本**（`mtmd_default_marker()` + prompt 的实际串接结果——adapter encode 局部字符串不算数，BLOCK-2/D-2）。完成判定：差异表 + D-2 三分支裁定（含分支 3 的触发/关闭明示）+ `page_parser_version` 是否 per-model 裁定。
- **O3-T04** 网格与预算 spike：`image_tokens_override=2048` 行为（生效/拒绝/忽略）；像素预算与 `min_pixels 448²/max_pixels 2880²` 对齐性；context 20480 充足性。完成判定：D-5 裁定输入明确。
- **O3-T05** spike 结论冻结：T01–T04 结论回写本文档（D-2/D-4/D-5 定稿）；提交 spike 记录。完成判定：§6 决策表全部【已冻结】；**D-2 分支裁定即 O3-T13b 触发状态——Phase 2 开工前已知**（v1.4 复审 BLOCK-3：分支 3 时 T13b 先于 O3-T11 执行）。

### Phase 1：资产与目录（O3-T06–T09）

- **O3-T06** `builtin/ovisocr3.rs`：revision、**实际可用档位集**逐档资产（sha256+字节，来自 T01 冻结表子集——可变档位集，D-7/BLOCK-5）、runtime_binding（按 D-4）、adapter_binding（`ovisocr3`/`0.1.0`）、display、license/provenance；按冻结表构造 manifest + validate；`builtin/mod.rs` 注册。完成判定：manifest 单测（角色路径/哈希/字节）与冻结表一致；档位集 = 冻结表子集（≥1）。
- **O3-T07** catalog 泛化与可变档位集（D-7）：四方法按模型 id 分派；`require_registered` 双模型通过、未知模型仍 `UnsupportedModel`；**推荐/回退规则泛化**——`recommend_profile`（recommend.rs:24-41）回退档从固定 `compact` 改为模型声明序首个实际可用档，`ModelManager::recommend`（lib.rs:212-226）与 catalog auto resolve（catalog.rs:74-88）经同一函数自动生效；6 GiB 下限不变；list/info 展示元数据源改模型声明序首个可用档。完成判定：catalog 单测 + `model list` 管理面测试 + 可变档位集判别（C15 全用例）；OvisOCR2 完整四档推荐行为逐字节不变（含 `["compact","balanced","quality","reference"]` 各内存档断言）。
- **O3-T08** 资产路径字面量清查：cache.rs 冻结布局参数化验证/泛化；install/remove/verify 按模型 id 泛化复核。判别测试：双模型同缓存共存、跨模型 revision 互不干扰、共享 mmproj 角色语义不混淆。完成判定：双模型路径测试落地。
- **O3-T09** model 子命令族 `--model` 选择器新增 + 输出策略（BLOCK-1 修复落点）：① 解析层：`parse_model_command`（main.rs:681-710）与 `CliCommand::Model`（main.rs:43-48）增 model 字段，全系接受 `--model {ovisocr2,ovisocr3}`，缺省 `DEFAULT_MODEL`，未知模型错误串与 scan 同款形态（`unsupported model: …`），help 同步。② 执行层：handler 的 `ModelId::new(DEFAULT_MODEL)` 硬编码（main.rs:797/866/885/923）与字面量 `model=ovisocr2 …` 成功串（main.rs:873/900/928）参数化——**缺省输出逐字节不变**（§3.3 第 5 条）。③ golden：既有无参/缺省形态逐字节保持；`--model ovisocr2` 新形态 golden **创建即冻结**（§3.3 第 8 条）；`model list` 无参新增 ovisocr3 行——行格式（及 info/recommend 的多行形态）定稿后再冻结 golden（N5）。④ install/remove/list/info/recommend 双模型用例（离线/非交互策略沿用）；verify 双模型仅**错误路径**用例——本任务时刻（Phase 1，绑定 per-model 泛化尚未落地）`model verify --model ovisocr3` **预期 `IncompatibleBinding`**（OvisOCR3 manifest 的 `adapter_binding.adapter="ovisocr3"` 与单组冻结常量必然失配，verify.rs:107 = 内容校验 + 绑定校验），该预期失败本身即 C1 前置判别断言；**verify 成功路径随 O3-T10 完成判定复核**（v1.1 评审 BLOCK-3 收窄，方案 A）。完成判定：golden 矩阵逐条断言（新建/保持两类分开断言）；**非绑定依赖管理面走通**（install 无绑定校验、remove/list/info/recommend 不触绑定校验）；verify 错误路径断言成立——本任务**不写** ovisocr3 verify 成功断言，Phase 1 门禁不因 T10 时序破碎、判别测试不被无声弱化。

### Phase 2：registry、adapter 与 service（O3-T10–T14；条件任务 O3-T13b 已裁定【不触发——D-2 分支 1】——v1.4 复审 BLOCK-3 时序要求满足）

- **O3-T10** registry/binding 泛化（D-3）：per-model 映射 + `check_binding` 查表比较。判别测试：错误组合矩阵（ovisocr3 资产×ovisocr2 adapter、错 runtime 身份、错 adapter 版本）→ `IncompatibleBinding`；ovisocr3 正确绑定通过。完成判定：绑定矩阵测试；**承接 O3-T09 收窄（v1.1 评审 BLOCK-3）**——`model verify --model ovisocr3` 成功路径复核（内容+绑定全过；若 Phase 1 已建 verify golden 此时点亮成功断言）。
- **O3-T13b【不触发——D-2 分支 1（原生直喂），2026-11-14 O3-T05 冻结（spike 记录：`docs/develop/ovisocr3-integration-spike-2026-11.md`）——v1.4 复审 BLOCK-3 时序要求即此裁定】** runtime 媒体标记策略改造（BLOCK-2）：`EncodedTask`/`WorkerTask` 增加媒体标记策略（`BeforePrompt`＝现状冻结语义 / `EmbeddedInPrompt`＝完整渲染模板已含标记、运行时跳过前置）；`llama_mtmd.rs:828-834`（串行）与 `llama_mtmd.rs:1193-1200`（窗口）**两条路径同改**；OvisOCR2 缺省 `BeforePrompt` 原语义——最终 MTMD 输入逐字节不变（C13）；fake 双策略单测。**触发状态在 O3-T05 已冻结（Phase 2 开工前已知）——分支 3 时 O3-T11 的 encode 依赖本任务的策略字段，故先于 O3-T11 执行**。完成判定：**C13（仅 OvisOCR2 部分——本任务时刻实际 OvisOCR3 adapter 尚不存在，不得以未来 adapter 的联合断言为完成条件，v1.5 复审 BLOCK-1 拆分）** + 双策略测试 + 架构脚本 0；OvisOCR3 联合 golden 归 O3-T11（C13b）；不触发时标注【不触发——D-2 分支 X】。
- **O3-T11** `ocrx-ovisocr3` crate（D-1）：adapter identity + encode（**按 D-2 分支落点**：分支 1/2 = 透传/等价复用；分支 3 = 输出 `EmbeddedInPrompt` 完整渲染模板并标记策略——依赖 O3-T13b 已完成的协议字段，故 T13b 先行）+ decode（家族语义）；同输入双 adapter 漂移检测 golden；架构脚本白名单增补（`ocrx-ovisocr3` → {primitives, inference-api}；`ocrx-inference` 白名单增补 ovisocr3）。完成判定：crate 测试 + clippy 干净 + 架构脚本 0；**分支 3 触发时执行 C13b——以实际 `ocrx-ovisocr3` adapter + runtime `EmbeddedInPrompt` 策略验证 OvisOCR3 最终 MTMD 输入 golden**（标记位于模板内正确位置、运行时不重复前置——v1.5 复审 BLOCK-1 拆分归属）。
- **O3-T12** service adapter 选择泛化——**三读取点统一**（BLOCK-3 修复落点）：`ovis_adapter()` 单点 → 按 `manifest.adapter_binding.adapter` 映射，覆盖 `service.rs:219`/`489`（session/prepare）与 **`service.rs:259`（`static_identity`）**——后者现读 `registry::adapter_identity()` 单组常量，不泛化则 ovisocr3 预比较必然拒绝（`ocr_pre_compare` 消费 static adapter）；identity 透传 adapter id/version。判别测试：双模型 `session.identity().adapter` 与 `static_identity().adapter` 各自正确且一致。完成判定：fake 双模型 prepare 身份断言 + static_identity per-model 断言（C14 前置）。
- **O3-T13** backend 复核与身份单源化（D-4 顺带）：协议约束（恰 1 图/BeforePrompt/贪心）按 T03 结论复核（D-2 分支裁定已在 O3-T05 冻结、O3-T13b 已按触发状态执行/关闭——本任务不再承担分支判定）；`backend.rs:249-255` 身份与 registry 常量单源化。完成判定：单源测试（registry=backend 同源）。
- **O3-T14** fake 双模型 service/prepare 矩阵（**不含 scan 全链路**——scan 模型贯穿在 O3-T16，全链路矩阵移 O3-T19 承接；BLOCK-1 阶段序修正）：FakeRuntimeBackend + 双 manifest 下 prepare/static_identity/session 身份全链路与 model 管理面（经 manager，非 CLI——CLI 面在 T09/T20）。完成判定：双模型 prepare 身份断言、static_identity per-model 断言、管理面测试；workspace 测试全绿。

### Phase 3：scan 接入（O3-T15–T19）

- **O3-T15** prompt pack 模块化与**全读取点贯穿**（D-2/BLOCK-3 修复落点）：① pack 模块 `scan/ocr/prompt/{ovisocr2,ovisocr3}.rs` + 按 ModelId 注册表；ovisocr2 字节原样迁移判别测试。② **贯穿全部读取点**（现均读全局常量）：`SessionRecognizer::request` 的基础与窗口/切栏重试请求构造（`ocr/artifacts.rs:191`）、预比较 `ScanOcrSide`（`app.rs:434-445`）、checkpoint 兼容身份构造（`scan/compatibility.rs:51-75`）。完成判定：迁移判别测试逐字节通过；**静态检查无残留全局 prompt 常量读取点**（三常量的直接引用仅存在于 pack 模块内部）。
- **O3-T16** 模型 id 从 CLI 到执行层的贯穿 + scan 解析放行（独立评审 BLOCK-1 阶段序修复落点；承接 v1.1 评审 BLOCK-2 接线）：① **CLI 接线**：`ScanOptions.model`（main.rs:63/139/178——现为**死字段**，仅赋值无消费）经 `CliCommand::Scan` handler（main.rs:938 `run_scan_via_application`）传入 app 入口；该断链不接通则 `scan --model ovisocr3` **静默跑 ovisocr2**（result.json 身份诚实标注 ovisocr2，用户不可察觉）。② **解析放行前移至本任务**（BLOCK-1）：scan `--model` 校验（main.rs:172-178）放开至 {ovisocr2, ovisocr3}——CLI e2e 断言的前提；help 文本与 golden 收口仍在 O3-T20。③ **应用层**：模型 id 挂入 `DocumentOptions`（app.rs:74——**不存在 `ScanRequest` 类型**，实际入口为 `run`/`run_document`，app.rs:212/295），`app.rs:449/461/716` 三处硬编码改读请求模型（默认 `"ovisocr2"`）。④ **贯穿面**：预比较/终比较/prepare/兼容身份/step-profile 全链路使用请求模型。完成判定：**CLI 解析层 → app 请求模型端到端断言**（从 `parse_scan_args`/CLI 子进程驱动至 fake service，禁止仅 app 内部测试替代）；**实际请求 prompt 捕获断言**（fake 捕获：ovisocr3 每个 `TaskRequest.prompt` 逐字节等于其冻结 pack、ovisocr2 缺省/显式选择逐字节等于旧 prompt——BLOCK-3）+ checkpoint prompt 三元组与所选 pack 一致；缺省行为不变。
- **O3-T17** 跨模型 checkpoint 否决（G3）：判别测试——ovisocr2 checkpoint × ovisocr3 请求（同输入同参）→ 预比较失败全量重扫；反向亦然；混合 profile 同理既有语义复核。完成判定：双方向测试钉死。
- **O3-T18** 阶梯与 think 适用性（D-5）：rung1 网格按裁定；think 计数/`think_removed` 警告通道对 OvisOCR3 生效性测试；rung2 采样链与 rung3 切栏复用确认。完成判定：阶梯行为按裁定冻结并有测试。
- **O3-T19** scan 双模型全链路 fake 矩阵 + 收口（**承接 O3-T14 拆分**——BLOCK-1 阶段序修正）：fake 端到端（prepare→窗口→checkpoint→resume→多文档共享 session）双模型全链路；跨模型/同模型续判别（C14）；依赖面不变（架构脚本既有 scan 条目零改动）。完成判定：`validate-architecture-boundaries.py` 返回 0；双模型全链路 fake 矩阵全绿。

### Phase 4：CLI、文档与打包（O3-T20–T24）

- **O3-T20** CLI 收口——**scan/model 两面**（help/错误串/golden；doctor 面整体归 O3-T21 同任务闭环，避免跨阶段借实现——v1.4 复审 BLOCK-2；BLOCK-1 重排后仅剩收尾；D-6）：CLI help 文本（scan/model 子命令族的 `--model` 取值说明）；错误串形态统一复核（`unsupported model: …` 两面同款）；usage/非法模型 golden 收口；`DEFAULT_MODEL` 不变复核。完成判定：CLI 子进程测试（help 文本、scan/model 双模型合法/非法 usage 错误）；§8.3 ①②④ 复核（doctor 项随 O3-T21）。
- **O3-T21** doctor 双模型面 + 报告接线（**N1 主要实现工作 + BLOCK-4 裁定 + v1.4 复审 BLOCK-2——doctor 解析/实现/help/用例/golden 同任务闭环**）：① **doctor**：**无参输出逐字节不变**（缺省模型就绪度语义，D-11）；新增 `doctor --model` 选择器（解析扩展 main.rs:649-655——现仅无参/`--report json`）+ handler 实现（实际档位集推荐 + installed 检查按所选模型；schema 2 不动）+ help/合法/非法子进程用例；`doctor --model ovisocr2/ovisocr3` 新面 golden **创建即冻结**；聚合双模型概览登记为未来工作。② result.json model 对象**当前并非 InferenceIdentity 通用派生**——`ScanResultFields`/`serialize_scan_result`（main.rs:1799-1845）硬编码 `id/revision/adapter/runtime("llama.cpp@0.1.152" 历史值)/profile→quantization` 映射；主要实现工作 = ovisocr3（及新面）路径从 `DocumentOutcome.identity`（app.rs:141）接线至报告字段并重写序列化；**ovisocr2 缺省形态保持现有串逐字节不变（含历史 runtime 值——0.1.152→0.1.161 修正属独立契约变更，本计划不做，登记）**。③ scan-summary 复核。完成判定：golden 逐字节（ovisocr2 既有保持 + ovisocr3/doctor 新面新建）；doctor 子进程用例（合法/非法）；§8.3 ③ 复核。
- **O3-T22** 文档与合规（D-10）：README 双语 Scope/用法、THIRD_PARTY_NOTICES 双语增补、模型选择章节。完成判定：文档评审通过。
- **O3-T23** 打包与 CI：package 脚本无模型假设复核；发布说明模板（双模型安装指引）；CI 冒烟注记（真实模型 E2E 保持环境门控，不强求 runner 资产）。完成判定：release-scripts/architecture 门禁通过。
- **O3-T24** 实施日志滚动 + 本计划勾选同步。完成判定：日志可断点续接。

### Phase 5：质量评估与验收（O3-T25–T29）

- **O3-T25** fixture gold 集：OvisOCR2 既有 gold（`assets/regression/ovisocr2-draft-v1/` 等）全部保持；OvisOCR3 同页 gold 在 `assets/regression/` 下新增分册目录（与 `ovisocr2-draft-v1/` 同级、同 manifest 形态）。完成判定：双模型 gold 测试。
- **O3-T26** OmniDocBench 对照复评：同 harness 先复跑 OvisOCR2 确认基线（对齐 `docs/test/reports/omnidocbench-0161-milestone-2026-11-10.md` 的 84.92 链），再跑 OvisOCR3 全量；后端与量化档注明（vulkan/CUDA 噪声 ±3% 注记沿用）。完成判定：对照报告（分数表 + 逐页差异抽样 + 退化/截断统计）。
- **O3-T27** 资源与吞吐观察：CPU/GPU 墙钟、峰值 RSS、batch_depth 分层、GPU→CPU 回退行为对照（单机，不外推；对齐 T107 形态）。完成判定：对照表入报告。
- **O3-T28** 真实 E2E 门控套件：`OCRX_E2E_MODEL` 双模型故障注入/取消/续扫/多文档（对齐 T106 语义与判别）。完成判定：`--ignored` 套件双模型通过。
- **O3-T29** 验收报告：§11 完成定义逐项核对；汇总 D-GATE 输入（质量/资源/供应链可靠性）。完成判定：报告落 `docs/develop/`。

### D-GATE：默认模型决策门（不属于本计划实施范围）

输入 = O3-T26/T27/T29 + GGUF 供应链可靠性（修订频率、哈希稳定性、下载成功率）。输出 = 保持 ovisocr2 默认 / 切换 ovisocr3 默认（另行变更：`DEFAULT_MODEL`、安装器默认档、README、checkpoint 宣传口径）。质量不及 OvisOCR2 同 harness 基线时**不切换**，接入本身仍算交付（能力可用 + 差距分析入档）。

## 8. 测试与验收矩阵

### 8.1 本机必跑门禁（每提交）

`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo test --workspace --locked`；`validate-architecture-boundaries.py`；`test-release-scripts.sh`；`bootstrap-check.sh`（既有 4 项 Python fixture 漂移失败沿用 T103 注记，不计入本计划回归）。

### 8.2 判别测试清单（新增，防"表面支持"）

| # | 判别点 | 任务 |
|---|---|---|
| C1 | 错误绑定组合 → `IncompatibleBinding`（资产×adapter 错配、runtime 身份错配、adapter 版本错配）；前置断言 = O3-T09 ④ 的 verify 错误路径用例（v1.1 评审 BLOCK-3 收窄） | O3-T10 |
| C2 | 跨模型 checkpoint 否决（双方向）+ 混合 profile 否决复核 | O3-T17 |
| C3 | ovisocr2 prompt 字节迁移逐相等；ovisocr3 pack 字节冻结 | O3-T15 |
| C4 | 双 adapter decode 漂移检测（同输入逐字节一致） | O3-T11 |
| C5 | 双模型缓存路径共存与互不干扰 | O3-T08 |
| C6 | model 管理面输出三断言：(a) 无参 `model list` 新增 ovisocr3 行；(b) 既有无参/缺省输出逐字节不变；(c) 新增 `--model ovisocr2` 形态创建即冻结 | O3-T09 |
| C7 | scan 缺省模型行为不变 + `--model ovisocr3` 全链路（fake）——**测试必须从 CLI 解析层/子进程驱动**，仅 app 内部测试不算数（钉死 `ScanOptions.model` 死字段断链）；终点断言 = prepare 身份 + **每个实际 `TaskRequest.prompt` 逐字节捕获**（ovisocr3 == 其 pack 冻结字节；ovisocr2 缺省/显式 == 旧 prompt 字节，杜绝"身份正确但请求仍用 OvisOCR2 prompt"）+ checkpoint prompt 三元组与所选 pack 一致（BLOCK-3） | O3-T16/T19 |
| C8 | 阶梯 rung1 网格按裁定的行为（含 `SUPPORTED_IMAGE_TOKEN_GRIDS` 校验路径） | O3-T18 |
| C9 | adapter identity 随 manifest.binding 透传至 session/result/checkpoint | O3-T12/T21 |
| C10 | 架构白名单：新 crate 依赖面越界即违规（负向测试） | O3-T11/T19 |
| C11 | 真实模型：取消 130/续扫复用/多文档一次加载/故障注入，双模型 | O3-T28 |
| C12 | GPU→CPU 回退与 batch_depth 降档（16→8）在 ovisocr3 上复现 T107 语义 | O3-T27 |
| C13 | OvisOCR2 最终 MTMD 输入逐字节不变（`mtmd_default_marker()` + prompt，串行/窗口两路径）——runtime 双策略验证，**不含未来 adapter 的联合断言** | O3-T13b |
| C13b | **OvisOCR3 最终 MTMD 输入 golden**——实际 `ocrx-ovisocr3` adapter 渲染 + runtime `EmbeddedInPrompt` 策略**联合判别**（标记位于模板内正确位置、运行时不重复前置）——仅 D-2 分支 3 触发时执行（C13 拆分，v1.5 复审 BLOCK-1） | O3-T11 |
| C14 | `static_identity` 按 model 返回 adapter/runtime（ovisocr3 static adapter = `ovisocr3`）；同模型零加载续扫复用（ovisocr3 checkpoint 预比较通过、零 prepare 零加载）；有待解码页时预比较 + 终比较判别（RBLK-6/8 语义双模型成立） | O3-T12/T19 |
| C15 | 可变档位集：**仅 balanced / 仅 quality / 仅 reference / 缺 compact 且首选缺失 / 缺中间档 / 仅单档**等非空子集，list/info/recommend/resolve 均不失败且 auto 行为确定（display 与推荐回退均取模型声明序首个可用档——旧算法在这些子集上会 `UnsupportedProfile` 失败，`["balanced"]`@8GiB 断言有意翻转）；OvisOCR2 完整四档行为逐字节不变 | O3-T07/T09 |

### 8.3 行为对比验收（字节不变集）

按 §3.3 清单逐项复验：doctor schema2（**无参缺省形态**）、model 命令既有缺省形态输出、result.json ovisocr2 形态、页产物 md/json/PNG、v6 checkpoint 兼容串、v5 迁移、StepProfile/CommitProfile 黄金串、`OCR_MARKDOWN_V1` 字节。**预期差异（完整清单，仅此四项）**：① `model list`（无参）新增 ovisocr3 行；② model 子命令族新增 `--model` 参数面（解析、错误串、help——BLOCK-1 修复引入的新面，其 `--model ovisocr2` 形态创建即冻结，见 §3.3 第 8 条）；③ doctor 新增 `--model` 参数面（**无参输出逐字节不变**——D-11/BLOCK-4 方案 1，ovisocr3 新面创建即冻结）；④ CLI help 文本新增 `--model` 取值说明。

### 8.4 质量验收

1. fixture gold 双模型全绿；
2. OmniDocBench（本仓 harness）：OvisOCR3 全量分 + 逐页对照 OvisOCR2 基线；差距/提升逐类（正文/表格/公式/阅读顺序/bbox 产物）归因；
3. 资源对照表（T107 形态）；
4. 官方模型卡分数只作背景引用，不作为本仓验收数值。

## 9. 风险登记

| # | 风险 | 影响 | 处理 |
|---|---|---|---|
| R1 | GGUF 为社区转换：质量/哈希/可用性无官方背书；仓库可能改版或消失 | 资产供应链不稳 | O3-T01 选型标准（哈希冻结+revision pin+备选仓库）；`model install` 校验链已有（sha256+receipt）；差异登记 |
| R2 | 冻结 pin 不支持 OvisOCR3 架构/投影器 | 触发 bump → OvisOCR2 全量回归 + 代次失效 | D-4 双轨预案；Phase 0 先行裁定，成本前置 |
| R3 | 输出约定漂移（bbox 前缀/表格/think/退化尾部与预期不符） | parser 误解释、产物损坏 | O3-T03 逐项采样比对 + raw sidecar 保底 + 判别测试；必要时 ovisocr3 专属 `page_parser_version` |
| R4 | chat template 包装缺口（`enable_thinking=False` 未生效） | think 泄漏/输出格式劣化 | D-2 归属明确 + T03 实测；泄漏形态与 strip 语义比对 |
| R5 | 质量不及 OvisOCR2 同 harness 基线 | D-GATE 不切换 | 接入仍交付（G1）；差距分析入验收报告 |
| R6 | 双 adapter decode 重复实现漂移 | 家族逻辑修一处漏一处 | C4 漂移检测 golden；抽取触发条件已记录（D-1） |
| R7 | catalog/registry 泛化破坏 OvisOCR2 golden | 违反铁律 1 | §3.3 字节不变集 + C6/C9 判别；每提交跑门禁 |
| R8 | D-2 分支 3 触发 → runtime 文本协议改造（O3-T13b）波及 OvisOCR2 冻结输入 | 违反铁律 1 | `BeforePrompt` 原语义保留为策略一态；C13 最终 MTMD 输入逐字节判别；串行/窗口两路径同改并测试（BLOCK-2） |

## 10. 详细任务清单

### 10.1 状态维护规则

- 勾选状态在本节维护（`- [ ]`/`- [x]`）；完成数以 `grep -c "^- \[x\]"` 统计；
- 跳过任务保持 `- [ ]` 并加【跳过——原因】删除线注记，不计入完成数；
- 断点续接锚点与关键备忘写入 `docs/develop/ovisocr3-integration-implementation-log.md`（Phase 0 启动时创建）；
- 每提交全绿门禁后才允许勾选。

### 10.2 任务图

**Phase 0（调研与冒烟）**
- [x] O3-T01 GGUF 供应链选型与冻结表（sha256/revision/档位/许可证/备选；**GO 条件 = ≥1 可用档位含 mmproj F16，四档为期望非硬前提**）
- [x] O3-T02 冻结 pin 加载 spike（GO/NO-GO + bump 影响面）
- [x] O3-T03 协议与输出形态 spike（chat template 落点/think/bbox/表格/退化尾部比对 + D-2 三分支裁定——**以运行时最终拼接 MTMD 输入文本为裁定依据**）
- [x] O3-T04 网格与像素预算 spike（D-5 裁定输入）
- [x] O3-T05 spike 结论冻结回写（D-2/D-4/D-5 全部【已冻结】；**D-2 分支裁定即 O3-T13b 触发状态——Phase 2 开工前已知**）

**Phase 1（资产与目录）**
- [x] O3-T06 `builtin/ovisocr3.rs` 按冻结表**实际可用档位集** manifest + 注册（可变档位集，非无条件四档）
- [x] O3-T07 catalog 按模型 id 分派泛化 + 可变档位集（list/info display 源改声明序首个可用档；C15 判别）
- [x] O3-T08 缓存/安装/校验/移除路径参数化验证与双模型判别测试
- [x] O3-T09 model 子命令族 `--model` 选择器新增 + handler 参数化 + golden（新建/保持两类；ovisocr3 行格式定稿后冻结）；verify 双模型仅错误路径（成功路径随 O3-T10 复核——BLOCK-3 收窄）

**Phase 2（registry、adapter 与 service；条件任务 O3-T13b 排序先于 O3-T11——v1.4 复审 BLOCK-3）**
- [x] O3-T10 registry/binding per-model 映射 + 错配矩阵测试
- [ ] O3-T13b【不触发——D-2 分支 1（原生直喂），2026-11-14 spike 裁定；~~runtime 媒体标记策略（`BeforePrompt`/`EmbeddedInPrompt`，串行+窗口两路径；C13 = OvisOCR2 逐字节不变——OvisOCR3 联合 golden 随 O3-T11/C13b）~~】
- [x] O3-T11 `ocrx-ovisocr3` adapter crate（encode 按 D-2 分支落点，分支 3 依赖 T13b 策略字段）+ 漂移检测 golden + 架构白名单增补；分支 3 时执行 C13b（实际 adapter + runtime 联合 MTMD golden）
- [x] O3-T12 service adapter **三读取点统一**（session ×2 + `static_identity`）按 `manifest.adapter_binding.adapter` 映射 + per-model 身份断言
- [x] O3-T13 backend 协议约束复核 + 身份单源化（backend.rs 与 registry 常量消重；分支判定已归 O3-T05）
- [x] O3-T14 fake 双模型 **service/prepare 矩阵**（不含 scan 全链路——移 O3-T19；BLOCK-1）

**Phase 3（scan 接入）**
- [x] O3-T15 prompt pack 模块化 + **全读取点贯穿**（请求构造/预比较/checkpoint；静态检查无残留全局常量读取点——BLOCK-3）
- [x] O3-T16 模型 id CLI→app 贯穿 + **scan `--model` 解析放行前移**（`ScanOptions.model` 死字段接线 + `DocumentOptions` 承载 + CLI 层端到端断言 + **实际请求 prompt 捕获断言**；缺省 ovisocr2 行为不变）
- [x] O3-T17 跨模型 checkpoint 否决判别测试（双方向）
- [x] O3-T18 阶梯 rung 网格按裁定 + think/警告通道适用性测试（D-5 修订：基础预算 per-model，2048@ovisocr3 判别测试钉死）
- [x] O3-T19 scan **双模型全链路 fake 矩阵**（承接 O3-T14 拆分——BLOCK-1）+ C14 续判别 + 架构脚本 0

**Phase 4（CLI、文档与打包）**
- [x] O3-T20 CLI 收口——scan/model 两面（help/错误串/golden；解析放行已在 O3-T16；§8.3 ①②④ 复核——doctor 面归 O3-T21）
- [x] O3-T21 doctor 双模型面（解析/实现/help/用例/golden 同任务闭环——D-11）+ 报告接线（result.json 从 `DocumentOutcome.identity` **接线**为主要实现工作——N1）+ §8.3 ③ 复核
- [x] O3-T22 README 双语 + THIRD_PARTY_NOTICES 双语（Apache-2.0 + GGUF provenance）
- [x] O3-T23 打包/CI 复核与发布说明模板
- [x] O3-T24 实施日志滚动 + 勾选同步

**Phase 5（质量评估与验收）**
- [x] O3-T25 fixture gold 双模型分册（`assets/regression/ovisocr3-draft-v1/`，draft bootstrap 状态，validate 通过）
- [ ] O3-T26 OmniDocBench 对照复评【部分交付——runbook 固化（`docs/test/reports/ovisocr3-omnidocbench-comparison-runbook.md`）+ 30 页同参对照样本已交付（`spike-assets/ovisocr3-comparison-sample.json`）；~~OvisOCR2 基线复跑 + OvisOCR3 全量 + 逐页归因~~全量分数对照超出本窗口算力，待按 runbook 执行】
- [x] O3-T27 CPU/GPU 资源与吞吐对照（CPU 单机口径完成：墙钟/路由/地板/加载入验收报告；GPU 对照留待 GPU 环境——T107 形态注记）+ GPU→CPU 回退复现（回退语义由既有 env 门控测试覆盖，模型无关）
- [x] O3-T28 真实 E2E 门控套件双模型（故障注入双模型 4 用例 + C7/C14 e2e 续扫/多文档；取消 130 走模型无关的冻结取消机制并有 ovisocr2 真实冒烟）
- [x] O3-T29 验收报告 + D-GATE 输入汇总（`docs/develop/ovisocr3-integration-acceptance-report.md`——G1-G5/门禁/合规/边界/可追溯逐项核对 + 质量差距根因分析 + **D-GATE 建议：不切换默认模型**）

## 11. 完成定义

1. **G1**：`--model ovisocr3` 走通 install→verify→scan（native/empty/OCR、续扫、取消 130、多文档一次加载、退化阶梯、GPU 回退）+ `doctor --model ovisocr3` 新面 → 产物/checkpoint/result/报告全链路，含真实模型证据（O3-T28）；result.json ovisocr3 路径已从 `DocumentOutcome.identity` 接线（O3-T21/N1）；
2. **G2**：§3.3 字节不变集逐项复验通过；预期差异仅为 §8.3 登记四项（`model list` 新增行、model 子命令 `--model` 参数面、doctor `--model` 参数面、help 文本）；
3. **G3**：跨模型复用否决判别测试（双方向）钉死；
4. **G5**：per-model prompt 身份（含**实际请求 prompt 捕获断言**与 checkpoint 三元组一致，C7）、per-model 绑定（**static/session/checkpoint 三读取点一致**，C9/C14）、网格裁定全部落地并有测试；
5. **门禁**：§8.1 全绿；架构白名单增补后负向测试成立；
6. **G4（质量证据）**：O3-T26/T27/T29 报告齐备，OmniDocBench 对照在本仓口径完成，官方分数仅背景引用；
7. **合规**：许可证/provenance/致谢文档更新；
8. **边界**：§3.2 非目标清单未越界（无结构化模式/KIE/双图/动态注册/默认切换的悄悄实现）；**D-2 分支 3 未获裁定触发时 O3-T13b 保持【不触发】注记，不得无声实现**；
9. **可追溯**：spike 记录（含 D-2 三分支裁定与最终 MTMD 输入文本证据）、实施日志、验收报告落 `docs/develop/`；决策表全部【已冻结】。

## 12. 参考资料

- [OvisOCR3 模型卡（ATH-MaaS/OvisOCR3）](https://huggingface.co/ATH-MaaS/OvisOCR3)——能力矩阵、坐标约定、Markdown 后处理顺序（沿用 OvisOCR2）、推理设置。
- [OvisOCR3 Technical Report（arXiv 2610.33066）](https://arxiv.org/html/2610.33066v1)。
- [michalland/OvisOCR3-GGUF](https://huggingface.co/michalland/OvisOCR3-GGUF)、prithivMLmods/ATH-MaaS-OvisOCR3-GGUF——社区 GGUF 转换（mmproj F16），O3-T01 选型对象。
- 本仓：`docs/develop/model-backend-extensibility-refactoring-development-plan.md`（§5.8 缺口评估）、`docs/develop/model-backend-extensibility-refactoring-implementation-log.md`（扩展点与遗留边界）、`docs/test/reports/omnidocbench-0161-milestone-2026-11-10.md`（同 harness 质量基线 84.92）。
- 范围约束：`docs/plan/v0.1-tdd-development-plan.md`、`docs/develop/technical-design-v0.1.md`（v0.1 单模型契约的演进注记随之更新，属 O3-T22）。
