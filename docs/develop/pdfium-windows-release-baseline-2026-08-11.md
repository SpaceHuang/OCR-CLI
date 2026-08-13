# PDFium Windows 发布基线

日期：2026-08-11  
范围：Windows 平台 PDFium 工件来源与发布基线。对应计划 `docs/develop/windows-platform-development-plan.md` 待决策项 2 的冻结。  
对应 macOS 基线：`docs/develop/pdfium-macos-release-baseline-2026-08-02.md`

## 1. 结论

Windows 采用与 macOS **同一来源、同一 revision** 的 PDFium 工件，不自编译：

- 来源项目：`bblanchon/pdfium-binaries`（官方 PDFium 源码 + 社区 CI 构建打包）
- release 标签：`chromium/7961`
- 资产：`pdfium-win-x64.tgz`
- PDFium 版本：`152.0.7961.0`（`VERSION` 文件：`MAJOR=152 MINOR=0 BUILD=7961 PATCH=0`）
- 运行时工件：`bin/pdfium.dll`（PE32+ / x64/AMD64）
- crate：`pdfium-render = 0.9.3`，feature `pdfium_7881`、`thread_safe`、`image_025`（与 macOS 一致）
- 许可证：PDFium BSD-3-Clause + 打包脚本 MIT 式许可 + 全套第三方许可，**可再分发**

选定社区构建而非自编译的理由：macOS 已冻结接受同一项目的 `chromium/7961` 工件；Windows 用同源同 revision 可避免第二条可追溯性链与额外 Chromium 构建工具链（depot_tools/gclient）维护成本，是最低风险选择。Google 官方不单独发布 PDFium 预编译件，bblanchon/pdfium-binaries 是事实标准来源。

## 2. 工件指纹

| 项目 | 值 |
|---|---|
| 资产文件 | `pdfium-win-x64.tgz`（3.6 MB 压缩） |
| 资产下载量 | 11180（chromium/7961 release，2026-07-20 发布） |
| 归档 SHA-256 | `88276459349b291c41f10422dad0210f007c04d919c8fa56472b6b7c6406adf4` |
| 运行时文件 | `bin/pdfium.dll`（7220736 字节 / 6.9 MB） |
| `pdfium.dll` SHA-256 | `d3d9f4b7c9dabe3363f30779c5c3c715c47332749fa590e4b4a2b8b6780cb1c4` |
| PE machine | `0x8664`（x64/AMD64），`PE32+` |
| 来源证明 | 同 release 附带 `pdfium-attestation.json` |

## 3. 构建标志（`args.gn`）

```text
is_component_build = false
is_debug = false
pdf_enable_v8 = false
pdf_enable_xfa = false
pdf_is_standalone = true
pdf_use_partition_alloc = false
target_cpu = "x64"
target_os = "win"
treat_warnings_as_errors = false
```

干净 release 构建，关闭 V8 与 XFA（OCR 仅需逐页渲染，不需要 JS/XFA），与 macOS 同项目产物一致。

## 4. 归档内部结构

```text
LICENSE                      # 打包脚本许可（Benoit Blanchon，MIT 式）
VERSION                      # 152.0.7961.0
args.gn                      # 构建标志
PDFiumConfig.cmake           # CMake 配置（构建期）
bin/pdfium.dll               # 运行时动态库 ★
lib/pdfium.dll.lib           # 导入库（构建期，运行时不需要）
include/                     # 头文件（构建期）
licenses/                    # 第三方许可集合
  pdfium.txt                 # PDFium BSD-3-Clause
  abseil.txt agg23.txt fast_float.txt freetype.txt
  icu.txt lcms.txt libjpeg_turbo.{ijg,md} libopenjpeg.txt
  libpng.txt libtiff.txt llvm-libc.txt simdutf.txt zlib.txt
```

★ 标记为运行时必需文件。

## 5. 与 macOS 基线的差异（实现需处理）

| 维度 | macOS | Windows | 处理方式 |
|---|---|---|---|
| 动态库路径 | `lib/libpdfium.dylib` | `bin/pdfium.dll` | 运行时加载路径不同；`pdfium-render` 的 `bind_to_library` 接受任意路径，无契约影响。W2.1 固定 Windows 运行时加载路径与发布包内相对目录 |
| 导入库 | 无 | `lib/pdfium.dll.lib` | `pdfium-render` 走动态加载，运行时不需 `.lib`；不进入用户运行时归档 |
| 许可证布局 | 发布包重排为 `licenses/PDFium.LICENSE` + `licenses/pdfium/*.txt` | 上游为顶层 `LICENSE` + 扁平 `licenses/*.txt` | Windows 发布包纳入同一套许可集合，具体布局由 W5.3 打包脚本与 macOS 对齐 |
| ABI feature | `pdfium_7881` 绑定 7961（已验证向后兼容） | 同 feature | W0.2/W2.1 在 Windows 验证 `pdfium_7881` 绑定 7961 DLL |

## 6. 运行时加载约束

- `pdfium.dll` 必须随发布包分发（PDFium 是扫描必需依赖，缺失则连一页都无法渲染）；与 macOS 随包分发 dylib 的策略一致（对应计划待决策项 4 的已冻结结论）。
- 运行时从发布包内固定目录加载，不得从系统 PATH、`System32` 或用户可控路径隐式搜索；启动时把“缺失 DLL / 架构不匹配 / 依赖无法加载”映射为稳定初始化错误（退出码 6）。
- 不得把用户输入、PDF 内容或模型输出参与 DLL 路径拼接。
- `Pdfium` binding 进程内只初始化一次并由扫描任务复用（进程级单例约束，与 macOS 一致）。

## 7. 许可证与再分发

- `licenses/pdfium.txt`：PDFium BSD-3-Clause（Copyright 2014 The PDFium Authors）——允许再分发，需保留版权与许可声明。
- 顶层 `LICENSE`：打包脚本 MIT 式许可——允许再分发。
- 第三方组件（abseil、freetype、icu、libpng、libjpeg_turbo、libopenjpeg、libtiff、zlib、simdutf、lcms、fast_float、llvm-libc、agg23）许可均随归档提供。
- 结论：**可随发布包再分发**；发布归档必须包含上述许可集合，并由打包检查脚本验证文件存在与 hash。

## 8. ABI 风险记录

1. PDFium 无跨 revision 稳定 ABI 承诺；升级 revision 必须重新执行 binding、渲染、错误分类与 hash 门禁（与 macOS 基线一致）。
2. crate feature 名为 `pdfium_7881`、实际动态库为 7961，是已验证的向后 ABI 兼容使用方式；Windows 必须在 W0.2/W2.1 复现该绑定验证，未验证前不得跨大版本复用。
3. `thread_safe` 只解决 binding 声明的线程安全边界，不改变进程级单例初始化约束。
4. 发布包必须锁定 crate 版本、PDFium revision、`pdfium.dll` SHA-256 与许可集合；任一变化产生新的发布兼容矩阵条目。

## 9. 尚待 W0.2/W2.1 完成的验证

本基线只冻结“来源、revision、版本、hash、架构、许可与再分发性”。下列仍需在 Windows 实机上验证，不改变选型：

- `pdfium-render 0.9.3` + `pdfium_7881` feature 在 Windows x64 上绑定 `bin/pdfium.dll` 成功。
- 300 DPI RGB8 渲染、不同页面尺寸、加密/密码恢复、损坏/非 PDF 分类、PNG 落盘，结果与 macOS fixture 在像素与 hash 层面可比。
- DLL 缺失、错误架构、加载路径逃逸的稳定错误分类。
- 100 页与真实复杂 PDF 的资源回归（worker 进程隔离）。
