# PDF 异常行为 fixture

生成命令：

```bash
python3 scripts/generate-pdf-fixtures.py benchmarks/fixtures/pdf
```

生成器没有 Python 第三方依赖，所有输入都是确定性的合成文件；它们不参与
30 页人工真值的 OCR 质量评分。它们用于 CLI/PDF 层的错误分类和部分成功行为
测试。

| 文件 | 用途 | 预期 |
|---|---|---|
| `encrypted-password-required.pdf` | 缺失/错误密码 | 无密码必须以 `pdf_encrypted` 失败；密码 `ocrx-test` 必须能打开。 |
| `corrupt-truncated.pdf` | PDF 文件层截断损坏 | 以 `pdf_corrupt` 或等价的格式错误失败，绝不能产生 OCR 成功结果。 |
| `not-a-pdf.txt` | 非 PDF 输入 | 以 `pdf_non_pdf` 或等价的格式错误失败。 |
| `dense-text-for-max-tokens.pdf` | 生成输出 token 上限 | 使用显式的低 `--max-output-tokens`（如 32）运行 OCR；结果必须标记 `termination: "max_tokens"` / `truncated`，并以 partial 状态结束。 |

`dense-text-for-max-tokens.pdf` 是有效的一页 PDF，不承诺在产品默认
`max_tokens=16384` 下必然截断。它故意与一个低输出上限一起使用，目的是稳定地
检验截断状态传播，而不是把模型质量问题伪装为异常输入。

基础 PDFium 验证（包括加密 PDF 的无密码失败与有密码成功）可运行：

```bash
scripts/macos-pdf-fixture-check.sh
```

CLI 输入预检回归（不需要模型已安装）可运行：

```bash
scripts/macos-cli-negative-scan-check.sh
```

它会确认加密、损坏和非 PDF 输入均在模型解析、下载或初始化之前返回稳定的
JSON 输入错误。密码恢复目前验证在 PDFium 边界；`ocrx scan` v0.1 的公开命令
契约尚未提供传入 PDF 密码的参数。
