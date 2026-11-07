# OmniDocBench 基线（ocrx / OvisOCR2）

外部质量基线：官方 [OmniDocBench](https://github.com/opendatalab/OmniDocBench) v1.6 主线
（1,651 页，dataset revision `aa1ee96d106dbe53d0ae59474d75c6e6d9b53fec`）上的 ocrx
端到端扫描评分，用于 v2 推理平台重构前后的对照。仅作 baseline，不提交榜单。

## 布局

| 目录 | 内容 | git |
|---|---|---|
| `dataset/` | HF 数据集原样（OmniDocBench.json 42MB + images/ 1.4GB） | 忽略 |
| `eval/` | 官方评测仓库 clone（HEAD `ea2d138c`） | 忽略 |
| `pdfs/` | 单页 PDF（`scripts/build-omnidocbench-pdfs.py` 产物，1,651 个）+ `mapping.json` + `build-report.json` | 忽略 |
| `scans/<profile>/` | ocrx 多文档扫描输出（compact/balanced/quality/reference） | 忽略 |
| `predictions/<profile>/` | `<image stem>.md` 官方评测输入 + `export-report.json` | 忽略 |
| `reports/` | pdf_validation.py 原始输出（每档 + zh/en 切分） | **提交** |
| `manifest.json` | 版本冻结与复现参数 | **提交** |

## 复现步骤

```bash
# 0) 环境：Docker Desktop（官方评测镜像），ocrx（Vulkan 构建），本仓库 scripts/
export HF_ENDPOINT=https://hf-mirror.com   # huggingface.co 直连不通时
venv=".venv/Scripts/python.exe"            # 需 huggingface_hub img2pdf Pillow

# 1) 数据集 + 评测仓库
$venv -m huggingface_hub.commands.hf_cli download ... # 见 manifest.json 复现段
git clone --depth 1 https://github.com/opendatalab/OmniDocBench eval

# 2) 图片 -> 单页 PDF（MediaBox = px*72/300，300 DPI 渲染像素级保真）
$venv ../../scripts/build-omnidocbench-pdfs.py \
    --gt dataset/OmniDocBench.json --images dataset/images --out pdfs

# 3) 全量扫描 ×4 档（每档 20-30 分钟，GPU）
for p in balanced compact quality reference; do
  ocrx scan pdfs -o "scans/$p" --model ovisocr2 --model-profile "$p" \
      --dpi 300 --non-interactive --progress off --log-level warn
done

# 4) 导出官方评测预测（<stem>.md；剥 bbox 图片链接与 <think> 泄漏）
$venv ../../scripts/export-omnidocbench-predictions.py \
    --scans scans/balanced --mapping pdfs/mapping.json --out predictions/balanced
# ……其余三档同理

# 5) 官方评测（Docker 镜像含 py3.10 + TeX Live 2025 + ImageMagick 7.1.1 + Ghostscript 9.55）
# 注意：ghcr.io 直连在此网络环境下层数据下载 0 字节（manifest 可读但 blob 卡死）；
# 用南大镜像源拉取（2026-11-06 实测 13.65GB 约 90 秒完成），再 tag 回标准名：
docker pull ghcr.nju.edu.cn/zeng-weijun/omnidocbench-eval:repro-ubuntu2204
docker tag ghcr.nju.edu.cn/zeng-weijun/omnidocbench-eval:repro-ubuntu2204 \
    ghcr.io/zeng-weijun/omnidocbench-eval:repro-ubuntu2204

docker run --rm --entrypoint bash \
  -v "$(pwd)/dataset/OmniDocBench.json:/workspace/gt/OmniDocBench.json:ro" \
  -v "$(pwd)/predictions/balanced:/workspace/data_md/predictions:ro" \
  -v "$(pwd)/reports:/workspace/result" \
  ghcr.io/zeng-weijun/omnidocbench-eval:repro-ubuntu2204 \
  -c 'cat > configs/custom.yaml << "EOF"
<见 manifest.json 中的逐档 yaml>
EOF
python pdf_validation.py --config configs/custom.yaml'
# 实际执行用封装脚本（自动写 yaml + 挂载 + 日志落盘 reports/）：
# .venv/Scripts/python.exe ../../scripts/run-omnidocbench-eval.py \
#     --benchmark-root . --gt dataset/OmniDocBench.json \
#     --predictions predictions/balanced --name omnidocbench-balanced-all --workers 8
```

## 已知管线缺陷（基线中发现）

- **`<think>\n\n</think>` 泄漏**：OvisOCR2 空推理块偶发进入合并 markdown
  （冒烟 3 页中 1 页）。导出脚本确定性剥离并计数
  （`export-report.json: think_blocks_stripped_total`）。
  **产品侧已修复**（质量方案 Q3，`strip_think_blocks`，产物层单源剥离；
  result schema 3 新增 `pages.think_blocks_stripped_total`），导出脚本剥离
  保留为评测对账的第二实现。

## 复评纪律（质量方案 Q7.1）

1. **每次复评绑定 manifest 冻结**：二进制 rev/SHA、wrapper 版本（llama.cpp
   pins）、模型 revision、数据集 revision——改任何一项都构成新 manifest，
   不得与旧轮结果直接对比。
2. **质量数据必须采自门禁通过的构建**：`fmt --check` + `test --workspace` +
   `clippy -D warnings` 全绿后才开始扫描（一期教训）。
3. **里程碑用全量 balanced（~90 min）；终评用四档**。语料级实验只用于
   迭代（213 页语料 ~11.6 min/轮，`assets/regression/omnidocbench-degeneration-v0.1/`
   + `degeneration-v0.1/eval-subset.sh`），其结果必须换算为官方同粒度
   `scripts/omnidocbench-projection.py` 口径后才可外推全集（D-11）。
4. **确定性复现检查**：同二进制同参数重扫，退化页集合与 page md 必须与
   前轮逐字节一致（balanced 语料 2026-11-07 已验证 213/213 一致）。

## 结果

见 `reports/`（官方评测原始输出）与
`docs/test/reports/omnidocbench-baseline-<date>.md`（人读报告：四档 Overall、
text/table/formula 分项、zh/en 切分、量化损耗阶梯、对照官方 BF16 96.58）。
