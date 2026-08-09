# OCR CLI v0.1 开发前置与 macOS 冻结计划

状态：待执行
日期：2026-08-02
对应方案：`docs/develop/technical-design-v0.1.md`
对应验证记录：`docs/develop/p0-validation-macos-2026-08-02.md`

## 1. 目标

本计划的目标不是直接实现完整 CLI，而是完成进入正式开发前的三件事：

1. 将当前只有文档的仓库初始化为可编译、可测试、可复现的 Rust 工程。
2. 在当前 macOS Apple Silicon 环境中完成所有可完成的技术验证，并把对应决策写入 ADR、锁定文件和验证报告。
3. 输出明确的进入正式开发结论；仍有影响进入开发的阻断问题必须单独列出，不以“候选已冒烟”代替冻结。

## 2. 当前基线

执行状态：阶段 A 已完成，阶段 B 正在进行。

阶段 A 已完成的证据：

- workspace、8 个 crate、`rust-toolchain.toml`、`Cargo.lock` 和 bootstrap CLI 已建立；
- `cargo fmt --all -- --check` 通过；
- `cargo build --workspace` 通过；
- `cargo test --workspace` 通过，2 个领域契约测试通过；
- `cargo run -p ocrx -- version` 和 `cargo run -p ocrx -- doctor` 通过；
- `scripts/macos-environment.sh` 已记录当前 macOS 环境，`scripts/bootstrap-check.sh` 已固定初始门禁。

阶段 A 中发现并修正了一个契约问题：bbox 终点坐标原本允许 `1000`，已按方案要求的 `[0,1000)` 修正为严格小于 `1000`。

阶段 B 已完成的基础设施工作：

- 增加 `scripts/macos-environment.sh`、`scripts/macos-p0-report.sh` 和 `scripts/bootstrap-check.sh`；
- 已在当前 macOS 执行环境快照、workspace bootstrap 和历史 PDFium 单页/100 页 runner；
- 已将执行结果记录到 `docs/develop/macos-p0-automation-2026-08-02.md`；
- PDFium、RSS 和资源稳定性尚未冻结，因为当前 runner 与模型/PDF 工件仍位于 `/private/tmp`，还未纳入正式 crate 和可复现 fixture。

阶段 B 的最新进展：

- `ocrx-pdf` 已接入锁定的 `pdfium-render 0.9.3` 和 `image 0.25.10`；
- 正式 crate 已用当前 PDFium 动态库完成 300 DPI 单页和 100 页 release 渲染；
- 已发现并记录 PDFium binding 的进程级单例约束，renderer 必须在扫描任务内复用一次 binding；
- `/usr/bin/time -l` 仍无法在当前沙箱返回 RSS，资源阈值尚未冻结；
- PDF fixture 和 PDFium 动态库仍在 `/private/tmp`，尚未成为仓库可复现资产。

阶段 B 的最新进展：

- 已增加无第三方依赖的确定性 PDF fixture 生成器和 runner；
- 仓库内已生成并验证单页、100 页、截断损坏、非 PDF 四类 fixture；
- 正常 PDF 通过正式 `ocrx-pdf` runner，损坏/非 PDF 稳定返回 `FormatError`；
- 加密/密码恢复、RSS/footprint 采样和正式 PDFium 发布工件仍未完成。

阶段 B 的最新进展：

- 已建立逐页延迟的 RSS 采样 runner；
- 在允许读取进程信息的 macOS 环境执行 100 页采样，初始化后 RSS 约稳定在 46.6–46.8 MB；
- 该结果只冻结“当前简单 PDFium 渲染链路未观察到按页持续增长”的候选结论，不冻结完整 CLI 资源阈值；
- 当前仍需真实复杂 PDF、模型加载/推理、PNG 裁图、checkpoint 生命周期和加密/密码恢复验证。

阶段 B 的最新进展：

