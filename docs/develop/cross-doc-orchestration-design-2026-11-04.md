# X1.0 半页设计：跨文档编排与 PDFium 单 owner 跨文档切换协议（冻结稿，2026-11-04）

**方案：** `docs/plan/scan-throughput-phase2-development-plan.md` §4 X1.0
**性质：** 本文档是 X1.1/X1.2 实施的**前置评审门禁**（v1.2 状态行重申）；PDFium 协议五点按 BLK-2 冻结，实施与本冻结稿偏离即为门禁违例。

## 1. CLI 面与编排语义

- 命令：`ocrx scan <input>... [-o <output-root>]`，输入可为 PDF 文件或目录（目录展开为其中 `*.pdf`，按文件名排序，不递归）；按 canonical path 去重；任一输入不存在/无法打开 → 用法错误 **exit 2**（逐个校验，集合为空同样 exit 2）。
- **多输入（展开后 >1）时 `-o` 必填**（缺省 exit 2）；单输入完全维持现行为（默认 `<input-dir>/<stem>-ocr`）。
- 编排：文档级 FIFO 串行——下一文档的窗口只从队首文档取页；**窗口永不跨文档**（每文档窗口组成、机制 A 原子提交、checkpoint 兼容键、resume 语义与单文档扫描逐字节一致）。
- 进程级不变式延续：**唯一推理 actor**（`LlamaMtmdRuntime` worker 线程跨文档复用）+ **唯一 PDFium owner 线程**（整调用一次 spawn）。
- 模型校验+加载：懒共享槽——首个需要 OCR 的文档触发 `resolve_and_prepare_model` + load（每调用恰好一次），后续文档复用（BLK-1 裁决后的固定税摊薄路径）；纯 native 文档不触发。
- 逐文档进度：文档边界行 `[doc i/N <name>]` 进入 reporter 输出；页级心跳沿用。
- 取消 = 整调用：共享一个 `CancellationToken`；在飞窗口零提交（机制 A 自然成立），已提交文档保留。
- 退出码聚合：全成功 → 0（任一文档页级 partial → 10）；任一文档级失败（不可读 PDF 等）→ 该文档记为 skipped，**继续其余文档**，最终退出码取各文档退出码的最高严重级（3 > 10 > 0）；汇总经 stderr/`scan-summary.json` 报告。owner 中毒/超时/通道断开仍为**整调用 fail-loud**（exit 3），禁止续跑。

## 2. 输出布局（X1.3）

- 多输入：`<output-root>/<doc-stem>-<hash8>/`（`hash8` = 输入 canonical path SHA-256 前 8 hex；stem 碰撞由 hash 天然消解；目录已含 `document.md`/`result.json` 时按单文档既有 `--resume`/`--overwrite` 规则逐文档处理）。
- 多输入附加：`<output-root>/scan-summary.json`（schema_version、inputs、逐文档 status/output_dir/elapsed_ms/pages 计数、整体 elapsed）。
- 单输入：输出布局零变化（gate ②）。

## 3. PDFium 协议（BLK-2 冻结，五点）

1. **整调用唯一 owner 线程**。每文档各自 `spawn` 被禁止（pdfium binding 进程全局 `OnceLock` 绑定，跨线程复用是硬约束）；主线程兜底同样禁止。编排器持有单个 `Rc<RefCell<PdfiumOwnerHandle>>` 贯穿全部文档。
2. **新增 `SwitchDocument { input: PathBuf }` 请求**。owner 据此原子切换 executor 的输入文档（渲染配置 `RenderOptions{dpi}` 与密码为调用级，切换不改）。
3. **切换仅允许在窗口调度边界**（编排器串行化保证：上一文档的 scan 完整返回后、下一文档首个 PDFium 请求前）。切换时 owner 清空预取缓存并重置 `next_prefetch = 1`——第二文档零命中第一文档的页缓存/预取（X1.4③ 断言点）。
4. **文档级错误作为值返回**：`SwitchDocument`/`PageCount` 的"不可读 PDF/加密/损坏"以 `Err` **值**经通道返回，编排器记 skipped 并继续下一文档；owner 超时、悬挂、通道断开维持既有 **fail-loud 中毒语义**（整调用终止），**禁止另起 owner 或主线程接管**。
5. **单输入路径零行为变化**：单输入不发送 `SwitchDocument`（owner 以首文档 spawn，与现状一致）；现有全部 CLI/owner 测试必须原样通过。

## 4. 模型共享槽

```
enum ModelSlot { PerCall, Shared(Option<(LlamaMtmdRuntime, String /*profile*/, String /*device*/)>) }
```

- 单输入 = `PerCall`（run_scan 现行为：scan 内 resolve+load，随 scope 丢弃）。
- 多输入 = `Shared(slot)`：scan_document 需要时从槽取用/首次填充（resolve + `load_runtime_with_fallback`），用毕放回——运行时对象跨文档存活，加载/校验每调用一次。
- `ocrx-output::ScanService` 新增 `run_with_model(model: &mut M, ...)` 入口（把窗口循环体从 loader-load 中解耦）；既有 `run_with_checkpoints*` 包装不变（load 后调用同一循环体）——共享 actor 复用的落点。

## 5. 安全验收映射（X1.4③，四项断言的测试落点）

| 断言 | 测试落点 |
|---|---|
| ① 两文档顺序处理全程同一 owner 线程 | owner 协议测试：fake executor 记录线程 id，SwitchDocument 前后断言同一线程 |
| ② 第二文档零命中第一文档页缓存/预取 | owner 协议测试：Switch 后首渲染必有 executor 调用（缓存清空）；沿用 `inject_render_failure` seam 的编排级扩展断言 |
| ③ 不可读第二文档作为文档级错误跳过且后续继续 | 编排测试：[好, 坏, 好] → 好两份产物齐全，坏文档 skipped 值，退出码非零 |
| ④ owner 超时注入 → 整调用终止、无第二 owner 线程、无主线程 PDFium 调用 | 既有超时测试 + 编排级：Switch 后超时注入 → poison 传播，进程级 fail-loud |

## 6. 实施切分（TDD 序）

1. X1.1 `parse_scan_args` 多输入扩展（纯解析 + 展开/去重，单测先行）。
2. owner `SwitchDocument` 协议（§3-2/3/5，owner 协议单测先行）。
3. `ocrx-output` `run_with_model` 解耦（等价性单测先行）。
4. `scan_document` 提取 + `run_scan_multi` 编排器 + X1.3 布局/汇总（X1.4①–③ 单测 + fault-injection 集成测试）。
5. 门禁：多文档内容等价（各文档 == 单独扫描逐字节）、单输入不回退（现有全套 + 金集复测）、`md-small-20` ≥1.5×。
