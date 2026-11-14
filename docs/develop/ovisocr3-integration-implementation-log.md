# OvisOCR3 接入实施日志（断点续接锚点）

> 对应计划：`docs/develop/ovisocr3-integration-development-plan.md`（v1.5 终稿）
> 工作树：`.worktree/ovisocr3-integration`（分支 `feat/ovisocr3-integration`，基于 main@16577de）
> 推进方式：TDD（先判别/golden 测试，后实现；每提交全绿门禁）
> 状态标记：✅ 完成 / 🔄 进行中 / ⬜ 未开始 / ⏭ 跳过

---

## 环境事实（断点续接必读）

- 本机：Windows x64，CPU-only 构建（无 vulkan/cuda feature；GPU 对照属 Phase 5 注记级）。
- ovisocr2 四档 + mmproj 已装于默认缓存（`%LOCALAPPDATA%/ocrx/models/ovisocr2/8a22290a…/`）。
- huggingface.co 本机不可直连（DNS 污染）；**下载一律走 `https://hf-mirror.com`**（API 与 resolve 均可用，偶发 502 需重试）。
- vendored pdfium 在主 checkout `D:/Develop/OCR-CLI/vendor/pdfium/bin`（worktree 通过相对路径回退，spike 测试已内置该回退）。
- 基线：`cargo test --workspace --locked` 全绿（2026-11-14，@16577de + spike dev-deps）。

## Phase 0（调研与冒烟 spike）

### O3-T01 ✅ GGUF 供应链选型与冻结表

候选仓库（2026-11-14 经 hf-mirror API 核实）：

| 仓库 | revision | 文件（sha256 / 字节） | 判定 |
|---|---|---|---|
| **michalland/OvisOCR3-GGUF**（选定） | `7be644a312005c3a55e2c826aee985a5c93d06b2`（lastModified 2026-10-22） | `OvisOCR3-Q8_0.gguf` = `11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6` / 811,843,712 B；`mmproj-OvisOCR3-F16.gguf` = `698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd` / 204,987,136 B；LICENSE（Apache-2.0 全文）在仓库内 | **GO**：≥1 可用档（Q8_0）+ 对应 mmproj **F16**；单一转换源，模型/mmproj 自洽 |
| prithivMLmods/ATH-MaaS-OvisOCR3-GGUF（备选） | `c29551652e828085ca1e892092f1319dbee22d2f`（2026-10-29） | BF16/Q3_K_L/Q3_K_M/Q4_K_M/Q4_K_S/Q5_K_M/Q5_K_S/Q6_K/Q8_0 全阶梯 + `mmproj-bf16`（sha256 见 spike 记录 JSON） | 不满足字面 GO（mmproj 为 **BF16** 非 F16）；且与 michalland 混装属跨转换源风险。仅登记为灾备（michalland 失效时的替代），不进 manifest |

**冻结表（manifest 将采用，O3-T06 输入）**：

- 档位集 = **仅 `quality` 一档**（Q8_0 → quality，对齐 OvisOCR2 的 Q8_0→quality 映射）；GO 条件"≥1 档含 F16 mmproj"满足；"四档期望"如实记录为不可得（无单一转换源的 Q4_K_M/Q5_K_M/BF16 + F16 mmproj 组合）。
- revision pin = `7be644a312005c3a55e2c826aee985a5c93d06b2`；URL 基址 `https://huggingface.co/michalland/OvisOCR3-GGUF/resolve/<revision>`（本机测试下载走 hf-mirror 等价路径）。
- 单档位集正是 C15"仅单档"判别场景，D-7 可变档位集语义有真实判别载体。

**预验证事实**：OvisOCR2 GGUF 的 `general.architecture` 即 `qwen35`（本机 GGUF 头解析核实）→ 冻结 pin 0.1.161 已支持 qwen35 架构，T02 GO 预期强。

资产落位：两文件已按 manifest 布局下载到 `%LOCALAPPDATA%/ocrx/models/ovisocr3/7be644a3…/{quality/model.gguf, mmproj.gguf}`，sha256/字节均与冻结表一致（下载后本地复验通过）。

### O3-T02/T03/T04 🔄 真实模型 spike（探针已写，运行中）

- 探针：`src/ocrx-runtime/tests/ovisocr3_spike.rs`（env 门控 `OCRX_E2E_MODEL=1`；生产 actor 路径 `LlamaMtmdRuntime::load` → `generate_with_progress`）。
- 输出：`docs/develop/spike-assets/ovisocr3-spike-record.json` + 本文档结论回写。
- 已知输入：官方 `prompts/document_markdown.txt` 与 OvisOCR2 `OCR_MARKDOWN_V1` **语义逐字相同**，仅格式差（官方无前导 `\n`、"Preserve" 前为空格；现行冻结字节为 `\n` 前导 + `\n` 中缝）。原文已存 `docs/develop/spike-assets/ovisocr3-document_markdown.txt`。
- T03 关键裁定输入（D-2）：运行时最终 MTMD 输入 = `mtmd_default_marker()` + prompt（两 prompt 变体的输出形态都将记录）。

