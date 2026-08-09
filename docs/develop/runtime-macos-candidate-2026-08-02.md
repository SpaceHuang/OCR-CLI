# macOS Rust runtime candidate

当前已将 MTMD 依赖接入 `ocrx-runtime` 的 macOS target：

```text
llama-cpp-2       = 0.1.152
llama-cpp-sys-2   = 0.1.153
features          = llama-cpp-2/mtmd, llama-cpp-sys-2/metal+mtmd
```

`Cargo.lock` 已锁定上述版本，`cargo check -p ocrx-runtime --offline` 通过。

已知底层版本事实：

- 当前 `llama-cpp-sys-2 0.1.153` 内嵌 llama.cpp commit：`9e3b928fd8c9d14dbf15a8768b9fdd7e5c721d66`。
- 之前验证过的 CLI b10210 commit：`000547513f1530346ecd163db8b3e13962949961`。
- 两者不是同一 commit，因此当前组合只能称为 Rust runtime candidate，不能称为最终对齐组合。

当前 probe 已在沙箱外的 M2 Metal 环境完成真实生成：model/mmproj 加载、MTMD image encode、`eval_chunks()`、greedy token 采样、EOS 检测和 128 token 上限均通过；输出包含 bbox、公式和 HTML table。Metal 运行日志确认使用嵌入 Metal library 和 Apple M2。probe stdout SHA-256 为 `5da8c8010f0bca56f1b734629a4ad2dd998e9b21a258b08bf7dad6bf38bdb072`。

## 冻结验证结果

真实 M2 Metal probe 已验证：与 b10210 CLI 的结构化输出一致（正文、bbox、公式和 HTML table；不冻结逐字节相等），EOS（`runtime_termination=eos`）、max-token（`runtime_termination=max_tokens`）、协作取消（`runtime_termination=cancelled`）以及可审计进度事件（`runtime_progress generated_tokens=N`）。`cargo check -p ocrx-runtime --bin mtmd-generate-probe --offline` 通过。

因此 v0.1 macOS Metal runtime 的加载、图像编码、生成、EOS、max-token、协作取消和进度事件契约冻结；真实 CLI 接线后的 SIGINT/checkpoint 属于 CLI 契约测试，不再是 binding 可行性阻断。

CPU fallback 方案已冻结：v0.1 macOS 不做运行时 CPU fallback。启动阶段只执行轻量 Metal 探测（创建 `MTLDevice`、检查 GPU family 和内嵌 Metal library）；模型/mmproj 与 llama.cpp runtime 延迟到首次 OCR 时初始化。Metal 初始化失败时直接返回可诊断错误，不切换 CPU。用户无需安装 Metal、Xcode 或额外驱动；Xcode/Metal Toolchain 仅用于构建。
