# Community OvisOCR2 runtime smoke test

日期：2026-08-04  
资产：`Abiray/OvisOCR2-GGUF`  
revision：`8a22290a0c42dbcf84739d6d4c2763f877494ae0`  
profile：`compact` / `OvisOCR2-Q4_K_M.gguf`  
projector：`mmproj-F16.gguf`

## 结果

- Q4_K_M GGUF 加载：通过
- GGUF metadata/架构识别：通过（`qwen35`）
- mmproj 加载：通过
- MTMD bitmap 读取：通过
- MTMD image tokenize：通过
- context 初始化：通过
- Metal 图像编码/解码：通过
- 生成输出：通过（1 token，正常 EOS）

真实 macOS Terminal 执行已确认 Metal backend 正常工作：`MTL0 (Apple M2)` 被识别，模型
层分配到 GPU，图像完成两批解码，随后生成循环输出 `runtime_progress generated_tokens=1`
并以 `runtime_termination=eos` 正常结束。`tensor API disabled for pre-M5 and pre-A19
devices` 是 Apple M2 上的预期提示，不是失败原因。

Codex 沙箱内仍可能复现 command queue 失败，但不再阻断真实 macOS runtime 验收。