### O3-T05 ⬜ spike 结论冻结回写（待 T02–T04 完成）

## Phase 1 ✅（O3-T06–T09）

- O3-T06：`builtin/ovisocr3.rs`（冻结表逐字段 manifest + validate；档位集 = 实际可用子集 `["quality"]`）。判别：manifest 单测钉死 revision/sha256/字节/runtime pin/adapter 绑定/provenance。
- O3-T07：catalog 四方法按 model id 分派；`recommend_profile` 回退泛化为声明序首档（D-7/BLOCK-1）——OvisOCR2 完整四档逐字节不变，`["balanced"]`@8GiB 断言按新语义有意翻转；list/info 展示源改声明序首档；C15 判别（单档/缺 compact/缺中间档/资源下限）全绿。
- O3-T08：缓存布局参数化验证 + 双模型判别（同缓存共存/跨 revision 不混淆/mmproj 角色各自 revision 层）+ ovisocr3 tiny-manifest install→verify→remove 全链路（机制 model-agnostic）。注：该判别测试随 O3-T09 提交入树。
- O3-T09：model 子命令族 `--model` 选择器（解析/执行/golden）；`model list` 无参 = 声明序双行（首行字节不变，C6）；`--model ovisocr2` 形态创建即冻结；verify 双模型仅错误路径（C1 前置判别：RegistryVerifier 拒绝 ovisocr3 绑定 "adapter id"）。

## Phase 2 ✅（O3-T10 / T13b【不触发】/ T11 / T12 / T13 / T14）

- O3-T10：binding per-model 映射（D-3）：`adapter_identity_for(model)`；RegistryVerifier 与 prepare 同走该映射；C1 错配矩阵（资产×adapter/runtime/版本/未注册模型）全 IncompatibleBinding；**O3-T09 交接兑现：`model verify --model ovisocr3 quality` 成功路径点亮**（env 门控 golden，真实资产 content+binding 全过）。
- O3-T13b：**【不触发——D-2 分支 1（原生直喂）】** runtime 零改造。
- O3-T11：`ocrx-ovisocr3` crate（D-1）+ C4 同输入双 adapter 逐字节漂移检测 golden + 架构白名单增补（新 crate 依赖面 + ocrx-inference 白名单 + dev-dep 白名单）。
- O3-T12：service 三读取点统一（session 构造/identity/static_identity 全走 `adapter_for(manifest)` 与 per-model registry 映射）；env 门控真实 prepare 断言 ovisocr3 session 身份（model ovisocr3/quality + adapter ovisocr3/0.1.0）。
- O3-T13：runtime 身份单源化（冻结 pin 常量上移 inference-api；registry 与 backend factory 同源）+ 单源测试；协议约束复核由 spike 证据覆盖（分支 1 零改）。
- O3-T14：fake 双模型 prepare 矩阵（绑定门→设备门→资产门贯通）+ manager 面 install_plan 双模型（URL 基址/字节/许可各自冻结表）。

## Phase 3 ✅（O3-T15–T19）

- O3-T15：prompt pack 注册表（`prompt/{mod,ovisocr2,ovisocr3}.rs`，冻结常量私有化——唯一公共入口 `pack_for(ModelId)`，结构性杜绝残留全局读取点）；C3 字节判别；三读取点贯穿（请求构造/预比较/checkpoint triplet 经 ScanOcrConfig/pack）。
- O3-T16：模型 id 全贯穿（`DocumentOptions.model` 默认 ovisocr2；app 三处硬编码改读请求；scan `--model` 放行 {ovisocr2,ovisocr3}，错误串同款）；**C7 捕获 e2e**（env 门控 CLI 子进程 + 组合根捕获 seam `OCRX_E2E_CAPTURE_OUT`）：双模型每个 TaskRequest.prompt 逐字节 == 冻结 pack + checkpoint 三元组一致（真实资产验证）。
- O3-T17：跨模型 checkpoint 否决双方向判别（pre/final compare；同模型/混合 profile 语义复核）。
- O3-T18：阶梯冻结态判别（2048 rung/1200 milli×64/切栏 + SUPPORTED_IMAGE_TOKEN_GRIDS 不变）+ ovisocr3 请求走同一网格/采样契约；think 通道（非空→think_removed/空→静默计数）由 adapter 测试钉死。
- O3-T19：C7/C14 全链路矩阵扩展——prepare→窗口→checkpoint→**零加载续扫**（capture 文件不存在 = 工厂从未被触达）→**多文档共享 session**（双输入单 prepare，prompt 逐字节一致）；架构脚本 0。

## Phase 4 ✅（O3-T20–T24）