- 正式 runner 已验证加密 PDF：无密码返回 `PasswordError`，正确密码 `ocrx-test` 可打开并渲染；
- 已验证 PDFium dylib 为 arm64，依赖可解析，当前工件 SHA-256 为 `c2f68f87106da4f0cb6d6a90a7aad9644b3c1cb92a3463dbdc8f39449aec3869`；
- 加密 PDF fixture 仍来自 `/private/tmp`，尚未迁移为仓库内可生成 fixture；
- PDFium 基础边界和当前候选动态库证据已较完整，但正式发布工件、复杂 PDF 和完整 CLI 资源生命周期仍未冻结。

阶段 B 的最新进展：

- 已加入不同页面尺寸 fixture，验证 renderer 按页面尺寸与 DPI 输出，不强制方形裁切；
- 已加入 PDFium 缺失动态库门禁，缺库时稳定返回加载错误；
- 当前 PDFium 基础行为门禁已覆盖正常、长文档、不同页面尺寸、损坏、非 PDF、加密、密码恢复和缺库场景。

阶段 B 的最新进展：

- 已增加文本密集的 20 页复杂 fixture；
- 正式 runner 已验证复杂页面逐页渲染和 PNG 落盘；
- PNG 示例 hash 为 `4f6a5f525589a7c7b985136b1e8502a961857ebf35fc47150a906f680ebdab22`；
- PNG 写入错误现在可以从逐页 callback 传播到任务层，符合后续原子输出/失败处理设计。

阶段 B 的最新进展：

- 已接入 `test-data` 中 5 个真实数学教材 PDF；
- 共 867 页，每个文件首页/中间页/末页在 300 DPI 下均成功渲染；
- 真实 PDF 抽样输出约 `2220/2256 × 3141/3142 RGB8`；
- 该结果提升了 PDF renderer 的真实输入证据，但尚不等于全页资源回归或 OCR 质量通过。

阶段 B 的最新阻断：

- 对真实 190 页教材 PDF 执行全页 RSS 采样，166 个样本中 RSS 约从 52 MB 增长到 612 MB，结束前回落到约 373 MB；
- 这与简单合成 PDF 的稳定结果不同，说明真实 PDFium 输入存在显著资源增长；
- 因此“100 页无随页数线性增长”不能冻结，必须先定位 PDFium 文档缓存、页面对象、bitmap 或字体/图片缓存的来源并验证释放策略。

阶段 B 的最新进展：

- 完成第一组对照实验：每页重开/关闭 PDF 文档时峰值 RSS 约 288 MB，相比单文档逐页路径约 612 MB 有改善；
- 但每页重开仍有阶段性增长且性能代价明显，暂不能作为生产冻结方案；
- 下一步需要比较分段重开、显式释放页面/文档对象和缓存策略。

阶段 B 的最新进展：

- 每 20 页重开 PDF 的真实 190 页 RSS 峰值约 331 MB；
- 目前三种策略的观测峰值约为：单文档 612 MB、每页重开 288 MB、每 20 页重开 331 MB；
- 每 20 页重开是较有希望的内存/性能折中候选，但必须完成耗时和多 PDF 复跑后才能冻结页段大小。

同一真实 190 页 PDF 的 release 耗时约为：单文档 6.97 秒、每页重开 7.96 秒、每 20 页重开 6.83 秒；目前每 20 页重开是较有希望的候选，但仍需其他真实 PDF 复跑。

其余 4 个真实 PDF 的每 20 页重开 RSS 峰值约为：九年级上册 318 MB、九年级下册 292 MB、八年级上册 309 MB、八年级下册 575 MB。文档间差异明显，说明必须继续定位内容相关的缓存/资源增长，不能冻结统一页段和资源阈值。

针对八年级下册继续测试：每 5 页约 548 MB、每 10 页约 560 MB、每 20 页约 575 MB、每 40 页约 619 MB。减小分段只能有限改善，未消除增长，下一步应转向 bitmap/页面内容/字体图片缓存定位，而不是继续只调分段大小。

阶段 B 的最新进展：

- 已启用 PDFium 图像缓存限制和渲染前清理开关；
- 高 RSS 文档峰值仍约 576 MB，两个开关没有显著改善；
- 下一步应评估进程隔离/renderer 重启或替代 PDF renderer，而不是继续依赖单个 PDFium cache flag。

阶段 B 的最新进展：

