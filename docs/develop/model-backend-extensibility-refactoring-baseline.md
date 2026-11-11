# 模型后端可扩展架构重构——阶段 A 基线记录（T001–T008）

**分支：** `refactor/model-backend-extensibility`（worktree `D:/Develop/OCR-CLI/.worktree/model-backend-ext`，基于 main `2f6f637`，2026-11-12）

## T001 工作区状态与修改清单

- 主工作区 `D:/Develop/OCR-CLI`（main）：仅一个未跟踪目录 `assets/regression/symbol-coverage-v0.1/`（用户内容，本重构不触碰）。主工作区不改动；`.worktree/` 已在 `.gitignore`。
- 本 worktree 新增 `vendor/pdfium/bin/pdfium.dll`（自主工作区复制；gitignored 环境资源）。
- 本重构允许修改的文件清单：根 `Cargo.toml`/`Cargo.lock`；`src/ocrx-{core,models,runtime,ovisocr2,output,pdf,cli,skill}`；新 crate `src/ocrx-{primitives,inference-api,inference,scan,eval}`；`scripts/`（新增架构检查脚本、bootstrap 更新）；`docs/develop/` 本文档与实施日志；`src/ocrx-cli/tests/`、各 crate 测试。不修改 `assets/regression/` 既有内容与用户未跟踪文件。

## T002 基线构建与测试

- `cargo build --workspace` → **exit 0**；`cargo test --workspace` → **exit 0**，428 passed / 0 failed / 21 ignored（原始日志 `baseline-build-test.log`）。
- ignored 全部为环境门控：multi_doc 真实模型 9 项（`OCRX_E2E_MODEL=1`）、fault injection 2 项、ovisocr2 gated real-model 1 项、PDF 探针 subprocess 4 项、其余为 doc/doc-tests 类。无重构前失败。
- 结论：**无既有失败需要区分**，全部为通过基线。

## T003 CLI golden（Windows 10，CPU-only 构建，PDFium 在位）

| 命令 | stdout（关键行） | stderr | exit |
|---|---|---|---|
| `version` | `ocrx 0.1.0` | — | 0 |
| `doctor` | `model=ovisocr2 backend=Cpu image_token_limit=1024 memory_gb=63 metal_available=false`（+后续行） | — | 0 |
| `doctor --report json` | `{"schema_version":2,"platform":"windows","model":"ovisocr2","backend":"Cpu","image_token_limit":1024,"memory_gb":63,...,"recommended_profile":"quality","recommended_device":"cpu","skill_installed":true,"profiles":["compact","balanced","quality","reference"]}` | — | 0 |
| `model list` | `model=ovisocr2` | — | 0 |
| `model info` | `model=ovisocr2 profiles=balanced,compact,quality,reference installed=` | — | 0 |
| `scan does-not-exist.pdf` | — | `error code=3 kind=input unable to read PDF page count` | 3 |
| `scan <dir>`（多输入缺 `--output-dir`） | — | `error code=2 kind=usage scanning multiple inputs requires --output-dir <output-root>` | 2 |
| `scan --device tpu missing.pdf` | — | `error: unsupported or incomplete command: scan` | 2 |
| routing sidecar 写失败（代码路径 main.rs:3961/3414） | — | `error code=5 kind=output routing checkpoint commit failed`（部分 sidecar 变体为 `unable to persist partial routing sidecars`） | 5 |

保留特例（不得顺手统一）：单输入缺失文件=页数失败 exit 3；sidecar 提交失败=code 5/kind output/exit 5；多输入缺 `--output-dir`=usage 2。

## T004 页面计划与产物 golden

- 本机 PDFium 在位（`vendor/pdfium/bin/pdfium.dll`），无"环境未执行"项。
- 可重复 fixture 与预期输出由以下已通过的测试承载（基线全绿即证据）：
  - native/empty/auto fallback/OCR 队列与产物：`ocrx-cli/tests/cli_subprocess.rs`（264 行）、`ocrx-pdf` 各测试（199/130/99 行）、core/output 单测。
  - 多输入编排/scan-summary/退出码聚合：`ocrx-cli/tests/multi_doc_orchestration.rs`（909 行）、`multi_input_contract.rs`（206 行）。
  - 真实模型门控项（OCRX_E2E_MODEL）标注为"环境可选，未执行不阻塞"。

