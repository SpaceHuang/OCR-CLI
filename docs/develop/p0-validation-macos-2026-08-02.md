# OCR CLI v0.1 macOS P0 技术验证记录

状态：Partial / 存在阻断项  
日期：2026-08-02  
对应方案：`docs/develop/technical-design-v0.1.md`  
验证范围：v0.1 macOS Apple Silicon 范围内、无需扩充产品范围、可在当前 macOS 机器完成或预检的 P0 项；Windows 和其他跨平台验证延期

## 1. 结论摘要

本轮已在 Mac mini M2 16 GB 上跑通 OvisOCR2 Q4_K_M + BF16 mmproj 的 CPU 与 Metal `llama-mtmd-cli` 主路径，并用 Rust binding 完成真实 model/mmproj 加载、图片 tokenization 和 MTMD Metal 编码。固定的 llama.cpp b10210 可作为 P0 候选 commit；`llama-cpp-2` 可作为 binding 候选；技术方案采用的 `libmtmd` + adapter 边界具备本机实证。

本轮仍不能宣告整个 P0 通过，原因是：

1. 尚未建立需求规定的 30 页人工真值回归集，不能冻结质量、profile 量化映射或 DPI/像素预算。
2. Rust runtime Metal 路径的真实生成、CLI 结构化输出对比、EOS、max-token、协作取消和进度事件均已完成并冻结；v0.1 明确不做运行时 CPU fallback，启动仅做轻量 Metal 探测，模型初始化延迟到首次 OCR，Metal 失败时返回可诊断错误。
3. `llama-cpp-2` 0.1.152 实际解析 `llama-cpp-sys-2` 0.1.153，后者内嵌 llama.cpp commit `9e3b928f...`，与 CLI b10210 的 `00054751...` 不同；必须解决 commit 对齐后才可冻结 binding/runtime 组合。
4. Q4_K_M 与 mmproj 来自社区转换仓库；虽然固定 revision、字节数、SHA-256 和真实推理均已验证，且官方模型仓库和社区 GGUF 仓库均标注 Apache-2.0，但转换 provenance、官方 BF16 对照、项目再分发责任和其他 profile 尚未完成。
5. PDFium 已完成 macOS arm64 单页 300 DPI Rust 动态加载/渲染、100 页顺序渲染及加密/损坏错误分类冒烟，但峰值内存曲线、许可证审计和 Windows 打包仍未验证。

因此当前结论为：“macOS OvisOCR2/MTMD 核心可行性已通过，质量、版本对齐、PDF 渲染和发布来源仍是 P0 剩余门禁”。可以开展不依赖最终质量数值的接口/流水线 spike，但不能宣称完整 CLI 已获准进入发布实现。

## 2. 验证环境

| 项目 | 实测结果 |
|---|---|
| 设备 | Mac mini `Mac14,3` |
| SoC | Apple M2，8 CPU cores、10 GPU cores |
| 内存 | 16 GB |
| 架构 | arm64 |
| OS | macOS 26.5.2，Darwin 25.5.0 |
| Metal 硬件能力 | `system_profiler` 报告 `Metal: Supported` |
| Apple Clang | 21.0.0，target `arm64-apple-darwin25.5.0` |
| Metal 编译器 | Metal Toolchain 17F109；标识 `com.apple.dt.toolchain.Metal.32023.883` |
| Rust/Cargo | 1.97.1 |
| CMake | 4.4.2 |
| llama.cpp | Homebrew b10210；源码 commit `000547513f1530346ecd163db8b3e13962949961` |
| 可用磁盘 | 约 149 GiB，足够本期模型候选下载 |

说明：Xcode 默认 `xcrun metal` 映射仍报缺少组件，但 `xcodebuild -showComponent` 显示 toolchain 已安装；显式设置 `TOOLCHAINS=com.apple.dt.toolchain.Metal.32023.883` 后 Metal 32023.883 可执行，且 b10210 Metal 构建和推理成功。构建脚本必须显式处理/诊断该 Xcode 环境问题，不能仅以 `xcrun --find metal` 判断可用性。