- 已新增独立 `pdfium-worker`，每个 worker 只处理一个页段并将 PNG 写入独立临时目录；
- 八年级下册 178 页按每 20 页分段，共 9 个 worker，全部成功完成；
- 进程隔离基础路径已验证，下一步需要接入主进程 checkpoint、worker 崩溃/超时、取消和结果校验。

当前仓库只有需求、技术方案和 P0 验证文档，尚无：

- Rust workspace 和 crate；
- 可执行 `ocrx` CLI；
- manifest、模型兼容矩阵和锁定文件；
- PDF/模型/输出测试 fixture；
- 自动化测试 runner、benchmark runner 和 CI 配置；
- 安装脚本、Skill 和发布工件。

当前 macOS 已有的技术证据：

- Mac14,3、Apple M2、arm64、16 GB、Metal 可用；
- llama.cpp b10210 的 OvisOCR2 Q4_K_M + BF16 mmproj CPU/Metal 冒烟通过；
- Rust binding 已完成 model/mmproj 加载、图片解析、tokenization 和 MTMD Metal encode；
- `pdfium-render` 0.9.3 + PDFium 7961 已完成 300 DPI RGB PNG 和 100 页顺序渲染冒烟。

这些结果只能作为候选依据，不能替代完整生成、质量、资源、来源和发布验证。

## 3. 工作阶段总览

| 阶段 | 目标 | 产出 | 进入下一阶段条件 |
|---|---|---|---|
| A. 仓库初始化 | 建立可编译、可测试工程 | workspace、crate、工具链锁定、fixtures、runner | clean build、基础测试通过 |
| B. macOS 验证基础设施 | 让已有冒烟和新增验证可重复执行 | 验证脚本、报告格式、环境快照、资源采样器 | 所有测试有固定输入和证据路径 |
| C. Runtime 冻结 | 冻结 Rust/llama.cpp/libmtmd 组合 | runtime lock、兼容矩阵、ADR、完整生成对比 | Rust 与 CLI 生成契约通过 |
| D. PDF/资源冻结 | 冻结渲染、DPI、资源和失败边界 | PDF ADR、RSS/性能报告、fixture 报告 | 长文档和故障门禁通过 |
| E. 模型/profile 冻结 | 冻结来源、profile 和推荐规则 | manifest、hash/license/provenance、质量报告 | 来源合规，质量门禁明确 |
| F. 开发就绪评审 | 判断是否进入正式实现 | 开发就绪报告、阻断清单、批准的实现范围 | 非阻断项已转入 P1/P2 backlog |

## 4. 阶段 A：仓库创建和初始环境配置

### A1. 初始化工程结构

按技术方案建立单 workspace 和最小 crate 边界，优先创建：

```text
Cargo.toml
crates/ocrx-cli
crates/ocrx-core
crates/ocrx-runtime
crates/ocrx-pdf
crates/ocrx-models
crates/ocrx-output
crates/ocrx-ovisocr2
crates/ocrx-skill
assets/manifests/ovisocr2
assets/skills/ocrx
benchmarks/fixtures
scripts
```

crate 初期可以只有接口、错误类型和 fake implementation，但依赖方向必须符合技术方案：CLI → application/core → domain/infra，不能让 CLI/PDF/output 直接解析 OvisOCR2 原始标签。

### A2. 锁定开发工具链

- 建立 Rust toolchain 文件，锁定已验证的 Rust 版本或明确最低版本。
- 固定 Cargo lock 策略、Apple Silicon target 和最低 macOS deployment target。
- 记录 Xcode/Metal toolchain 检测方式，兼容默认 `xcrun` 映射异常的机器。
- 建立统一 `just`/shell 或 Cargo runner，避免验证依赖一次性 `/private/tmp` 命令。
- 为每次验证保存 OS、架构、芯片、内存、工具链、commit、模型 hash 和测试参数。

### A3. 建立基础测试和 fixture

先不依赖真实模型，建立：

- 页码、配置、profile、bbox、路径和错误码单元测试；
- PageDocument、manifest、result、checkpoint、错误 JSON、JSONL golden；
- 正常 PDF、损坏 PDF、加密 PDF、非 PDF、空文档 fixture；
- fake runtime 的多页、失败继续、截断、取消和 resume 测试；
- stdout/stderr 捕获测试；
- 本地 HTTP fixture，用于 Range、ETag、hash、长度和断流测试。