- O3-T20：help golden（scan/model 双面 --model 行）+ 错误串统一复核（D-6 同款 `unsupported model: …`）；§8.3 ①②④ 复核通过。
- O3-T21：doctor 双模型面（D-11：无参逐字节不变 golden 保持；`--model` 选择器 + ovisocr3 新面创建即冻结——注入快照 golden + 子进程测试；`--model ovisocr2` ≡ 无参）；**result.json N1 接线**（非默认模型从 `DocumentOutcome.identity` 派生 id/name/family/profile/revision/mmproj sha/adapter/runtime/quantization；ovisocr2 缺省形态含历史 `llama.cpp@0.1.152` 逐字节不变）；零加载续扫诚实重写 model:null（双模型断言）；真实 ovisocr3 result.json 派生对象 e2e 验证。
- O3-T22：README/README.zh-CN Scope 双模型化 + THIRD_PARTY_NOTICES(.zh-CN) 增补 OvisOCR3（Apache-2.0、michalland provenance、灾备仓库记录）。
- O3-T23：打包/安装脚本模型无关复核（package-windows-x64.ps1/install.ps1 无模型假设，权重永不随包）；发布说明模板 `docs/plan/release-notes-dual-model-template.md`（双模型安装指引 + 冻结表 + 供应链）；CI 真实模型 E2E 保持 env 门控（不强求 runner 资产）。
- O3-T24：本日志滚动 + 计划 §10.2 勾选同步。

## Phase 5 ✅（O3-T25–T29；T26 部分交付——定界诚实）

- **O3-T18/D-5 生产证据修订（两次）**：① 真实全页扫描暴露基础预算不足——单页矩阵实证
  页 6 在 2048 网格下从 0 字节恢复 2892 字节 → **基础预算 per-model**（ovisocr3=2048 /
  ovisocr2=1024 不变，`default_max_image_tokens` + 判别测试）；② 串行地板假设被复用探针
  证伪后如实修订——**混合 SSM 串行窗口地板**作为防御性语义保留（判别测试钉死），scan
  路径生成退化根因仍未定位（直驱路径同页完美 → 差异收敛在 scan 路径，候选因素登记）。
- O3-T25：`assets/regression/ovisocr3-draft-v1/`（30 页 draft bootstrap，页分类承自已评审
  同页复核，块内容来自真实扫描；validate-regression-set.py 通过；明确 NOT approved truth）。
- O3-T26（部分交付）：全量复评 runbook 固化 + 30 页同参对照样本（`spike-assets/
  ovisocr3-comparison-sample.json`；均值相似度 0.553、输出量 21KB vs 47KB）；全量分数
  对照超出本窗口算力，未执行。
- O3-T27：CPU 单机口径资源观测入验收报告（墙钟 819s vs 528s、串行地板、加载 502ms；
  GPU 口径留待 GPU 环境）。
- O3-T28：故障注入双模型 4 用例全绿（`real_scan_fault_injection.rs` 参数化）；续扫/多文档
  由 C7/C14 e2e 覆盖。
- O3-T29：验收报告落 `docs/develop/ovisocr3-integration-acceptance-report.md`——
  §11 完成定义逐项核对（G1 能力面✅/质量面⚠️、G2✅、G3✅、G5✅、门禁✅、G4 部分交付、
  合规✅、边界✅、可追溯✅）+ 质量差距根因分析（已排除四假设 + 候选因素）+
  **D-GATE 建议：不切换默认模型（R5 口径），接入按"能力可用 + 差距入档"交付**。

## 评审响应（2026-11-14，代码评审 BLOCK 修复）

- **BLOCK（检空盲区）**：立即 EOS 的空 OCR 结果原本绕过退化阶梯——被写成 Complete、计入
  成功页、可被 `--resume` 复用（14/27 实测空页 = 静默内容丢失风险）。修复（TDD，判别
  测试 4 例）：① 空 OCR 最终文本（含纯空白）即 degeneration 案例，附 `empty_ocr_output`
  警告——进入阶梯重试（rung 恢复则干净提交，早退）；② 阶梯耗尽仍空 → 提交为退化页
  （result.json `partial` + 退化计数；退化页永不复用——既有 resume 语义自动生效）。
  双模型同时生效（共享安全网，ovisocr2 goldens 无空 OCR 页不受影响）。附带修复
  `detect_column_bands` 对退化几何（1 像素宽页）的越界 panic → 回退单带。
  附带测试假体修复：FakeSession slot 解析支持 `page-N-band-M` 请求 id。

## 后续工作（验收报告 §遗留）

1. scan 路径生成退化 root-cause 立项（最高优先）。
2. 修复后按 runbook 执行全量 OmniDocBench 对照。
3. 检空盲区（立即 EOS 不被退化阶梯拦截，双模型通用）。
4. ovisocr3-draft-v1 升级 approved truth（人工评审）。
5. 串行地板移除条件：上游 pin 修复混合模型批量语义。

---

## 提交序列

（每提交全绿门禁后登记：任务 / 内容一句话）

1. **Phase 0（O3-T01–T05）**：spike 探针（env 门控真实模型测试）+ spike 记录 + 冻结表 + 计划回写（D-2 分支 1 / D-4 GO / D-5 零改 / T13b 不触发）+ 实施日志。门禁：fmt/clippy/test/arch/release-scripts/bootstrap 全绿。