## 3. 验证项与结果

### 3.1 macOS 首发平台与 Metal

结果：通过本轮冒烟。

- Apple Silicon arm64、16 GB 和 Metal-capable GPU 已实测确认。
- Apple Clang 可用。
- 下载并安装 Metal Toolchain 17F109 后，默认 Xcode 映射未生效；显式指定 toolchain 标识可正常运行 Metal compiler。
- 从准确标签 b10210/commit `000547513f1530346ecd163db8b3e13962949961` 构建 `GGML_METAL=ON`、嵌入 Metal library 的 `llama-mtmd-cli` 成功。
- 运行日志确认 GPU `Apple M2`、统一内存、Metal backend、model layer offload 和 CLIP/mmproj Metal backend 均实际启用。
- Q4 单页 Metal 冒烟图像编码 1.805 秒，总进程 9.12 秒，退出码 0；`/usr/bin/time -l` 报 maximum RSS 1,105,412,096 bytes、peak memory footprint 623,199,720 bytes、0 swaps。

冻结结论：可确认 macOS Metal 技术路径成立，并把“嵌入 Metal library”作为优先发布方式。最终构建配置仍需 release 工件复现、最低 macOS 14 运行验证和 100 页资源测试后冻结。

### 3.2 PDF 渲染候选

结果：macOS arm64 动态库与 300 DPI Rust 渲染冒烟通过，尚未最终冻结。

- PDFium 官方定位包含 PDF 解析与栅格化能力。
- `pdfium-render` 支持运行时动态绑定或静态链接，并显式支持 `aarch64-apple-darwin`。
- 社区持续构建的 PDFium binaries 提供 macOS arm64、macOS x64、Windows x64 工件，覆盖 v0.1 首发平台组合。
- `pdfium-render` 不自带 PDFium；若采用它，发布流程必须固定 PDFium binary revision、SHA-256、许可证清单和动态库装载位置。
- 下载并固定 `bblanchon/pdfium-binaries` `chromium/7961` 的 `pdfium-mac-arm64.tgz`：3,429,737 bytes，SHA-256 `1193a771e0bd934530afa3df73a0d44551d8f4078442e290054e6dd38ded960f`；本地校验与 GitHub Release digest 一致。
- 工件包含 arm64 `libpdfium.dylib`、头文件、主许可证及第三方许可证集合；动态库 7,151,568 bytes，版本 152.0.7961.0。
- `pdfium-render` 0.9.3 使用 `pdfium_7881` ABI feature 成功动态绑定较新的 PDFium 7961，打开一页 336×432 pt PDF，并按 `points * 300 / 72` 渲染为精确 1400×1800 RGB8 PNG。
- 输出 PNG SHA-256 为 `eee42b112c9cdf68675df5ef8da6378b173c81cc957c735b67abdf704428fda6`。
- 首次仅启用 `pdfium-render` 的 image feature 时，RGB bitmap 已成功生成，但 PNG 保存因未启用 encoder 返回 Unsupported；显式加入 `image` crate `png` feature 后成功写出。这一 feature 必须进入实现依赖，不能只依赖传递默认。
- 使用同一正常 PDF 构造截断损坏文件和口令加密文件；不提供口令打开时，PDFium 分别稳定返回 `FormatError` 和 `PasswordError`，进程均以非零状态退出。底层具备将“损坏”和“加密”映射为方案所定义错误类别的能力。
- 将同一页复制为 100 页合成 PDF 后，以 release 构建逐页渲染为 1400×1800 RGB8、每轮作用域结束即释放 bitmap/image，并关闭 PNG 落盘；100 页全部成功，墙钟时间 2.45 秒。该夹具只能验证顺序处理链路，不代表真实复杂 PDF 性能。
- 当前沙箱中的 `/usr/bin/time -l` 因 `sysctl kern.clockrate` 权限未返回 RSS 字段，因此不能据此冻结逐页内存上限或宣称无增长。
- 尚未执行非 PDF 文件、错误口令/正确口令恢复、可观测 RSS 曲线、macOS 14、Windows 动态库和完整许可证审计。

