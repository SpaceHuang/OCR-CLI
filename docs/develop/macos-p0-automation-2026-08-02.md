# macOS P0 自动验证执行记录

日期：2026-08-02  
范围：阶段 B 验证基础设施与现有 P0 工件复核

## 已建立

- `scripts/macos-environment.sh`：记录 macOS、架构、芯片、内存、Metal、Rust、CMake、Xcode 和 Metal 查找状态。
- `scripts/bootstrap-check.sh`：执行格式、workspace build、workspace test、`version` 和 bootstrap `doctor`。
- `scripts/macos-p0-report.sh`：检查 P0 临时工件、计算 PDF fixture hash，并在存在 runner 时重跑 PDFium 单页/100 页渲染。
- `scripts/generate-pdf-fixtures.py` + `scripts/macos-pdf-fixture-check.sh`：在仓库内生成确定性的正常、100 页、截断损坏和非 PDF fixture，并验证正式 `ocrx-pdf` runner。
- `scripts/macos-pdf-rss-check.sh`：延迟逐页渲染并用 macOS `ps` 采样 RSS。
- `scripts/macos-pdfium-binary-check.sh`：检查 PDFium dylib 的 arm64 架构、依赖和 SHA-256。
- `scripts/macos-encrypted-pdf-check.sh`：验证无密码失败、正确密码打开。
- `scripts/macos-real-pdf-check.sh`：对仓库 `test-data` 中每个真实 PDF 的首页、中间页和末页进行正式渲染抽样。
- `scripts/macos-pdf-worker-check.sh`：按 20 页分段启动独立 `pdfium-worker`，验证进程隔离渲染和 PNG 交付。

## 本机执行结果

本轮复跑（2026-08-02）确认：`./scripts/bootstrap-check.sh` 通过；workspace build/test 共通过 3 个 crate 单元测试及全部 doc tests；`version` 和 bootstrap `doctor` 均通过。`macos-pdf-fixture-check.sh`、`macos-pdf-worker-check.sh`、`macos-pdfium-binary-check.sh`、`macos-encrypted-pdf-check.sh` 均通过。

| 检查 | 结果 |
|---|---|
| 环境快照 | 通过：macOS 26.5.2、arm64、Apple M2、16 GB、Metal Supported |
| workspace bootstrap | 通过：build/test、version、bootstrap doctor |
| Rust 格式检查 | 通过 |
| workspace build/test | 通过 |
| PDF fixture 复核 | 已发现 `/private/tmp/ocrx-p0-20260802/` 历史工件 |
| 新鲜 PDFium runner | 通过：正式 `ocrx-pdf` crate + 仓库 fixture |
| RSS 曲线 | 未冻结；当前环境资源采样能力和 CLI 生命周期尚未接入 |
| 正式 `ocrx-pdf` crate 编译 | 通过：`pdfium-render 0.9.3`、`image 0.25.10` |
| 正式 crate 单页渲染 | 通过：300 DPI，`1400x1800 RGB8` |
| 正式 crate 100 页渲染 | 通过：100 页，release 约 1.89 秒渲染时间 |
| `/usr/bin/time -l` RSS | 阻断：沙箱中 `sysctl kern.clockrate` 权限不足，未返回 RSS |
| 仓库 PDF fixture | 通过：生成正常/100 页/不同页面尺寸/复杂页面/截断/非 PDF，并完成正式 runner 边界验证 |
| PNG 落盘 | 已加入正式 runner 门禁，验证 PNG 类型和 hash |
| 真实 PDF 抽样 | 通过：5 个真实教材 PDF，共 867 页，首页/中间页/末页均成功渲染 |
| PDF worker 隔离 | 通过候选：八年级下册 178 页分成 9 个 worker 进程，全部完成 PNG 输出 |
| PDFium 缺失库 | 已加入正式 runner 门禁，待汇总报告中记录 |
| 不同页面尺寸 | 通过：300 DPI 输出尺寸随页面尺寸变化，未强制方形裁切 |
| RSS 采样 runner | 已建立；真实长文档采样发现明显增长，资源稳定性阻断 |
| PDFium dylib 检查 | 通过候选：arm64、依赖可解析，SHA-256=`c2f68f87106da4f0cb6d6a90a7aad9644b3c1cb92a3463dbdc8f39449aec3869` |
| 加密 PDF runner | 通过：无密码返回 `PasswordError`，正确密码可打开 |

