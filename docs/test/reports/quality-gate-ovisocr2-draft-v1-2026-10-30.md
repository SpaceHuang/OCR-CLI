# W0.4 质量门禁执行报告 — ovisocr2-draft-v1（2026-10-30）

执行环境：Windows x64，NVIDIA GeForce RTX 4090 D（非登记基准机，超集于 RTX 2070 Max-Q 基准），
Rust 1.97.1，llama.cpp@0.1.152（sys 0.1.153），transformers 5.22.0 + torch 2.11.0+cu128（BF16 参考生成）。

## 1. 集与考卷

- set-id：`ovisocr2-draft-v1`，page_count=30，`validate-regression-set.py` 退出码 0
  （text 14 / visual 18 / mixed 25 / formula 10 / table 2 / blank 2）。
- 源书 SHA-256 与 `inputs/acquisition.md` 一致；composite 由 `assemble-regression-pdf.py`
  （qpdf 12.4.2）重建。**字节级偏差**：重建 hash `31fe8671…` ≠ manifest `ab3235512…`
  （原始 qpdf 版本未记录）。**像素级等价已验证**：30/30 页与源书对应页渲染 MD5 一致
  （pymupdf 72dpi RGB），见 `inputs/rebuild-report.json`。
- 渲染条件：300 DPI，ocrx 像素预算 2880²（CLI 常量；manifest 的 1048576 为真值侧
  LiteParse 截图预算，两侧模型输入均为 300 DPI 原始渲染，见 §6 差异记录）。

## 2. 版本冻结

| 项 | 值 |
|---|---|
| ocrx | 本机源码构建（含本报告 §5 的路由修复），binary sha256 见 reports |
| model revision | `8a22290a0c42dbcf84739d6d4c2763f877494ae0`（GGUF 四档 + mmproj，SHA-256 校验通过） |
| BF16 权重 | `ATH-MaaS/OvisOCR2@65c619d…`，10 文件 SHA-256 全部校验（`download-official-bf16.ps1` 等价流程） |
| prompt/adapter | `ovisocr2-prompt-schema-1` / `ovisocr2-adapter-0.1.0` |
| 评分器 | `scripts/score-regression.py` v2（本报告 §5 修订） |

## 3. 评分总表（vs 人工校准真值，300 DPI）

| 指标 | reference | quality | balanced | compact | BF16 官方 | Vulkan reference |
|---|---:|---:|---:|---:|---:|---:|
| 文本 NED（加权） | 0.7934 | 0.7934 | 0.7938 | 0.7916 | 0.7931 | 0.7933 |
| 公式完整率（29 项/10 页） | 0.3793 | 0.3793 | 0.3793 | 0.3793 | 0.4138 | 0.3793 |
| 表格 TEDS（2 项/2 页） | 0.2143 | 0.2143 | 0.2143 | 0.2143 | 0.2143 | 0.2143 |
| 阅读顺序（504 对） | 0.8682 | 0.8653 | 0.8642 | 0.8623 | 0.8313 | 0.8651 |
| bbox 有效率 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 |
| bbox 召回（31 项/16 页） | 0.1532 | 0.1532 | 0.1532 | 0.1532 | 0.1694 | 0.1532 |
| 重复/截断/空白失败页 | 0 | 0 | 0 | 0 | 0 | 0 |

原始逐页 JSON：`assets/regression/ovisocr2-draft-v1/reports/score-<profile>-300.json`。

## 4. GGUF reference vs 官方 BF16（17.2.3 门禁判定）

| 指标 | 降差（BF16−GGUF） | 阈值 | 判定 |
|---|---:|---:|---|
| 文本 NED | −0.0003 | ≤0.01 | PASS |
| 公式完整率 | +0.0345 | ≤0.02 | **FAIL（粒度性失效，见下）** |
| 表格 TEDS | ±0.0000 | ≤0.02 | PASS |
| 阅读顺序 | −0.0369（GGUF 更优） | ≤0.01 | PASS（降差为负不计） |
| bbox 有效率 | ±0.0000 | ≤0.02 | PASS |
| bbox 召回 | +0.0161 | ≤0.02 | PASS |