建议：把“`pdfium-render` 0.9.3 + 固定 PDFium 7961 动态库 + 显式 `image/png` feature”升级为 macOS 已验证候选。只有完成故障/长文档、双平台打包、ABI 风险和许可证审计后才能正式冻结。

### 3.3 llama.cpp `libmtmd`

结果：OvisOCR2 专用 CPU/Metal 冒烟通过。

- llama.cpp 官方文档确认 `libmtmd`、`llama-mtmd-cli`、图像输入、独立 model/mmproj 以及 image min/max token 参数存在。
- 官方说明 OCR 模型需要模型专用 prompt 和输入结构，与方案中的 adapter 边界一致。
- 当前官方多模态预量化模型清单仍未列出 OvisOCR2，所以本项目必须维持自己的锁定兼容矩阵。
- b10210 成功加载 Q4_K_M language GGUF 与 BF16 mmproj、编码 1400×1800 PNG，并生成自然阅读顺序 Markdown、HTML table 和有效 bbox 标签。
- CPU 固定 1024 image tokens 时图像编码 111.632 秒、总进程 125.46 秒；Metal 图像编码 1.805 秒、总进程 9.12 秒。两者输出逐字一致。
- CPU 默认图像 token 预算的首次运行编码耗时 301.716 秒，说明必须由 adapter/manifest 显式约束 image token，而不能直接使用运行时默认。
- CPU 首次运行中，Ctrl+C 在图片编码阶段未立即返回，编码结束后才继续处理；第二次中断才退出。需求中的“停止当前生成”必须通过 Rust 层取消实验单独验证，不能假设 CLI 已满足。
- CPU 与 Metal 均出现 `non-consecutive token position` warning，但仍输出相同完整结果。该警告需在真实回归集确认无正确性影响后才能接受。
- 输出包含空 `<think>...</think>`，`llama-mtmd-cli` 没有 `--reasoning` 参数。adapter 必须用准确 chat template 关闭 thinking，或仅对确定为空的包裹做受控清理；不能直接把当前 CLI 输出视为满足需求。
- 冒烟页中的 `Equation: E = mc^2` 未转成 LaTeX；单个人工样例不能据此判定模型不合格，但证明质量能力必须由需求回归集评分，不能由“端到端有输出”替代。

冻结结论：b10210 是已验证可工作的 CLI 候选 commit；Rust `llama-cpp-2 0.1.152` + `sys 0.1.153` 已完成 macOS M2 Metal 真实生成 candidate 验证。由于 sys 内嵌 commit 与 b10210 不同，仍需完成结构化输出对比、取消/进度和 CPU fallback 验证后才能升级为最终锁定 runtime。

### 3.4 Rust binding

结果：真实加载、tokenize、MTMD Metal encode 通过，完整生成尚未验证。

- `llama-cpp-2` 0.1.152 文档包含安全 MTMD wrapper、image input chunk、MTMD context、GPU 开关以及 image min/max token。
- 该模块明确标记为 experimental，API 可能发生破坏性变化。
- `llama-cpp-2` 0.1.152 最小程序编译成功，并在 M2 上报告 `gpu_offload=true`。
- Rust spike 成功加载 Q4 language model 和 BF16 mmproj，报告 `vision=true`、`mrope=true`。
- 1400×1800 图片被解析为 3 个 chunk、1014 tokens、42 positions，其中 image chunk 为 1008 tokens/36 positions；`encode_chunk()` 通过 Metal 成功返回。
- 版本解析发现：只固定 `llama-cpp-2 = 0.1.152` 会选择 `llama-cpp-sys-2 = 0.1.153`。通过 crates 发布 commit `b48ce80b...` 的 Git tree 追踪到 sys 内嵌 llama.cpp commit 为 `9e3b928fd8c9d14dbf15a8768b9fdd7e5c721d66`，而 CLI 候选为 b10210/`00054751...`。
- 尚未在 Rust spike 中执行 `eval_chunks`、采样生成、stop/EOS、取消和逐 token 进度，也未与同 commit CLI 对比完整输出。