## 阶段 B 当前结论

仓库已经有可重复的环境和证据入口，且 PDFium 已正式接入 `ocrx-pdf` crate；正式 crate 已使用仓库 fixture 完成单页、100 页、不同页面尺寸、复杂页面、错误输入、PNG 落盘和 worker 隔离验证。因此本阶段可以冻结“PDFium crate 接入方式、RGB8/300 DPI 基础契约和单进程 binding 生命周期约束”。输入 PDFium dylib 和模型文件仍有历史工件依赖，不能冻结最终发布工件、资源阈值或完整 PDF 稳定性。

将 PDFium 接入正式 `ocrx-pdf` crate 后，首次运行验证发现 `pdfium-render` 的 binding 是进程级单例：同一进程重复初始化会返回 `PdfiumLibraryBindingsAlreadyInitialized`。这已记录为 renderer 生命周期约束：扫描任务必须复用一次初始化的 PDFium binding，不能在每页或每个 API 调用中重新绑定。

随后已将无第三方 Python 依赖的确定性 PDF fixture 生成器和 runner 纳入仓库。当前 fixture 已验证：单页正常 PDF、100 页正常 PDF、不同页面尺寸、截断损坏 PDF、非 PDF 文件；两类错误均稳定映射到底层 `FormatError`。加密/密码恢复仍使用历史 P0 fixture `/private/tmp/ocrx-p0-20260802/encrypted.pdf`，生成脚本记录密码为 `ocrx-test`，尚未迁移为仓库 fixture。

已在允许读取测试进程的 macOS 环境下执行 RSS 采样：100 页、每页延迟 20 ms，共取得 47 个样本；初始化后 RSS 约从 15.9 MB 上升至约 46.7 MB，后段样本在约 46.6–46.8 MB 之间小幅波动。该结果支持“渲染 bitmap 未按页持续累积”的候选判断，但尚不足以冻结最终 CLI 资源阈值，因为测试仍是简单 PDF，且尚未包含模型加载、推理、PNG 裁图和 checkpoint 生命周期。

真实长文档采样改变了结论：对 `test-data` 中 190 页的数学教材 PDF 全页顺序渲染、每页延迟 10 ms，共取得 166 个 RSS 样本，RSS 从约 52 MB 逐步升至约 612 MB，期间持续增长，进程结束前回落到约 373 MB。该结果表明真实 PDFium 输入存在显著的进程内资源增长，当前不能宣称“100 页处理无随页数线性增长”，也不能冻结当前 renderer 的资源阈值。

已完成第一组定位对照：保持同一个 PDFium binding，但每页重新打开并关闭 PDF 文档。该路径峰值 RSS 约 288 MB，低于单文档逐页路径的约 612 MB，但仍有阶段性增长，且重复打开带来明显性能代价。因此“每页重开文档”只能作为候选缓解策略，不能直接冻结为生产方案。

第二组定位对照采用每 20 页重开 PDF 文档：峰值 RSS 约 331 MB，低于单文档逐页路径约 612 MB，高于每页重开约 288 MB。它提供了更合理的内存/重复解析折中候选，但尚未完成耗时测量、其他真实 PDF 复跑和模型/CLI 生命周期验证，暂不冻结页段大小。

同一 190 页真实 PDF 的 release 耗时对照：单文档逐页约 6.97 秒、每页重开约 7.96 秒、每 20 页重开约 6.83 秒。当前数据表明每 20 页重开同时降低峰值 RSS 且未增加该样本耗时，但仍需要其余真实 PDF 和更完整的采样报告确认。

其余 4 个真实 PDF 使用每 20 页重开策略复跑，RSS 汇总如下（启动阶段的 `32 KB` 样本为 macOS 进程信息读取初始化值，不作为有效工作集）：