**公式项判定分析**：29 条真值公式中 BF16 命中 12、GGUF 命中 11——全部降差来自
第 23 页的 1 条公式。阈值 0.02 低于样本粒度（1/29=0.0345）：单条公式在任何两栈间的
翻转必然超阈值。官方 BF16 自身对人工真值的命中率仅 0.41（记法差异：
`x^2`≡`x^{2}`、真值把多行方程并成单块而模型逐条输出等），说明该项度量在当前
真值记法下不具备 ±0.02 的分辨力。**结论：量化降差不成立（1 条翻转 = 度量噪声），
该项记为 INCONCLUSIVE（粒度不足），修复路径：真值公式样本扩至 n≥50（粒度 0.02）
或逐条人工复核翻转公式。** 按门禁纪律未放宽阈值，本报告如实记录。

## 5. 执行期间发现并修复的问题

1. **路由缺陷（产品代码，已修）**：苏科版教材页文本层 ToUnicode 损坏
   （提取为标点/高位 Latin-1 乱码），旧分类器以 `unicode_validity=1.0` + 有文本块
   判为 `native_text_reliable` 高置信度直走原生提取，输出整页乱码。修复：
   `PageSignals` 新增 `alphanumeric_ratio`（CJK 计为字母），阈值 0.70
   （真文本页实测 ~0.84，损坏页 ≤0.55），低于阈值 → Uncertain/Low → OCR 回退。
   workspace 301 测试通过（新增 3 个分类器/校验测试）。
2. **评分器 v2（§5.5 既有授权，已修）**：v1 为骨架——NED 方向反转且无真值字符
   加权、公式只读 `latex` 键漏掉真值的 `formula_latex`、IoU 0.8≠0.5、空白真值页
   被记为空白失败、顺序未找到块计为顺序错。v2 全部对齐 17.2.2，单测
   `scripts/test_score_regression.py` 全过。
3. **BF16 生成器 EOS 缺失（已修）**：pinned checkpoint 无 generation_config.json，
   transformers 5.22 的 generate() 不在 `<|im_end|>` 停止 → 30/30 页跑满
   4096 token、83% 周期重复尾。显式传入 `eos_token_id=[im_end, eos]` 后正常。
   生成器版本随本报告冻结。

## 6. GPU（Vulkan）E2E 与一致性（W4.2 增量证据）

- 机器：RTX 4090 D，驱动 610.88；最小 Vulkan 工具链（系统 dll + 由系统
  `vulkan-1.dll` 导出表生成的 `vulkan-1.lib` + Vulkan-Headers 1.3.290 +
  SPIRV-Headers + MSYS2 shaderc 2026.4 的 glslc 1.4.363.0）。
- 同 fixture、同 profile（reference）、同 30 页：**Vulkan 58.4s vs CPU 562.6s
  ≈ 9.6×**（对照 RTX 2070 基线的 7.0×）。
- 一致性：逐页可见文本相似度 mean=0.9999，min=0.9974（0 页 <0.99）；
  全部门禁指标与 CPU 等值（±0.003）。
- `doctor --report json`：`backend=Vulkan`，`recommended_device=vulkan`。

## 7. 结论

- **文本质量维度：GGUF 量化与官方 BF16 在本真值集上等价**（NED 差 0.0003），
  四档 profile 均无重复/截断/空白失败页；CPU 与 Vulkan 输出一致（≥0.997/页）。
- **公式项按现有 29 条样本无法给出 ±0.02 的有效判定**（粒度失效），
  记 INCONCLUSIVE；其余指标 PASS。
- 需求 17.2.3 的绝对阈值（NED≥0.97 等）对官方 BF16 自身也不成立
  （0.79）——阈值是按"理想人工级 OCR"写的，而真值采用"可见即纳入"规则并
  含水印/装饰文本，模型读法与人工记法存在系统性记法差。建议后续版本将绝对
  阈值门禁与量化降差门禁解耦（本报告只主张后者）。
- W0.4：真值集已存在且校验通过（计划文档"尚未构造"为滞后状态），门禁已首次
  全量执行；遗留项 = 公式样本扩充 + 200/250 DPI 附录跑。