建议：继续把 `llama-cpp-2` 作为首选候选，但必须精确锁定 wrapper、sys crate 和内嵌 llama.cpp commit，不能依赖 Cargo 的兼容版本范围。优先选择/提交一个与最终 CLI commit 对齐的 binding revision；若无法对齐，再评估固定 fork 或最小内部 FFI。

### 3.5 OvisOCR2 官方契约

结果：产品语义与方案一致，可记录为 adapter 输入依据。

官方模型卡确认：

- 模型为 OvisOCR2 0.8B/约 0.9B 参数页面级文档解析模型。
- 输出自然阅读顺序 Markdown，覆盖正文、LaTeX 公式、HTML table 和视觉区域。
- bbox 标签为 `images/bbox_{left}_{top}_{right}_{bottom}.jpg`，坐标缩放到 `[0,1000)`。
- `enable_thinking=false`、`temperature=0.0`、`max_tokens=16384`。
- 图像约束为最小 `448*448`、最大 `2880*2880` 总像素。
- 官方重复尾部清理参考参数为：最小文本长度 8000、周期 1..200、最少重复字符 100、最少重复 5 次。
- 官方模型许可证标记为 Apache-2.0，并明确关键场景仍需人工校验。

这些证据可以冻结 v0.1 OvisOCR2 adapter 的初始 prompt 语义、采样默认值、像素预算和 bbox 原始格式。重复清理参数只能作为初始回归基线，必须通过项目真值集验证后才能成为产品阈值。

### 3.6 GGUF、mmproj 与 profile

结果：成套 compact 候选已通过哈希和真实推理验证，来源策略尚未冻结。

- 选用 `prithivMLmods/OvisOCR2-F32-GGUF` 固定 revision `fb1777a83de20385620634fe675cb86504dc6c92` 执行冒烟。
- Q4_K_M：529,297,280 bytes，SHA-256 `b50261b57366bebd2328e3ebcab0233890150c7a596a6def6a2cc09cd91da143`。
- BF16 mmproj：207,346,336 bytes，SHA-256 `842015fc8bb09d8b953a04b1ba329510e7259081422a9dab50f41ce4fac430a1`。
- 下载后本地重算 SHA-256 与 Hugging Face LFS oid 完全一致。
- `bartowski/ATH-MaaS_OvisOCR2-GGUF` 声明使用 llama.cpp b10068 量化，但其仓库说明同时提示新模型可能需要等待运行时更新。
- bartowski 固定 revision `ab22420f...` 中的 BF16 mmproj 与上述候选 SHA-256 完全相同，说明两仓库引用的是同一 projector 文件；不能仅凭这一点推断所有 language quant provenance 相同。
- 官方 OvisOCR2 仓库提供 Safetensors BF16，而不是需求需要的已锁定 GGUF/mmproj 发布组合。

冻结结论：官方 `ATH-MaaS/OvisOCR2` 的模型来源和 Apache-2.0 标识已确认；社区 GGUF 仓库也公开列出 Q4/Q5/Q8/BF16 与 mmproj 文件并标注 Apache-2.0。经决策，固定 revision、大小和 SHA-256 的 Q4/mmproj 作为当前 macOS 开发候选，不因尚未找到完整 Safetensors→GGUF 转换 provenance 而阻断开发；正式生产 manifest 仍需记录 provenance 限制、来源责任链和 NOTICE。

### 3.7 profile 与性能

结果：只完成 compact 单页冒烟，不足以冻结 profile。