## T005 Ovis 四 profile 内置 manifest 字段清单（与 `ocrx-models` 代码逐项核对）

- revision（全部 profile 共用）：`8a22290a0c42dbcf84739d6d4c2763f877494ae0`
- 共享 mmproj：sha256 `7e371704e8f3c88638830e52866dfa88b2fd383155e3a0f9484e4ce037fdd42f`，204,987,079 字节，URL `<base>/mmproj-F16.gguf`
- URL base：`https://huggingface.co/Abiray/OvisOCR2-GGUF/resolve/8a22290a0c42dbcf84739d6d4c2763f877494ae0`

| profile | 模型文件 | sha256 | 字节数 | quantization 展示 | display_name/family |
|---|---|---|---|---|---|
| compact | `OvisOCR2-Q4_K_M.gguf` | `4ae54c66bfe1efb2882854ff642c3d137aa082dbd0699bc9234ff39874a492a1` | 529,297,280 | `Q4_K_M` | OvisOCR2 / Ovis |
| balanced | `OvisOCR2-Q5_K_M.gguf` | `e17744e367adb891cc9c82f4255408cf46a00efa1b20a83f8187d5bd662ce89a` | 577,998,720 | `Q5_K_M` | OvisOCR2 / Ovis |
| quality | `OvisOCR2-Q8_0.gguf` | `3fba6d94312e550575a92d55cffb8d75997bf68a9f133c92ab6ad7f0bf2bc93e` | 811,843,498 | `Q8_0` | OvisOCR2 / Ovis |
| reference | `OvisOCR2-F16.gguf` | `bd8f2c62c930d74590abcb5f65f79e898672e458d6044e8b2e885daea948592e` | 1,516,744,617 | `BF16` | OvisOCR2 / Ovis |

- 缓存布局：`<cache_root>/ocrx/models/ovisocr2/<revision>/<profile>/model.gguf` + `<revision>/mmproj.gguf`（共享）。
- `recommend_profile`：<6 GiB 报 InsufficientMemory；<12→compact；<24→balanced；≥24→quality；首选不可行回退 compact。
- `recommend_device`：auto 优先级 `metal > cuda > vulkan > cpu`；显式设备不可用报错。
- adapter/version：`ovisocr2`/`0.1.0`（result 展示）。
- **基线差异（计划明示处置）**：main 当前 `ocrx_core::RUNTIME_COMPATIBILITY = ("0.1.152","0.1.153","9e3b928","ovisocr2")`，而 `Cargo.lock` 已是 llama-cpp-2/sys-2 **0.1.161**。按计划 §2.2（Q1.3 pins 修订）与 §5.1/T007/T075，新身份冻结为 wrapper=`0.1.161`、sys=`0.1.161`、upstream=`d81235049384534c167caea52b85a694f6103d14`；result 展示 runtime 串随之从 `llama.cpp@0.1.152` 变为 **`llama.cpp@0.1.161`**（计划 §7.4 表冻结值，属计划明示契约变更，非回归）。

## T006 ocr-markdown-v1 prompt 冻结（`ocrx-runtime/src/llama_mtmd.rs:26`）

字节级字面量（Rust 转义表示，开头 `\n`、中段 `\n`、结尾无换行）：

```text
"\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing."
```

- 拼接顺序：`mtmd_default_marker()`（图像 marker）在前 + prompt 原样（llama_mtmd.rs:824/1190）。
- 生成默认：max_output_tokens=16384（范围 128–16384）、image tokens 1024、context 20480、DPI 300、像素预算 2880×2880、贪心（temperature 0）、thinking 关闭、DECODE_GENERATION=2、批深分层 8/16（12 GiB 阈值，回退 8）。

## T007 格式盘点（字段级）