| 文件 | 页数 | 采样数 | 峰值 RSS |
|---|---:|---:|---:|
| 九年级上册 | 158 | 124 | 318,320 KB |
| 九年级下册 | 162 | 125 | 291,616 KB |
| 八年级上册 | 179 | 144 | 308,992 KB |
| 八年级下册 | 178 | 167 | 575,120 KB |

多文档结果显示每 20 页重开通常把峰值控制在约 292–318 MB，但八年级下册仍达到约 575 MB，说明文档内容差异会显著影响资源增长；因此不能仅凭一个页段大小冻结通用资源阈值。

针对 RSS 最高的八年级下册，分段大小对照结果为：每 5 页约 548 MB、每 10 页约 560 MB、每 20 页约 575 MB、每 40 页约 619 MB。分段变小只能有限降低峰值，未消除内容相关的增长，说明根因不只是文档对象重开频率。该文档每页重开路径已能完整渲染 178 页，但仍需独立采样报告确认其资源峰值。

代码已增加显式 `drop(Pdfium bitmap)`、`drop(PdfPage)`，并在回调前完成 bitmap 转换和释放；八年级下册每 20 页重开复跑峰值仍约 576 MB。由此可排除“仅因当前 Rust 局部变量未及时释放”的简单解释，资源增长更可能来自 PDFium 内部缓存或真实 PDF 内容处理路径。

进一步启用 `pdfium-render` 暴露的 `limit_render_image_cache_size(true)` 和 `clear_before_rendering(true)` 后，八年级下册每 20 页重开峰值仍约 576 MB。当前两个开关没有显著改善该真实 PDF 的 RSS，不能作为单独的解决方案。

已冻结进程隔离方案：每个 `pdfium-worker` 只负责一个连续半开页段 `[start,end)`，将 PNG 写入独立临时目录，单个文件先写 `.part` 后原子 rename，worker 退出后主进程校验精确页数、文件完整性和无残留临时文件，再提交该页段。对八年级下册 178 页以每 20 页分段执行，共启动 9 个 worker，全部成功输出；非法空页段也稳定失败。主进程 checkpoint/IPC/崩溃恢复尚未接入，但不再影响“独立 worker 作为资源隔离方案”的技术选型冻结。

不同页面尺寸 fixture 已通过：336x432 pt 页面输出 1400x1800，612x792 pt 页面输出 2550x3300，证明当前 renderer 按页面尺寸和 DPI 计算输出，不强制裁成固定方形。

复杂页面 fixture 和 PNG 落盘门禁已加入：复杂 20 页 PDF 可逐页渲染，正式 runner 可将 RGB8 图像保存为 PNG，并检查 PNG 类型与 SHA-256。该 fixture 仍是程序生成的文本密集页面，不等同于真实扫描书籍或含嵌入图片的生产 PDF。

本轮复杂 fixture 执行通过：20 页文本密集 PDF 全部渲染成功；PNG 落盘成功，样例 `page-0001.png` SHA-256 为 `4f6a5f525589a7c7b985136b1e8502a961857ebf35fc47150a906f680ebdab22`。此外，renderer callback 已改为可返回错误，PNG 写入失败会中止当前任务并向上层传播。

已使用仓库 `test-data` 中的 5 个真实数学教材 PDF 进行抽样：总计 867 页、单文件 158–190 页；每个文件的首页、中间页和末页均在 300 DPI 下成功渲染，输出约 `2220/2256 × 3141/3142 RGB8`。这证明真实页面尺寸和真实 PDF 内容的基础渲染路径可行，但仍不是全页资源回归或 OCR 质量验证。

独立 worker 已确定为真实复杂 PDF 的资源隔离方案：通过 worker 进程退出回收 PDFium 文档、字体和图片缓存，主进程不再依赖单进程 RSS 不增长来保证长文档稳定性。仍需在正式 CLI 接入后验证 worker 超时、崩溃、取消、checkpoint 提交和临时目录清理；这些属于实现级故障门禁，不再是 PDFium 选型阻断。