本机 Q4_K_M/1024 image token 的 Metal 单页冒烟总耗时约 9.12 秒，CPU 约 125.46 秒；两者文本输出一致。该数据只证明 Metal 加速收益和 16 GB 可加载 compact，不代表 300 DPI 文档性能或质量。尚未运行 Q5/Q8/BF16、200/250/300 DPI、官方 BF16 对照和 30 页回归，因此 `balanced` 是否采用 Q5、16 GB 默认档以及全部质量阈值仍不能决定。

### 3.8 验证工件与复现边界

本轮临时工件位于 `/private/tmp/ocrx-p0-20260802/`，包括固定 GGUF/mmproj、合成冒烟图片、CPU/Metal stdout/stderr、b10210 源码/构建和 Rust spike。它们不属于产品发布工件，系统清理 `/private/tmp` 后可以丢失；关键 revision、hash、参数和结论已写入本文。

合成图片为 1400×1800 RGB PNG，SHA-256 `1a9407b95efe5a8458490ce749abdb354a513ae7fc2325ea64c9885fa7c25e07`，内容覆盖英文正文、简单公式、3×3 表格和柱状图。它仅用于链路冒烟，不进入质量回归集。

## 4. 决策状态更新

| 决策 | 本轮状态 | 结论 |
|---|---|---|
| macOS arm64 首发硬件 | 已验证 | 当前 M2/16 GB/Metal-capable 设备符合验证基准 |
| PDF renderer | macOS 冒烟通过 | `pdfium-render` 0.9.3 + PDFium 7961 成功输出 300 DPI RGB PNG、完成 100 页顺序渲染并区分 `FormatError`/`PasswordError`；待内存/双平台验证后冻结 |
| Rust binding/runtime | macOS Metal runtime 已冻结 | `llama-cpp-2` 0.1.152 + sys 0.1.153 已完成真实生成、结构化对比、EOS、max-token、协作取消和进度事件；不承诺逐字节等同；v0.1 不做 CPU fallback |
| llama.cpp commit | 候选通过 | b10210/`00054751...` CPU 与 Metal 推理成功；待完整回归后冻结 |
| OvisOCR2 官方 prompt/像素/bbox | 可作为基线 | 与需求一致；以固定官方 revision 纳入 adapter |
| GGUF/mmproj 来源 | P0 候选通过 | 固定 revision/hash 的 Q4+mmproj 成套推理成功；生产 provenance/再分发未冻结 |
| profile 量化/资源阈值 | 仍阻断 | 仅运行 compact 冒烟，未运行四 profile 回归 |
| Metal backend | 冒烟通过 | 同 commit 源码构建、M2 初始化、model/mmproj offload 和输出成功 |
| Windows/跨平台支持 | 延期 | v0.1 不支持 Windows，不进入当前发布承诺和 P0 冻结门禁 |
| 页面失败策略 | 已由产品决策确认 | 页面失败继续处理，记录失败明细，最终 `partial` 并返回退出码 10；错误字段和恢复行为仍需实现后契约测试 |
| Skill 安装层级 | 未涉及 | 属于安装器/宿主目录兼容性决策，仍待验证 |

## 5. macOS 剩余 P0 工作

1. 将 Rust wrapper/sys/llama.cpp 对齐到同一 commit：作为后续一致性维护项，不阻断当前已冻结的 macOS Metal runtime 契约。
2. 冻结官方 OvisOCR2 Safetensors revision、GGUF/mmproj 转换 provenance、许可证和再分发策略。
3. 建立至少 30 页人工真值集，运行官方 BF16、GGUF BF16、Q8、Q5、Q4 全量质量评分。
4. 用真实 PDF 渲染比较 200/250/300 DPI、送模像素/token 预算、耗时和峰值内存。
5. 继续验证已通过正常/加密/损坏及 100 页顺序渲染冒烟的 `pdfium-render` + PDFium 7961：非 PDF 与口令恢复、可观测 RSS 曲线、真实复杂长文档和 macOS 发布打包；macOS 14 实机验证属于发布阶段的版本支持验收，不阻断开发；Windows 延期。
6. 解释或消除 `non-consecutive token position` warning，验证空 thinking 标签处理不会误删正文。
7. 验证单次 Ctrl+C 在 Rust 生成和图片编码阶段的响应边界，保证 checkpoint/退出码契约可实现。