- **v5 checkpoint**（`ocrx-output` 写格式）：`ocrx-checkpoint-v5\npage={n}\ninput={hex}\nmodel={hex}\nprofile={hex}\nparameters={hex}\ncomplete={bool}\noutput={hex}\ntruncated={bool}\ndegenerated={bool}\nattempts={n}\nraw={hex}\nwarnings={hex}\nthink_blocks_stripped={n}\naudit={hex}\n`；v4 及更旧=不支持重处理。
- **v5 兼容串**（main.rs:2920 + 后缀）：`pdf-mode={};quality-tier={};classifier=pdf-route-v1;extractor=pdf-text-v1;platform={};dpi={};tokens={};max_image_tokens={};context_size={};device={};batch={};route=native;decode={}` + 可选 `;ocr-profile=`/`;ocr-device=`/`;pending-ocr=true`。native/empty 宽松匹配=前五参数（含 platform）。`decode`/`think_blocks_stripped` 为既有字段；v5 OCR complete 迁移必重处理，两字段不参与复用判定、缺省解析不失败。
- **result.json schema 3** 字段顺序（main.rs:4654）：`schema_version,job_id,status,input,markdown_path,output_dir,required_quality_tier,document_type,ocr_model_loaded,model,backend,pages{total,completed,failed,truncated,degenerated,think_blocks_stripped_total},routing_counts{native_text,ocr,empty,fallback},images{directory,count},elapsed_ms,warnings,diagnostics,pdf_routing,routing_metadata`。
- **model 对象**（main.rs:4622）：`id,name,family,profile,revision,mmproj_revision,adapter,adapter_version,prompt_schema_version(数值1),runtime,quantization`；`ocr_model_loaded=false` 时 `model=null`。
- **doctor schema 2**（main.rs:4881）：`platform,model,backend,image_token_limit,memory_gb,metal_available,cpu_available,cache_root,installed_profiles,recommended_profile,recommended_device,skill_installed,profiles`。
- **scan-summary.json schema 1**（main.rs:4255）：`schema_version,mode,inputs,output_root,documents[{input,output_dir,status,exit_code}],elapsed_ms`。
- **routing sidecar**：每页一个 routing JSON（page/classification/processing/fallback），失败路径见 T003。
- **step/commit profile**：`OCRX_STEP_PROFILE` 开关；`step-profile.json`/`commit-profile.json` 各文档一份，`schema_version:1` 黄金串，取消/失败零写盘（`ocrx-runtime/src/step_profile.rs`、`ocrx-output/src/commit_profile.rs`）。

## T008 质量模块盘点（ocrx-pdf → ocrx-eval 迁移映射）

| ocrx-pdf 现有项 | 目标（ocrx-eval） | 验证方式 |
|---|---|---|
| `QualityManifestEntry`/`load_quality_manifest`/`validate_quality_manifest`/`load_quality_manifest_for_input` | `manifest.rs` | 既有 TSV 合法/非法用例单测 |
| `QualityPageOutcome`/`QualityOutcomeSummary`/`summarize_quality_outcomes` | `metrics.rs` | 同 fixture 数值对比 |
| `normalized_edit_distance`/`block_precision_recall_f1`/`bbox_validity_rate`/`bbox_recall`/`reading_order_accuracy` | `metrics.rs` | 浮点 golden 逐项一致 |
| `RouteConfusionMatrix`/`route_confusion_matrix`/`route_accuracy` | `metrics.rs` | 同上 |
| `NativeReleaseMetrics`/`NativeReleaseGateReport`/`native_release_gate_report`/`validate_native_release_metrics` | `gate.rs` | 阈值/失败列表 golden |
| `PerformanceGateInput`/`validate_performance_gate`/`ReleaseReport`/`generate_release_report`/`validate_release_manifest` | `gate.rs`/`report.rs` | JSON golden 不变 |
| `scripts/score-regression.py`、`run-quality-gate.py`、`test-release-scripts.sh`、`bootstrap-check.sh` | 保持可执行，调用对象收敛到公开 CLI/文件契约 | `python -m unittest discover -s scripts`、`bash scripts/test-release-scripts.sh` |
