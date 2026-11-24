# R1 运行时路线裁决记录（2026-11-17）

**裁决：** **路线 A——paddle sidecar（批式预检测形态）**
**依据：** 预注册判据（D-35）"路线 B 转换成功且规范化后完整区域序列一致 → 选 B"——转换环节未通过，判据未满足，按预注册规则选 A。

## 1. 路线 B 实测（未通过）

- `paddle2onnx`（最新 wheel，含 `paddlepaddle 3.4.0` 在位）在 Python 3.12 环境
  C++ 扩展加载即失败：`ImportError: DLL load failed while importing
  paddle2onnx_cpp2py_export`——force-reinstall 后复现，属 wheel 与本环境
  的二进制不兼容，非操作问题。转换判据第一关即未通过。
- 备选路径（`paddle.onnx.export`）同样落在 paddle2onnx 依赖上，同因出局。

## 2. 路线 A 形态（R2 接线的输入）

**批式预检测**：区域只依赖渲染位图，与 OCR 解码无耦合——扫描前对整个文档
一次性预计算区域（复用已验证的 `PaddleDetector` 代码路径，含坐标交换修正），
产出**区域文件**（每文档一份 JSON：`{image_path, regions:[{class,
index_in_class, bbox_px, conf}]}`，R1b 规范化契约序列）。扫描管线内的
`RegionProvider` 端口（R2.1）读取该文件——运行时不嵌 Python 调用，
sidecar 仅是**预处理工具**（`layout-detect` 子命令或独立脚本），不是
扫描热路径组件。

- 优势：零转换风险；B2 探针与本方案验证完全同代码路径。
- 代价：发行需 paddle runtime（约 600MB）——按方案 §6 预注册条款，发行
  形态（可选依赖 vs 引导安装）随 R3 结果再定。
- 身份：`layout_model_id = "pp-doclayout-v2"` + 资产哈希（manifest，D-45）
  进 checkpoint/result（B4.2 字段已就绪）。

## 3. 三态真实行为（本裁决的额外证据）

真实 V2 输出 744 页验收（`analysis-b0-pairing-safety.json`）：漏检 6 /
多检 20 / 去重 3 / 歧义 16 / 计数不匹配 26——全部被积分制整页跳过规则
正确处置（unsafe 0），证明区域提供者的降级语义在真实输出上成立。

## 4. 后续

R2 各子任务按本裁决的批式形态接线（R2.1 的 RegionProvider 读文件而非
进程交互，实现与测试成本显著降低）。
