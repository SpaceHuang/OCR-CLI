# PDFium macOS 发布基线

日期：2026-08-02  
范围：v0.1 macOS Apple Silicon 开发/发布基线，不包含 Windows。

## 结论

v0.1 macOS 采用：

- `pdfium-render = 0.9.3`
- PDFium binary revision `chromium/7961`
- macOS arm64 `libpdfium.dylib`
- crate feature `pdfium_7881`、`thread_safe`、`image_025`
- `image` crate 显式启用 `png` feature
- 运行时从发布包内固定目录加载 PDFium，不从系统路径、Homebrew 或用户可控路径隐式搜索

当前已验证的候选动态库：

| 项目 | 值 |
|---|---|
| 文件 | `lib/libpdfium.dylib` |
| Mach-O 架构 | arm64 |
| PDFium 版本 | `152.0.7961.0` |
| 已记录 SHA-256 | `c2f68f87106da4f0cb6d6a90a7aad9644b3c1cb92a3463dbdc8f39449aec3869` |
| 上游压缩包 SHA-256 | `1193a771e0bd934530afa3df73a0d44551d8f4078442e290054e6dd38ded960f` |
| 装载方式 | `Pdfium::bind_to_library()`，进程内只初始化一次 |

## 发布包内容清单

PDFium 发布包必须至少包含：

```text
lib/libpdfium.dylib
licenses/PDFium.LICENSE
licenses/pdfium/abseil.txt
licenses/pdfium/agg23.txt
licenses/pdfium/fast_float.txt
licenses/pdfium/freetype.txt
licenses/pdfium/icu.txt
licenses/pdfium/lcms.txt
licenses/pdfium/libopenjpeg.txt
licenses/pdfium/libpng.txt
licenses/pdfium/libtiff.txt
licenses/pdfium/llvm-libc.txt
licenses/pdfium/pdfium.txt
licenses/pdfium/simdutf.txt
licenses/pdfium/zlib.txt
```

开发验证工件 `pdfium-chromium-7961` 已包含主 `LICENSE` 和上述第三方许可证文件。正式发布前应将它们复制到发布归档，并由打包检查脚本验证文件存在和 hash；头文件只用于构建，不进入用户运行时归档。

## 动态库依赖与装载约束

当前 `otool -L` 检查结果：

- 自身 install name：`./libpdfium.dylib`
- 依赖仅为系统 `AppKit`、`CoreGraphics`、`CoreFoundation`、`Foundation` 和 `/usr/lib/libSystem.B.dylib`
- 未发现 Homebrew、构建机绝对路径或其他第三方动态库依赖

发布实现必须将 CLI 与 PDFium 的相对目录关系固定下来，并在启动时把“缺少动态库/架构不匹配/依赖无法加载”映射为稳定初始化错误。不得把用户输入、PDF 内容或模型输出参与动态库路径拼接。

## ABI 风险记录

`pdfium-render 0.9.3` 使用 `pdfium_7881` ABI feature 成功绑定 PDFium 7961，并已完成页面打开、300 DPI RGB8 渲染、PNG 落盘、加密/损坏错误分类和真实 PDF 抽样。该事实足以作为当前 macOS 开发基线，但不等于 PDFium ABI 的长期兼容承诺：

1. PDFium 本身没有稳定的跨 revision ABI 承诺；升级 Chromium/PDFium revision 必须重新执行 binding、渲染、错误分类和 hash 门禁。
2. 当前 crate feature 名称是 `pdfium_7881`，而实际动态库为 7961；这是已验证的向后 ABI 兼容使用方式，不应在未验证时继续跨大版本复用。
3. `thread_safe` 只解决 binding 声明的线程安全边界，不改变进程级单例初始化约束；扫描任务仍必须复用单个 binding。
4. 发布包必须锁定 crate 版本、PDFium revision、动态库 SHA-256 和许可证集合；任一变化都产生新的发布兼容矩阵条目。

## 尚未完成但不再是方案选型问题

- macOS 14 实机验证：属于发布阶段的最低版本支持承诺，不阻断开发。
- 真实复杂 PDF 的资源策略：属于性能/稳定性验证，不改变 PDFium 基础选型。
- 正式归档打包和签名：属于发布工程实现，应在发布阶段执行。

因此，PDFium macOS 的基础选型、依赖检查、许可证清单和 ABI 风险记录已经可以冻结；后续只需把本基线落实到正式打包脚本和发布验收。