阶段 A 完成标准：`cargo build --workspace`、`cargo test --workspace`、最小 CLI `version/doctor` 可运行；所有基础测试不需要网络、真实模型或人工操作。

## 5. 阶段 B：macOS 验证基础设施

### B1. 环境和构建验证

自动记录并验证：

- macOS 版本、arm64、Apple Silicon、Metal 能力；
- Rust/Cargo/CMake/Xcode/Metal toolchain；
- release 构建是否嵌入 Metal library；
- 最低 macOS 14 的部署目标和启动检查；
- PDFium dylib 架构、依赖、ABI feature、加载路径和 hash。

### B2. PDF 自动验证

自动执行并保存报告：

- 200/250/300 DPI 的页面尺寸、RGB8、PNG hash 和耗时；
- 真实复杂 PDF、合成 100 页 PDF 和大页面；
- 非 PDF、截断、损坏、加密、错误密码、正确密码和空文档；
- 逐页释放 bitmap/image 的 RSS 采样；
- PDFium 缺失、坏库、错误架构和 ABI 不兼容时的稳定错误。

### B3. 资源和中断验证

- 记录每页 RSS、峰值 RSS、峰值 footprint、swap 和耗时；
- 判断 RSS 是否随页数线性增长；
- 注入长生成、单页失败、渲染失败和 SIGINT；
- 检查取消后返回 130、当前页临时文件清理、已完成 checkpoint 保留；
- 检查输出目录中不存在可被误认为完成页的 `.part` 或半写入文件。

阶段 B 完成标准：所有测试可在当前 macOS 重跑，报告包含输入 fixture hash、工具版本、命令、原始日志和结论；可冻结的 PDF renderer、RGB/DPI 默认值、资源阈值和错误映射均有证据。

## 6. 阶段 C：Runtime 对齐与冻结

### C1. 解决版本对齐

明确并锁定以下完整组合：

```text
llama.cpp commit
llama-cpp-2 wrapper version/revision
llama-cpp-sys-2 version/revision
libmtmd ABI/API
Metal build flags
OvisOCR2 model/mmproj compatibility
```

当前 b10210 CLI commit 与 `llama-cpp-sys-2` 内嵌 commit 不一致，不能继续只固定 Cargo semver 版本。若上游无法提供对齐组合，必须建立固定 fork 或最小内部 FFI，并记录 ADR。

### C2. 完整 Rust 生成验证

在 Rust 端完成并自动验证：

- model/mmproj 加载；
- image chunk 编码；
- `eval_chunks`、采样、stop/EOS、max tokens；
- `enable_thinking=false` 和 prompt/chat template；
- 逐 token 进度；
- 单次 SIGINT 取消和安全返回；
- CPU/Metal 输出一致性；
- Rust 输出与同 commit CLI 的结构化对比；
- `non-consecutive token position` warning 的影响。

阶段 C 完成标准：Rust 真实生成路径通过，runtime lock 可复现，模型/`mmproj`/adapter/prompt/schema 兼容矩阵已锁定。否则 runtime 只能标记为候选，不得作为正式实现基线。

## 7. 阶段 D：PDF 与资源方案冻结

基于阶段 B 的报告冻结：

- PDFium 版本、来源、动态库或静态链接方式、ABI feature 和加载策略；
- 默认 DPI、页面缩放、最小/最大像素约束；
- image token 预算和送模图像策略；
- 单页和 100 页资源阈值；
- bitmap、PNG buffer、runtime context 的生命周期；
- 非 PDF、加密、损坏、渲染失败和页面失败映射；
- 默认继续/最终退出 10 的页面失败策略。

注意：这一阶段可以完全在 macOS 完成技术冻结，但不能据此宣称 Windows 动态库、Windows 性能或跨平台发布已通过。

## 8. 阶段 E：模型来源、质量和 profile 冻结

### E1. 模型来源

冻结：

