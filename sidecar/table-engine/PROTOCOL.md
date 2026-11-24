# Table Engine Sidecar 协议 v1（冻结 fixture 契约，M4.2 D-59）

行分隔 JSON（每行一个完整 JSON 对象，UTF-8），stdin/stdout 全双工严格串行：
Rust 客户端一次只有一个未决请求；worker 按序应答。stderr 仅用于日志，
协议帧不走 stderr。worker 收到 stdin EOF 后退出（Rust 关闭 stdin = 正常关停）。

## 启动握手（worker → Rust，启动后立即发送，先于任何模型加载）

```json
{"type":"hello","engine":"slanet-plus","revision":"bae6e5f8",
 "paddleocr":"3.7.0","paddlex":"3.7.2","paddlepaddle":"3.4.0",
 "models_sha256":{"<model_dir_name>":"<sha256>","...":"..."}}
```

Rust 侧逐字段比对冻结期望值（engine/revision/paddleocr/paddlex/paddlepaddle）；
models_sha256 与 `assets/manifests/table-engine-slanet-plus/` 冻结清单比对。
任何不匹配 = D-63② 启动类故障（硬错误，无页面产物）。

## 请求：识别（Rust → worker）

```json
{"type":"recognize","id":7,"png_b64":"<base64 PNG>","page":12,"ordinal":0}
```

`png_b64` = 裁剪区域 PNG（Rust 侧 crop_region + PNG 编码）。`id` 单调递增。

## 应答

成功（表格结构 + 单元格文本）：

```json
{"type":"result","id":7,"ok":true,"html":"<table>...</table>"}
```

结构化拒绝（worker 判定，非错误）：

```json
{"type":"result","id":7,"ok":false,"reject":"no_ruling_lines"}
{"type":"result","id":7,"ok":false,"reject":"empty_output"}
```

- `no_ruling_lines`：表格线预检门未通过（D-62：横向长线 <2 且纵向长线 <2），
  不加载/不运行识别管线。
- `empty_output`：识别管线运行但 `pred_html` 为空（内嵌单元格检测无结构）。

协议错误（worker 无法处理该帧；连接视为不可用）：

```json
{"type":"error","id":7,"message":"<diagnostic>"}
```

## 心跳

```json
{"type":"ping","id":3}   ←→   {"type":"result","id":3,"ok":true,"html":"pong"}
```

（pong 复用 result 帧形，避免第三种成功帧。）

## 冻结常量

- 表格线门（cv2）：灰度 → `adaptiveThreshold(255-gray, MEAN, blockSize=15, C=-2)`
  → 形态学开运算（横 kernel `(ceil(0.45*w), 1)`，纵 kernel `(1, ceil(0.45*h))`）
  → `HoughLinesP(rho=1, theta=π/180, threshold=40, minLineLength=kernel, gap=3)`
  → 横线数 ≥2 或 纵线数 ≥2 判定有线。T0 校准 27/27（伪表 0 线被拒，16 张
  有产出的真表全有线）。
- 超时：握手 30s；单 recognize 600s（CPU 186s/表实测 + 余量）。超时 = D-63③
  engine_error（该区域保原）；worker 进程死亡后后续请求 = D-63④ engine_down。
