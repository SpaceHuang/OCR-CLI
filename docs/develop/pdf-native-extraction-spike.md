# PDFium 原生文字能力验证（v0.2 阶段 0）

状态：binding 能力已在实现级测试中冻结；真实样本 fixture 与发布集合仍属于阶段 6 门禁

当前工程使用 `pdfium-render 0.9.3`、`pdfium_7881`。已有 `PdfiumRenderer` 能够打开文档、查询页数/页面尺寸并渲染 RGB 页面；现有 worker 复用同一 binding。

本阶段先冻结不依赖渲染器的路由契约：`ocrx-pdf::PageSignals` → 纯函数 `classify_page`。空页、图片覆盖率高的扫描页、混合页、重复/几何异常/低 Unicode 有效率页均保守进入 OCR；只有文字覆盖率和字符事实达到阈值的页推荐原生提取。阈值由代码版本控制，当前路由版本为 `pdf-route-v1`（调用方应持有版本字段）。

## 能力结论

| 信号 | 当前状态 | 备注 |
| --- | --- | --- |
| 页面几何、旋转 | 直接取得 | `PdfPage::width/height/rotation` 可用于归一化坐标 |
| 字符 Unicode、文字对象 bbox | 已由 PDFium 7961 真实 fixture probe 验证 | `extract_native_page()` 读取 Unicode 字符并归一化 bbox；测试使用 `/private/tmp/ocrx-p0-20260802/pdfium-chromium-7961/lib` |
| 字体/字号、可见性 | 待用受控 fixture 实测 | PDFium 对不可见渲染模式的暴露需要单独确认 |
| 图片对象 bbox/尺寸 | 已接入实现级覆盖率计算 | PDFium image object bbox 用于 image coverage 与 mixed 判定；专用扫描图 fixture 仍待补 |
| 高 DPI 位图 | 直接取得 | 仅 OCR 路径使用；分析路径不得调用 |
| OCR 模型/运行时 | 不属于分析能力 | classifier 单元测试必须可在无模型环境运行 |

正式合格集和不得直接提取集在阶段 6 冻结；在它们冻结前，不将“真实样本门禁未执行”报告为通过。
> 测试限制：当前 PDFium 7961 binding 在 Rust 测试并行环境下重复打开并遍历多页文档可能触发 native SIGSEGV，因此阶段 0 的自动门禁暂保留单页对象探测；多页/复杂样本需通过独立 `pdfium-check` 进程验证，不能作为同进程单元测试直接遍历。

独立探针复核结果（2026-08-25）：`normal-100-pages.pdf` 成功读取 100 页，抽样输出稳定的 `1400x1800 RGB8`；`varied-page-sizes.pdf` 成功读取 4 页，首个页面输出 `2550x3300 RGB8`。这证明 PDFium 本身可以稳定处理多页和不同页面尺寸，限制只存在于当前同进程测试生命周期。