- 官方 OvisOCR2 Safetensors revision；
- GGUF 转换脚本、转换 commit 和 provenance；
- language GGUF 与 mmproj 的准确 revision、大小和 SHA-256；
- model/mmproj 配对规则；
- Apache-2.0 及第三方许可证清单；
- 是否可再分发、下载地址和 manifest 信任字段。

### E2. 质量回归

准备至少 30 页人工真值集及版本化评分器，统一使用相同的 PDF 渲染、adapter、prompt、参数、后处理和随机种子，比较：

- reference/BF16；
- quality/Q8；
- balanced/Q5；
- compact/Q4。

自动输出正文 NED、公式完整率、TEDS、阅读顺序、bbox precision/recall、重复率、截断率、空白率和失败率。人工真值的制作和最终语义判断仍是外部依赖，不能由 macOS 自动化替代。

### E3. profile 规则

依据质量和 RSS 报告冻结：

- 各 profile 的实际量化文件；
- 最低加载内存和推荐内存；
- `auto` 推荐规则；
- 16 GB 默认是 balanced，还是由 quality 接替；
- reference 只能显式选择；
- 未达质量门槛的 profile 不得安装或被自动选择。

## 9. 阶段 F：进入正式开发的条件

### 9.1 必须完成的条件

进入正式实现前，至少必须满足：

1. 仓库已能 clean build/test，基础 crate 和测试 runner 存在。
2. `PageDocument`、manifest、result/checkpoint/JSONL、错误码和 stdout/stderr 契约已形成 golden。
3. macOS PDFium、DPI/像素策略、资源阈值和故障映射已有自动报告。
4. Rust runtime 已完成完整生成验证，或已明确批准先用固定 runtime candidate 开发，并设置后续替换边界。
5. 模型文件、mmproj、revision、hash 和兼容矩阵可复现。
6. 所有尚未冻结的事项均已转为明确的 P1/P2 任务，不再隐含改变公共契约。

### 9.2 当前影响进入正式开发的阻断清单

按当前仓库状态，阻断项如下：

1. **仓库尚未初始化**：没有 workspace、crate、CLI、测试 runner 和可编译入口。
2. **runtime 尚未完成生产级对齐**：Rust binding 与 CLI 使用的 llama.cpp commit 不一致，Rust 尚未完成生成/取消/输出对比。
3. **模型来源尚未冻结**：GGUF 转换 provenance、正式 revision、许可证和再分发策略未完成。
4. **质量基线尚未完成**：没有 30 页真值集，也没有四 profile 的完整评分。
5. **PDF/资源稳定性尚未冻结**：缺少真实复杂长文档 RSS 曲线、最低 macOS 14、release PDFium 加载和完整边界测试。

阶段 B 最新证据进一步确认第 5 项为实质阻断：对高 RSS 真实 PDF 显式释放 bitmap 和 `PdfPage` 后，RSS 仍约 576 MB；问题不能仅归因于 Rust 局部变量生命周期，必须继续定位 PDFium 内部缓存或真实 PDF 内容处理路径。

其中第 1 项是立即开始任何正式实现前必须解决的工程阻断；第 2 至第 5 项阻断“以最终冻结方案进入完整开发”，但不阻断 P1 的契约、fake runtime 和测试基础设施建设。

### 9.3 明确不作为当前 macOS 计划的项目

以下项目不能在当前 macOS 机器冻结，只能保留为后续跨平台门禁：

- Windows x64 CPU；
- Windows CUDA/Vulkan；
- Windows PDFium 动态库和安装包；
- macOS/Windows 正式发布包的跨平台验收；
- 许可证法律最终确认；
- 需要人工判断的 OCR 语义质量最终签字。

## 10. 最终交付物

本计划完成后应产生：

- 可编译的 Rust workspace 和基础测试报告；
- macOS 环境快照和可重复验证脚本；
- runtime lock、PDF ADR、模型来源 ADR、profile/资源兼容矩阵；
- 自动化验证原始日志、fixture 清单和汇总报告；
- 质量回归数据集、评分器和报告；
- 开发就绪评审记录；
- 未解决问题及其 P1/P2 归属。

只有在第 9 节条件明确标记为满足或已批准延期后，才将技术方案状态从 Draft 推进到“允许进入正式开发”。