完成以上项目之前，b10210、Q4 文件和 binding 均保持“候选已冒烟”，不标记为最终生产冻结。

## 6. 参考来源

- [OvisOCR2 官方模型卡](https://huggingface.co/ATH-MaaS/OvisOCR2)
- [llama.cpp 多模态文档](https://github.com/ggml-org/llama.cpp/blob/master/docs/multimodal.md)
- [`llama-cpp-2` MTMD 源码文档](https://docs.rs/crate/llama-cpp-2/latest/source/src/mtmd.rs)
- [`pdfium-render` 官方仓库](https://github.com/ajrcarey/pdfium-render)
- [PDFium macOS arm64/Windows x64 预编译工件说明](https://github.com/bblanchon/pdfium-binaries)
- [OvisOCR2 社区 GGUF 候选](https://huggingface.co/bartowski/ATH-MaaS_OvisOCR2-GGUF)
- [含 mmproj 的社区 GGUF 候选](https://huggingface.co/prithivMLmods/OvisOCR2-F32-GGUF)

## 7. 本轮补充的 macOS 自动验证（2026-08-02）

### 7.1 当前仓库与验证前置条件

本轮在仓库根目录执行检查。仓库当前已经建立 Rust workspace、7 个 crate、bootstrap CLI、基础测试入口、PDFium 渲染边界、仓库内确定性 PDF fixture 和多个 macOS 验证脚本；但 manifest、真实 runtime、完整 CLI 流水线、完整输出/恢复契约、安装脚本和 Skill 安装逻辑仍未实现。因此，本节可以冻结已执行的工程前置事实和 PDF 基础边界，不能把尚未实现的完整 CLI 契约标记为通过。

| 检查项 | 结果 | 证据/限制 |
|---|---|---|
| macOS 版本 | 通过 | `26.5.2` |
| CPU 架构 | 通过 | `arm64` |
| 硬件 | 通过 | `Mac14,3`、Apple M2、8 CPU cores、10 GPU cores |
| Metal | 通过 | `system_profiler` 报告 `Metal: Supported` |
| Rust/Cargo | 已安装 | `/opt/homebrew/bin/rustc`、`cargo` 可发现；workspace 可执行 build/test |
| CMake/Xcode 工具 | 已安装 | `cmake`、`xcrun`、`xcodebuild`、`vmmap`、`/usr/bin/time` 可发现 |
| 内存读取 | 受限 | 当前沙箱中 `sysctl hw.memsize` 返回 `Operation not permitted`；以系统硬件报告中的 16 GB 为准 |
| P0 临时工件 | 存在 | `/private/tmp/ocrx-p0-20260802/` 含模型、PDFium、llama.cpp、Rust spike 与冒烟日志 |

这次检查确认当前机器仍满足原 P0 验证基准，并确认 workspace、基础测试 runner、PDF fixture 和正式 `ocrx-pdf` 边界已经存在。仍没有新增真实 runtime、完整输出或恢复契约的通过证据；`/private/tmp` 工件仍不是仓库资产，不能替代可重复的模型 fixture 和发布验证入口。

### 7.2 可立即纳入实现的自动门禁

以下门禁不需要人工判读，待 CLI 骨架建立后应作为 macOS 本机测试套件的一部分执行：

1. CLI 参数、页码、配置优先级、退出码，以及 stdout/stderr 隔离。
2. `result.json`、错误 JSON、checkpoint、JSONL 和 Markdown 的 schema/golden 校验。
3. 输出目录覆盖、路径逃逸、软链接、临时文件和原子 rename 故障注入。
4. checkpoint 兼容键、输入 SHA-256 或 model revision 改变后的恢复拒绝。
5. SIGINT 取消、返回 130、当前页临时文件清理和已完成页保留。
6. 非 PDF、损坏 PDF、加密 PDF、错误密码、正确密码和空文档错误分类。
7. PDFium 重复渲染的尺寸、RGB8 色彩空间、像素 hash 和动态库加载失败行为。
8. 100 页 RSS 曲线、bitmap 释放、长生成 heartbeat 和 OOM 防护。
9. 下载 Range/ETag/hash/大小失败、磁盘不足、`.part` 事务和 `--offline` 网络阻断。
10. manifest model ID、profile、revision、model/mmproj 配对和远端字段不可篡改。
11. doctor 结果与 macOS 硬件/Metal/磁盘/缓存状态的一致性，且不得隐式下载模型。
12. 日志、result 和错误输出中的 prompt、正文、token、代理凭据及带查询参数 URL 脱敏。

这些门禁可以冻结工程契约和失败安全性，但不能冻结 OCR 语义质量、许可证法律结论或 Windows 行为。

### 7.3 当前 P0 状态调整

本轮不改变原有候选结论：macOS OvisOCR2/MTMD 与 PDFium 技术路径仍为“冒烟通过”，不是生产冻结。仓库已具备最小 workspace、fixture 和测试 runner；项目可以正式进入开发阶段。CLI、manifest、完整输出恢复契约和真实复杂 PDF 资源验证属于开发任务及后续发布门禁，不再解释为项目启动前的未决架构问题。后续按 7.2 的门禁顺序推进，已实现的 PDF/基础契约先执行，未实现部分记录为开发任务而不宣称通过。

### 7.4 非性能/非效果冻结审计（2026-08-02）

本轮对需求和技术方案中所有非性能、非 OCR 效果门禁进行了实现审计，并执行了当前已有入口。结论分为两类：

| 类别 | 状态 | 可冻结结论 |
|---|---|---|
| workspace build/test、model ID 枚举、bbox 基础不变量 | 已验证 | 仅允许 `ovisocr2`；bbox 必须为非空、半开区间 `[0,1000)` |
| PDF 输入分类、300 DPI/RGB8、PNG 落盘、密码恢复、动态库失败、worker 隔离方案 | 已验证并冻结开发方案 | 独立 worker、默认 20 页半开页段、`.part.png` 原子落盘、精确页数校验和非法页段拒绝已验证；正式 CLI checkpoint/IPC/崩溃恢复仍待实现 |
| CLI 参数/页码/配置/退出码/报告/JSONL | 未实现 | bootstrap CLI 只有 `version` 和 `doctor`，不能冻结 |
| checkpoint、resume、overwrite、SIGINT、原子输出 | 未实现 | `ocrx-output` 只有状态枚举，不能冻结 |
| manifest、模型缓存、下载、offline、hash/ETag/许可 | 未实现 | `ocrx-models` 只有身份类型，不能冻结 |
| 真实 runtime、取消、生成终止原因、错误映射 | 未实现 | `ocrx-runtime` 只有 fake runtime，不能冻结 |
| doctor 完整 JSON、profile 推荐、安装状态 | 未实现 | 当前 doctor 只输出 bootstrap 文本，不能冻结 |
| Skill 安装/卸载和安装脚本 | 未实现 | `ocrx-skill` 只有常量，不能冻结 |
| 发布包、签名、动态依赖、干净环境安装 | 未实现 | 尚无发布工件，不能冻结 |

因此，本轮不能宣称“非性能和效果部分全部验证完成”。已验证部分可以冻结为开发基线；未实现部分属于产品实现阻断，而不是测试失败。继续冻结必须先完成对应实现，再用自动化测试验证，不能通过修改文档或增加空测试绕过。
