# OCRX DSH 插件需求 v0.1

状态：共识基线（待开发；2026-11-26 六轮评审通过：首评审 BLOCK×6 → 重审 BLOCK×3 → 二审 BLOCK×2 → 三审 PASS → 四审 BLOCK×3 → 五审 BLOCK×1 → 本轮 PASS。修订历史与依据见 `docs/review/dsh-plugin-v0.1-requirement-review-2026-11-26.md` 及其 `-re-review.md`、`-re-review-2.md`、`-re-review-3.md`、`-re-review-4.md`、`-re-review-5.md`）  
日期：2026-11-26  
工作名称：DSH 插件分发（DeepSeek Harness Plugin Distribution）  
适用产品：`ocrx`（新增 DSH 插件包与 npm 平台二进制包，含 §13 所列 CLI 侧配套扩展）

## 1. 背景与问题

### 1.1 产品背景

DeepSeek Harness（`dsh`）是 DeepSeek AI 开源的 Agent Harness，采用 Cordis 框架的 "everything-is-a-plugin" 架构。插件经 `dsh plugin --profile <name> add <pkg>` 安装（透传给 PATH 上的 pnpm，支持 npm 包、本地路径、git、tarball）；插件可贡献模型工具、slash 命令、Skill 等，经 `ctx.subprocess` 以全显式 argv 托管外部进程。DSH 的插件市场（Discover）是实验性机制：index 由作者**自行托管**、用户手动添加 URL，不存在官方提交入口；官方 bundle 清单编译在安装包内。

做 DSH 插件的战略目的是**扩大产品影响力**：DSH 用户群是 Agent 重度使用者，正是 `ocrx` 的目标人群。插件是产品进入 DSH 生态的**分发前门**——用户发现、安装、用起来的全链路体验决定这扇门能带来多少用户。

### 1.2 现状问题

`ocrx` 当前的获取与启用链路是：发现 GitHub 仓库 → 下载 Release 归档 → 运行 `install.ps1` / `install.sh` → 手动 `ocrx model install` 下载 GB 级权重（默认 OvisOCR3 quality 约 1.0 GB，含 mmproj）→ 阅读 SKILL.md 学习用法。这条链路对 DSH 用户完全失效：

1. **npm 上没有 ocrx**。DSH 插件安装以 npm 为主通道，`ocrx` 二进制只存在于 GitHub Release 归档。
2. **模型权重是最大的启用摩擦**。GB 级下载且当前不支持续传（中断即从零开始），无 agent 化引导；裸装用户第一次撞到模型错误后大概率放弃。
3. **模型不知道 ocrx 的存在**。不装插件，DSH Agent 不会主动想到本地 OCR；装了插件，工具 schema 自动进系统提示。
4. **无可发现性**。npm 无包、无 `dsh-plugin` topic 曝光、无可被 Discover 消费的自托管 index。

### 1.3 关键平台事实（本需求的约束基线）

以下事实经静态核对确认，需求全部条款以此为前提（依据见 §18）：

1. **DSH 不为插件工具提供审批**。`tools/pre-execute` 瀑布兜底是 `allow`；插件工具要弹确认必须自己返回 `ask` 或在 `execute` 内经审批服务发起（服务经 `ctx.get('approval')` 可选获取，见 §6.3/§7.5.3）；`danger-full-access` 预设下审批策略为 `never`，一切询问直接判拒。
2. **插件自起的子进程不受 DSH 沙箱约束**。沙箱是调用方显式用 `ctx.sandbox.confine` 包裹 argv 才生效的；subprocess seam 不引用沙箱。
3. **`ocrx scan --non-interactive` 在模型缺失时会静默下载**（安装策略直通 `offline`/`non_interactive`，`license_accepted: true`）。因此插件必须自建"禁止下载"的执行策略，不能依赖平台审批或 CLI 行为。
4. **CLI 的稳定输出面是 `result.json`（schema v3）、失败对象（schema v1：`{schema_version, code, kind, message, cause, suggestion}`）与退出码**；`--report json` 的 `ResultReport` 格式化器是死代码，进度 JSONL 在进程结束时才一次性写出（非实时，进程被终止时无输出）。
5. **Windows 构建动态链接 MSVC CRT**，Release 归档随包分发三个 CRT DLL；macOS 归档目前缺许可文件。
6. **DSH 在 `dsh.bundle.platforms` 声明的平台之外直接拒绝安装**（bundle 不激活、工具不注册）；`engines.dsh` 以 `semver.satisfies(..., {includePrerelease: true})` 校验，`<0.3.0` 拦不住 `0.3.0-alpha.x`。

## 2. 目标

### 2.1 北极星

**DSH 用户从发现插件到拿到第一份扫描结果，全程只做一件事：安装插件。** 其余一切（找二进制、下模型、学用法）由插件与 Agent 补齐；一切下载必须经显式同意，且同意机制由插件自身实现，不依赖平台审批。

### 2.2 产品目标

1. 插件包及全部平台二进制经 npm 分发：`dsh plugin --profile <name> add <ocrx 插件包>` 是唯一必须的手工步骤（前提：宿主已有 Node.js + pnpm，见 5.3），零配置文件改动、零额外下载命令。
2. 二进制随包分发：Windows x64 / macOS arm64 平台子包（esbuild 式 `optionalDependencies`）携带与 GitHub Release 归档**同一 CI 构建、同一文件集**的产物（Windows 含 CRT DLL 与许可，macOS 为 `bin/ + lib/` 布局并补齐许可），装完即可运行。
3. 首次使用自举：模型权重缺失时，由 scan 的结构化失败推导就绪度，Agent 在对话内经**插件自实现的同意询问**征得用户同意后调用模型安装工具补齐并自动重试，全程不出终端、不读文档；`ocrx_scan` 恒以禁止下载策略执行，**静默下载在架构上不可能发生**（成立边界与回归门槛见 §11.3、§14.3）。
4. 开箱引导：插件贡献 `/ocrx` slash 命令作为安装后的第一入口（状态总览 + 自检指引文本），Skill 经 provider 供给让模型立即掌握用法。
5. 结果交付以**工具文本结果**为主（结构化 JSON 的可读摘要 + 产物路径）；专用 Web 卡片需要独立 Client 插件，列入 P2。
6. 工具参数/结果建立在 CLI **实际**稳定面之上（CLI 参数、`result.json` v3、失败对象 v1、退出码），插件层做归一化，不发明新 OCR 语义；经 DSH subprocess 管线获得超时、取消与进程树终止。
7. 版本与状态可见：`/ocrx` 报告插件、二进制、模型三者的版本与状态（依赖 §13-C3 的版本数据源）。

### 2.3 成功定义（漏斗量化）

- 安装：`dsh plugin add` 一条命令成功启用（Node.js + pnpm 前提满足时），零手改配置、零手工下载；任何受支持平台上因插件包自身原因失败视为不达标。不支持的平台（Linux 等）在安装期得到明确拒绝提示，不算漏斗失败。
- 首次成功扫描：≤ 3 步人工动作、≤ 2 轮对话（不含模型下载等待）：① 安装插件；② 对 Agent 提出转换需求（第 1 轮）；③ 在同一对话流内对下载询问说"同意"（若跨轮则计入第 2 轮）。
- 零文档依赖：不阅读任何 ocrx 文档即可完成上述漏斗；模型侧知识全部来自工具描述与 Skill。
- 可发现性：npm 可检索（keywords 命中 dsh-plugin/deepseek-harness/ocr/pdf-to-markdown）、GitHub 仓库有 `dsh-plugin` topic、README 首屏 30 秒上手块；**不依赖官方 market**（无提交入口，自托管 index 为可选增强）。

## 3. 范围与非目标

### 3.1 本期范围

- DSH 插件包：manifest、bundle patch、入口、Config schema、`/ocrx` 命令、Skill provider。
- npm 平台二进制包（Windows x64、macOS arm64）：与 Release 归档同源同文件集（§5.1），经 `optionalDependencies` 随插件安装。
- 模型工具：`ocrx_scan`、`ocrx_doctor`、`ocrx_model`、`ocrx_model_install`（全部 P0，理由见 §7.1）。
- 同意机制：`ocrx_model_install` 的询问门、审批不可得时的拒绝路径、`ocrx_scan` 的恒禁下载策略。
- 就绪度推导与自举流（从 scan 失败对象推导，不做 scan 前预检）。
- CLI 侧配套扩展（§13，其中 C2/C3/C4 为 P0）。
- 测试（fake binary 单测、真实 CLI 集成、DSH 装载与漏斗冒烟）与 CI。

### 3.2 非目标

1. 不做进程内集成（Rust FFI / Node binding）。跨进程 CLI 是 v0.1 唯一集成边界。
2. 不在本期实现 `ocrx` 自身的 MCP server。
3. 不承诺冻结窗口外的 DSH API 稳定（上游全部为预发布版本）；只在 §6.2 冻结的 `engines.dsh` 窗口内保证可用。
4. 不把模型权重打进 npm 包。GB 级下载必须经同意，由 `ocrx_model_install` 承载。
5. 不改变 `ocrx scan` 的语义、参数与退出码。
6. 不做 DSH 内置客户端的工具卡片（需独立 Client 插件，P2）。
7. 不做后台任务执行与流式进度推送（P2）。
8. 不做遥测上报。
9. 不改造 DSH 官方 bundle 清单（改清单需要新的 dsh 版本，不在本产品控制面内）。
10. v0.1 不实现模型下载续传；如实呈现"中断即从零开始"（续传列入 §13-C7，P2）。

## 4. 集成形态选型

| 形态 | 机制 | 对分发目标的贡献 | 结论 |
| --- | --- | --- | --- |
| A. Cordis 工具插件 + npm 平台二进制 | `ctx.tools.register` + `ctx.subprocess.spawn` + `optionalDependencies` 携带二进制（官方先例：DSH `native/system` entry 包与 `tool-fs-search`） | npm 一次安装解决二进制 | **v0.1 采用** |
| B. MCP server | `ocrx` 提供 stdio MCP server，DSH 侧加一条 `dsh-mcp-client` row | 可扩大到所有 MCP 宿主，但 ocrx 侧成本高 | 待决策（§16） |
| C. Skill 供给 | `ctx.skills.registerProvider` | 让模型立即会用，成本极低 | **v0.1 采用（配套）** |
| D. 裸 bash 调用 | 无插件 | 无可发现性、无自举、无禁下载策略 | 放弃作为目标路径 |
| E. 能力 seam 后端（`ctx.ocr`） | 跨插件消费的 OCR 基础设施 | 生态位更高，需维护契约 | 远期 |

"运行时定位用户自装 CLI"降级为**回退路径**（§6.5），服务已装 `ocrx`（如 GPU 构建用户经 `Config.command` 指定）的场景。

## 5. 分发与可发现性

### 5.1 npm 包矩阵

| 包名（占位，待 §16 决策） | 内容 | 说明 |
| --- | --- | --- |
| `ocrx-dsh` | 插件本体（JS 入口、patch、Skill provider、`/ocrx` 命令） | 零或最少第三方依赖 |
| `@ocrx/ocrx-win32-x64` | 与 Windows Release 归档（CPU 部分）同一文件集：`bin\ocrx.exe`、`bin\pdfium.dll`、`bin\msvcp140.dll`、`bin\vcruntime140.dll`、`bin\vcruntime140_1.dll`、`SHA256SUMS`、`licenses\`（含 ms-vc-runtime） | `optionalDependencies`，`os`/`cpu` 字段声明；缺 CRT DLL 则干净机器加载器阶段即失败 |
| `@ocrx/ocrx-darwin-arm64` | 保持 CLI 的查找布局：`bin/ocrx` + `lib/libpdfium.dylib`（exe 相对 `../lib/` 查找）、`SHA256SUMS`、`licenses/`（macOS 线上归档当前缺许可，npm 包必须补齐） | 同上 |
| CUDA 变体 | 不在本期（Release 侧 CUDA 为本地工具链构建、非 CI 产物，见 §16） | 不阻塞 v0.1 |

- 平台包**单包体积 ≤ 64 MiB**（实测 Windows CPU 约 16.6 MB、macOS 约 23.6 MB；64 MiB 同时是参考 index 生成器的拒收上限）。
- CPU 变体的 npm 包与 GitHub Release 归档出自**同一次 CI 构建**（§13-C4 后成立），包内附 SHA256SUMS 并与 Release 清单一致；CUDA 不做此声明。
- `optionalDependencies` 安装失败不阻断插件装载：走 §6.5 定位回退与指引。
- 许可文件（llama.cpp、PDFium、MS VC 运行库等传递许可）随包完整分发。

### 5.2 可发现性

DSH 无官方 index 提交入口，可发现性以以下三层构成：

1. **npm 检索**：keywords 覆盖 `dsh-plugin`、`deepseek-harness`、`ocr`、`pdf-to-markdown`；README 首屏 30 秒上手块。
2. **GitHub 曝光**：仓库打 `dsh-plugin` topic；`ocrx` 主 README 与官网文档增加 DSH 安装入口。
3. **自托管 index（可选增强，P1）**：按 DSH index format 1 生成并托管 `index.json`（含版本、来源、`engines.dsh`、图标），README 给出"用户在 Discover 手动添加 URL"的说明；官方 index 无提交入口，不作为目标。

### 5.3 安装、卸载与升级

1. **前提**：宿主已有 Node.js 与 pnpm（`dsh plugin` 将参数透传给 PATH 上的 pnpm）。README 30 秒上手块必须写明该前提。
2. 安装：`dsh plugin --profile <name> add ocrx-dsh`（`--profile` 必填且须置于 pnpm 参数之前）。不得要求 `postinstall` 执行脚本（与 pnpm `allowBuilds` 门禁一致，列为禁止项）。
3. 卸载/升级：经 `dsh plugin`（remove/update 为 pnpm 动词透传）。卸载后**插件自身文件**无残留——不写全局配置、插件自身不写 `$DSH_HOME` 之外的位置；Skill 经 provider 供给不落盘。**范围限定**：经 `ocrx_model_install` 安装的模型缓存与 `.part` 残留位于用户缓存目录（`$DSH_HOME` 之外），与直装 CLI 共享，卸载插件**不删除**（§11.2）。升级插件即完成二进制升级。
4. **发布时间窗**：pnpm 11 默认 `minimumReleaseAge` 24 小时可能挡住新发布版本；发布公告与"可安装"之间留出该窗口，实现期按用户实际 pnpm 版本验证。

## 6. 插件包设计

### 6.1 包结构

```text
ocrx-dsh/
├── package.json                # dsh.bundle.patch、engines.dsh、platforms、optionalDependencies
├── cordis.patch.yml            # - insert: [{ id: ocrx, name: 'ocrx-dsh', config: {...} }]
├── lib/index.js                # 插件入口
├── src/index.ts                # 源码
└── skills/                     # SkillProvider 注册的内容（随包，不落盘到用户目录）
```

### 6.2 manifest 要求

| 字段 | 要求 |
| --- | --- |
| `main` / `exports` | 指向 `lib/index.js` |
| `dsh.bundle.patch` | insert 一条 row（id 建议 `ocrx`） |
| `engines.dsh` | **冻结为 CI 实测窗口**，上界带 `-0` 排除下一线预发布。初始：`>=0.2.2-rc.5 <0.2.3-0`（CI 锁定 `@deepseek-ai/dsh@0.2.2-rc.5`）。窗口扩大的唯一途径是该版本进入 CI 验证矩阵并随插件发版；不做无验证矩阵的宽范围承诺 |
| `dsh.bundle.platforms` | 声明 `win32`、`darwin`。声明后不受支持平台在**安装期被明确拒绝**（`UnsupportedBundlePlatformError`，bundle 不激活），这是选定的平台策略；插件内不做"未支持平台仍注册工具"的降级 |
| `optionalDependencies` | 平台二进制子包（§5.1），版本与插件同节奏 |
| `peerDependencies` | 所有 `@deepseek-ai/dsh*` 放 peerDependencies，与宿主共享实例 |

### 6.3 插件入口

- 导出 `name`、`inject: ['tools', 'commands', 'skills', 'subprocess', 'workingDirectory']`、Config schema、`apply(ctx, config)`。**`approval` 不进 `inject`**（必选 `inject` 会让无审批服务的部署整个插件不加载、工具全部消失，`unavailable` 分支永远不会发生）：需要时经 `ctx.get('approval')` 可选取得（仿 DSH bash/fs 工具的调用方式），服务缺席得 `undefined`，走 §7.5.3 的 `unavailable` 分支。
- 注册：四个模型工具（§7）、`/ocrx` 命令（§8.3）、SkillProvider（§10）。
- 二进制/模型缺失不阻断装载：工具照常注册，调用时返回结构化状态与指引（自举漏斗的前提）。

### 6.4 Config schema

| 字段 | 类型 / 默认 | 说明 |
| --- | --- | --- |
| `command` | `string?` | 覆盖二进制路径（GPU 构建、定制构建场景）；缺省用随包二进制 |
| `extraEnv` | `Record<string, string>` | 透传环境变量（如 `OCRX_PDFIUM_LIBRARY`、`OCRX_TABLE_ENGINE_CMD`）。注：`OCRX_MODEL_CACHE` 在 §13-C5 落地前不被 `scan` 读取，v0.1 不提供缓存目录配置项，避免"doctor 看一处、scan 用另一处"的失准 |
| `offline` | `boolean = false` | 仅透传给 `ocrx_model` / `ocrx_model_install` 的 `--offline`；**不影响 `ocrx_scan`**（scan 恒带 `--offline` 是硬编码行为，见 §11.3）。缺省 `false` 是自举流成立的条件：默认配置（零改动）下唯一下载入口在同意后即可执行；显式置 `true` 表示"永不下载"，此时 `ocrx_model_install` 会如实返回 CLI 的 offline 拒绝（`install_blocked`，§7.6） |
| `defaultTimeoutMs` | `number = 1800000`（30 分钟） | `ocrx_scan` 的 execute 内默认超时 |
| `maxTimeoutMs` | `number = 14400000`（4 小时） | `ocrx_scan` 注册的管线级 `timeoutMs` 上限（`timeoutMs` 是注册期单一静态值，30 分钟默认在 execute 内实施） |
| `installTimeoutMs` | `number?`（缺省不设） | `ocrx_model_install` 的可选上限；**缺省不声明管线超时**（DSH 对未声明 `timeoutMs` 的工具不设上限），因为模型下载不可续传，超时即放弃本次安装（下次从零开始）并留下 `.part` 残留（§7.5.3）——让用户主动取消而不是定时掐断 |
| `enableScan` / `enableDoctor` / `enableModel` / `enableModelInstall` | `boolean = true` | 工具开关；`enableModelInstall: false` 时该工具**不注册**（不是隐藏） |

### 6.5 二进制定位（含回退）

1. `Config.command`（显式覆盖，GPU/CUDA 构建用户的正式通道）。
2. 随包平台子包二进制（主路径）。
3. `PATH` 与已知安装位置（GitHub Release + `install.ps1`/`install.sh` 的目标目录，列表实现期固化）。
4. 全部未命中：工具照常注册，调用返回 `binary_not_found` + 指引（npm 重装插件 / GitHub Release 路线 / 设 `command`）。该原因只覆盖**同平台内**的缺失（optionalDependencies 安装失败、架构不匹配）；**平台不支持发生在安装期**（§6.2），不会走到这里。
5. 随包二进制存在但加载器阶段失败（如缺 CRT 的 `0xC0000135`）：插件层检测 spawn/立即退出形态，映射 `launcher_failed` reason，指引安装 VC++ 运行库或重装插件。
6. 顺序说明与已知取舍：随包二进制优先于 PATH 中的用户安装，GPU 构建用户会被遮蔽，须设 `Config.command`。README 与 `/ocrx` 总览显示当前所用二进制路径与来源（bundled / PATH / config），让 GPU 用户能自行发现并切换。

## 7. 工具设计

### 7.1 工具清单

| 工具名 | 对应 CLI | 并发安全 | 优先级 | 角色 |
| --- | --- | --- | --- | --- |
| `ocrx_scan` | `ocrx scan`（恒 `--offline`） | 否 | P0 | 产出交付物；不下载 |
| `ocrx_doctor` | `ocrx doctor --report json` | 是 | P0 | 状态展示；**不作为就绪度预检依据**（§8.2） |
| `ocrx_model` | `ocrx model list/info/recommend/verify` | 是 | P0 | 告诉 Agent"该装哪个"；§13-C2 后可给出体积/revision/目录 |
| `ocrx_model_install` | `ocrx model install` | 否（长任务） | P0 | **唯一下载入口**，自带同意门 |

`ocrx_model_install` 为 P0 的理由：它是自举漏斗的收口，缺了它 Agent 只能让用户去终端敲下载命令，漏斗断裂。工具名用 `ocrx_` 前缀避免冲突并让模型从名字归因。

### 7.2 `ocrx_scan` 参数 schema

| 参数 | 类型 | CLI 映射 | 说明 |
| --- | --- | --- | --- |
| `input` | `string[]`（必填） | 位置参数 | 绝对路径；目录表示批量 |
| `outputDir` | `string`（必填） | `--output-dir` | 统一必填，消除歧义 |
| `pages` | `string?` | `--pages` | 描述与 Skill 引导大 PDF 先抽样 |
| `model` | `enum?`：`'ovisocr2' \| 'ovisocr3'` | `--model` | 缺省走 CLI 自动选择 |
| `modelProfile` | `enum?` | `--model-profile` | `auto/compact/balanced/quality/reference` |
| `pdfMode` | `enum?` | `--pdf-mode` | `auto/text/ocr` |
| `qualityTier` | `enum?` | `--quality-tier` | `l1/l2` |
| `device` | `enum?` | `--device` | `auto/cpu/metal/cuda/vulkan`；`cuda`/`vulkan` 在 Windows CPU 平台包上由工具层直接拒绝（`device_unavailable`，静态能力事实，可预判），指引经 `Config.command` 使用 GPU 构建 |
| `resume` / `overwrite` | `boolean?` | `--resume` / `--overwrite` | 互斥，工具层校验拒绝；`overwrite` 语义为"允许写入非空输出目录"，**不清理旧产物**，工具描述如实表述 |
| `password` | `string?` | `--password` | 会随工具调用参数进入会话记录与模型上下文，无脱敏手段（§11.4） |
| `offline` | — | — | 不作为参数暴露：scan 恒带 `--offline`（§11.3） |

工具层无条件追加：`--non-interactive`、`--report json`、`--progress off`。`--progress off` 的理由：进度 JSONL 在进程结束时才一次性写出、对交付无贡献，而 stderr 需要承载失败对象（§7.3.2），去除进度行噪声以保证失败对象解析的可靠性。**不做 scan 前的模型就绪度预检**：预检依据（doctor/model list）在显式 model/profile 组合与自定义缓存目录下不可靠，且会破坏纯文本文档的零模型路径（§8.2）。

### 7.3 执行语义

1. `ctx.subprocess.spawn` 全显式 argv，不经 shell；`cwd` **必填**，取 `ctx.workingDirectory.ensure(...)` 且须传入 `agent`（`ensure` 的要求）；无 agent 执行时先判空，回落到插件进程的 `process.cwd()`（与官方 bash 工具的判空处理一致），不得因缺 agent 抛错。
2. 输出解析按流分流——失败对象所在流由 CLI 按**输入数量**决定（批量输入或目录输入才把失败对象改写到 stderr；单文档保持历史 stdout 错误契约）：
   - **stdout** 有界收集（`maxBytes` + spill）：单文档成功是 `result.json` 字符串（schema v3），**单文档失败是失败对象（schema v1）**——不能假定 stdout 恒为成功形态，成功/失败两种形态都要解析；多文档为聚合对象。
   - **stderr** 以 `{maxBytes, spill}` 保存全流：批量模式从中逐行解析 schema v1 逐文档失败对象（与人类可读行混排）；解析结果入内存结构，原始文本仅保留有界尾部兜底。
   - **不承诺进度尾部**：进度 JSONL 已因 `--progress off`（§7.2）不存在，超时/取消场景本就无输出（§1.3.4）。
3. 环境注入 `extraEnv`；其余继承 seam 清洗后的环境，工具层不新增透传通道。
4. 取消/超时经 spawn 规格的 `signal`（与工具上下文 `exec.signal` 关联）触发，进程树终止由 seam 承担；macOS 上 DSH 无持久进程范围属主，进程树终止走降级路径——验收以"ocrx 进程已终止"为准，不承诺孙进程（sidecar）级联。
5. 并发安全：`isConcurrencySafe` 以函数形式注册、恒返回 `false`（scan/install），避免显存与输出目录争抢。
6. `ocrx_scan` 注册管线 `timeoutMs = Config.maxTimeoutMs`；execute 内启动 `Config.defaultTimeoutMs` 计时器，到点主动终止 CLI 并返回 `timeout` reason。`ocrx_model_install` 缺省不注册 `timeoutMs`（§6.4）。
7. 退出码 `0` 与 `10`（部分完成）均返回结果；`interrupted` reason 由**插件自身的取消/超时状态**判定，不以退出码 130 为依据（130 仅来自控制台 Ctrl+C，插件经 abort 终止的进程不会产生它）。

### 7.4 结果 schema（插件归一化）

CLI stdout 有三种形态（§1.3.4），插件归一化为自己的结果 schema：

| CLI 形态 | 出现条件 | 归一化来源 |
| --- | --- | --- |
| `result.json` 字符串（schema v3，status `success \| partial`） | 单文档成功 | stdout 直接解析 |
| 聚合对象（每文档仅 `input`/`output_dir`/`status`/`exit_code`，`elapsed_ms` 恒 0） | 多文档 | 逐文档 exit_code；页数/warnings 等细节由插件读取各 `<output_dir>/result.json` 补齐 |
| 失败对象（schema v1：`code`/`kind`/`message`/`cause`/`suggestion`） | 失败 | 映射为 `error`（§7.6）。所在流按输入数量分流：**单文档在 stdout，批量逐文档在 stderr**（§7.3.2） |

```json
{
  "ok": true,
  "exit_code": 0,
  "partial": false,
  "documents": [
    { "input": "...", "output_dir": "...", "status": "success | partial | failed | cancelled",
      "exit_code": 0,
      "pages": 42, "markdown": "...", "result_json": "...",
      "warnings_count": 1, "duration_ms": 186400,
      "error": null }
  ],
  "scan_summary": "多文档时输出根下 scan-summary.json 的路径",
  "error": null
}
```

1. 不内联 Markdown 正文与 `result.json` 全文；产物以路径返回，模型按需读文件。
2. `documents[].status` 归一化为 `success | partial | failed | cancelled`，由 CLI 批量状态（四种取值）与逐文档 `exit_code` 映射；逐文档 `error`（失败/取消文档）为 `{ reason, message, guidance }`，`reason` 按 §7.6 从**该文档的失败对象**（按输入数量分流：单文档在 stdout、批量在 stderr，§7.3.2）推导。该设计可完整表达批量部分失败：两文档分别因 `model_missing` 与 `input` 失败时，各自携带正确的 `error`。
3. `ok` 取值：退出码 ∈ {0, 10} 时 `ok: true`（`10` 附 `partial: true`）；其余 `ok: false`。顶层 `error`（同 `{ reason, message, guidance }`）在整次失败（单文档路径）时必填；批量含失败文档时取**首个失败文档**的原因，其余原因保留在 `documents[]` 内。
4. 插件自身取消/超时时，CLI 的 stdout/stderr 都缓冲到进程结束才写出、进程被终止即无任何输出：`documents[]` 为空，只返回顶层 `interrupted` 错误。`cancelled` 状态仅用于 CLI 自身已给出逐文档信息的场景（如控制台中断后的批量结果）。

### 7.5 `ocrx_doctor` / `ocrx_model` / `ocrx_model_install`

1. `ocrx_doctor`：透传 `doctor --report json`，工具结果附加插件视角字段（所用二进制路径与来源、随包版本、插件版本；ocrx 真实版本依赖 §13-C3）。**如实标注 doctor 的局限**：`installed_profiles` 只覆盖一个已解析模型，不感知自定义缓存目录——它用于状态展示，不用于 scan 前拦截。
2. `ocrx_model`（查询）——参数 schema：

| 参数 | 类型 | CLI 映射 | 说明 |
| --- | --- | --- | --- |
| `action` | `enum`（必填）：`list \| info \| recommend \| verify` | 子命令 | `list` 无需其他参数 |
| `model` | `enum?`：`'ovisocr2' \| 'ovisocr3'` | `--model` | `info` / `verify` / **`recommend`** 可用（CLI 支持 `model recommend --model <model>`，与 scan 的按模型 auto 解析同逻辑）；缺省交由 CLI 默认模型 |
| `profile` | `enum?`：`compact \| balanced \| quality \| reference` | 位置参数 | 仅 `verify` 使用；缺省省略 |

   工具层无条件追加 `--non-interactive`；**禁止使用 `--report` / `--progress` / `--model-profile`**（CLI 对 model 子命令一律报 `unsupported model flag`）。职责：查询已安装模型、推荐档位、校验完整性；§13-C2 落地后可回答"装 model×profile 需要多少字节、装到哪"，C2 落地前对体积/目录问题如实返回"需要 CLI 侧安装计划输出（未实现），可从官方文档获得估算值"。
3. `ocrx_model_install`——参数 schema：

| 参数 | 类型 | CLI 映射 | 说明 |
| --- | --- | --- | --- |
| `model` | `enum?`：`'ovisocr2' \| 'ovisocr3'` | `--model <model>` | 缺省不传，由 CLI 默认模型解析 |
| `modelProfile` | `enum?`：`compact \| balanced \| quality \| reference` | 位置参数 `<profile>` | **CLI 强制要求 profile**（解析与执行两处检查，无缺省值）；缺省时由工具层填充：自举流取就绪度目标（按本节"自举目标继承"矩阵解析）；无来源可得时返回结构化错误，**不得发出缺 profile 的 argv** |

   argv 形态：`ocrx model install [--model <model>] [--offline] --non-interactive <profile>`（`--offline` 按 Config.offline；C2 落地后按其定义追加机器可读旗标）。工具结果附实际安装目标 `installed: { model, profile }`。
   **自举目标继承（四种组合的确定解析矩阵；显式 `modelProfile: "auto"` 等同于未显式指定 profile）**：

| 原 scan 参数组合 | 目标 (model, profile) 解析 |
| --- | --- |
| 二者均显式 | 直接沿用该对值 |
| 仅 `model` 显式 | `ocrx_model(action: "recommend", model: <原值>)`——保留该 model，profile 采用按此模型的推荐返回值（与 scan 的按模型 auto 解析同逻辑）；**不得用无参数 recommend 代替**（可能跨模型） |
| 仅 `modelProfile` 显式 | 保留该 profile；model 取与 scan 一致的 **CLI 默认模型**（来自 CLI 查询/机器可读解析结果，**不由插件硬编码**）；**不得用无参数 recommend 代替**（其跨模型隐式选择可能返回另一模型 + 推荐档位，偏离原扫描目标） |
| 二者均缺省 | 无参数 `ocrx_model(action: "recommend")`，采用返回的实际 (model, profile) |

   解析结果贯穿 readiness、同意文案、install argv 与重试 scan；**不得固定写成 OvisOCR3 quality**（§8.2.2 数值仅为示例）。
   - **同意门（插件自实现，形态唯一）**：在 `execute` 内经 **`ctx.get('approval')`** 取得审批服务后调用 `request` 发起确认。`approval` 不是必选依赖（§6.3，仿 DSH bash/fs 工具的可选访问）。**不采用 `tools/pre-execute` 返回 `ask` 的形态**：该路径的询问与拒绝文案均由 DSH 管线生成（固定模板，如 `the user rejected tool "<name>"`），插件的 `execute` 不会运行，无法返回带指引的结构化结果，无法满足 §8.2.4。询问文案必须含真实体积与目标目录（§13-C2），并注明"中断需重新下载，暂存文件将占盘至下次安装开始时清理"。
   - `request` 返回与异常的归类约定：`rejected`（**用户拒绝，以及 `never` 策略**——该策略在审批服务内、分发之前就被强制执行，故归入拒绝而非不可用）→ 结构化 `consent_denied` 错误 + 离线放置指引（手动下载 + 缓存目录说明），不重试；`unavailable`（审批服务缺席——`ctx.get` 得 `undefined`、`request` 抛错、或前置条件不满足如无 agent / 不在打开的 turn 内）→ 与 `rejected` 同样处理；`cancelled` → `consent_cancelled`，提示可在对话中重新发起。任何分支都不降级为静默下载。
   - `Config.offline: true` 时 CLI 以退出码 5 拒绝安装（`model install blocked: offline mode`），结果如实映射 `install_blocked` 并说明与 `offline` 配置的冲突（§7.6）。
   - 取消行为：进程树终止。**CLI 现状无安装中断处理**：`.part-<role>` 暂存文件残留、无清理路径（唯一的清理函数无调用方，且文件名模式与实际暂存名不匹配），下次安装不复用、从零开始；因缺省不设超时，用户取消是终止停滞下载的唯一途径，每次取消最多残留约 1 GB。v0.1 处置：工具结果**如实报告残留**——目录与体积由插件按安装计划的 `target_dir`（revision 目录，见 §13-C2）自行扫描 `.part-*` 得出，覆盖 profile 与 revision 两个目录（不新增 CLI 面），并提示"下次安装开始时自动清理"；残留的确定性回收由 §13-C2 承担（安装开始前清理陈旧 `.part-*`）。续传列入 §13-C7（P2）。
   - `enableModelInstall: false` 时工具不注册，Agent 无从发起下载。
   - 实现期验证项：PTC（经 `run_code` 调用工具）路径下 `ctx.get('approval')` 的 `request` 表现未核实，列入 §14.1 验证清单。

### 7.6 错误映射（按 CLI 实际行为）

**判定规则**（先于下表）：

1. `reason` 以 **(代码路径, code, kind)** 三元组判定，`kind` 优先于 `code`；不以 `code` 单独判定——scan 与 install 共享部分退出码但语义不同（如 5）。
2. `kind` 已知 → 按表映射；code 在表内但 `kind` 未列出 → `cli_error`（透传原始失败对象的 `code`/`kind`/`cause`/`suggestion`），**不得落入 `launcher_failed`**；**install 路径为本条的例外：未列 kind → `install_failed`**（见 install 行与数据来源段）；code 表外或进程未产生有效输出（spawn 失败、加载器失败如 `0xC0000135`）→ `launcher_failed`。
3. 指引文案由插件生成，**不依赖 CLI 的 `suggestion`**（`prepare` 失败对象的 `suggestion` 恒为 `null`），按 `cause` 的类别前缀分支。
4. code 在表内但**完全没有失败对象**（仅一行文本；真实分支：内存低于 6 GiB 的退出码 6、整次调用 I/O 中止的退出码 3）→ 回落到仅按 code 的表行默认 `reason`（3 → `input`，6 → `runtime`，其余类推），并标注 `details_unavailable`；`launcher_failed` 只用于表外退出码与 spawn 失败，不承接此情形。
5. **代码路径前置**：`ocrx_scan` 与 `ocrx_model_install` 的失败分别按各自路径的表行判定；install 路径**不得套用 scan 的 `model_missing`**（一次失败的下载不能被引导成再次安装）。

**单文档与逐文档映射**：

| code | kind / cause | reason | guidance 要点 |
| --- | --- | --- | --- |
| 2 | — | `usage` | 修正 schema 参数 |
| 3 | `input` | `input` | 核对绝对路径 |
| 3 | `pdf` | `pdfium_missing` | PDFium 缺失/损坏；重装插件或设 `OCRX_PDFIUM_LIBRARY` |
| 3 | `pdf_native` | `pdf_native` | `pdfMode: "text"` 遇到非可靠文本页；改用 `pdfMode: "auto"` 或 `"ocr"` |
| 4 | `model`，`cause: "offline"`（模型缺失） | `model_missing` | **自举流**：`ocrx_model` 确认目标（继承原 scan 目标，§7.5）→ 征得同意 → `ocrx_model_install` → **原参数 + `resume: true` 重试**（批量必带 resume：首次失败已写 `scan-summary.json`；原 `overwrite` 改为仅 `resume`，§8.2.3） |
| 4 | `model`，其他 cause | `model` | 按 `cause` 前缀分支：`unsupported_profile` → 修正 model/modelProfile；`runtime_initialization` → 重试或换 profile/device |
| 5 | `output`（**scan 路径**：写 `result.json` 失败） | `output` | 同退出码 9 |
| 5 | install 路径：offline 拦截或磁盘不足（纯文本；stderr 原文可区分：`model install blocked: offline mode` / 磁盘不足） | `install_blocked` | 指引附 stderr 原文以保留二者可区分信息：offline 冲突需用户显式关闭 `Config.offline`；磁盘不足提示清理空间（含陈旧 `.part-*`，§13-C2） |
| 4 | install 路径：无失败对象（C2 前过渡规则；传输失败、哈希不匹配、安装锁等通用分支，仅 stderr 一行文本） | `install_failed` | 原始文本透传；**不自动重试、不再次发起下载同意**，提示用户检查网络与磁盘后自行决定 |
| 6 | 失败对象 `kind: "runtime"` 且 `message` 匹配 `requested device is unavailable` | `device_unavailable` | Windows CPU 包无 cuda/vulkan；经 `Config.command` 用 GPU 构建或去掉 device |
| 6 | 无失败对象（仅一行文本，落入判定规则第 4 条按 code 默认） | `runtime` | 内存/设备不满足；`modelProfile: "compact"` 或换 device |
| 9 | `output` | `output` | 指引附 CLI 给出的 `cause`/`suggestion`；常见诱因是输出目录策略拒绝（目录为 cwd 或其上级、与输入重叠、含 `..`、已存在 `scan-summary.json`），其次才是权限与磁盘（**不含沙箱表述**，§11.2） |
| 10 | — | `partial` | ok=true + 逐文档失败原因 |
| 130 | — | `interrupted`（控制台中断） | 提示 `resume: true`；插件自发起的取消不走此码 |

**数据来源**：单文档路径的失败对象在 **stdout**（历史 stdout 错误契约，成功时同一位置是 result.json 字符串，须先判别形态）；批量路径的逐文档失败对象在 **stderr** 的 JSON 行（§7.3.2 spill 全流解析，`--progress off` 已去除进度噪声）。`model install` 路径：C2 落地后输出 schema v1 失败对象（**stdout**，对齐单文档 scan 错误契约；kind 建议值 `transport` / `hash` / `lock` / `offline` / `disk`，按 kind 优先规则映射，未列 kind → `install_failed`）；C2 落地前仅 stderr 纯文本，按上表 install 行的 code + 文本映射。

**批量路径**：整体退出码聚合规则（含 130 取 130；否则存在非 0/10 即取 3）**不可用于原因推导**，插件对每个文档独立套用上表；模型缺失在批量下经 stderr 失败对象推导 `model_missing` 引导，不会被误报为 `input`；单文档模型缺失经 stdout 失败对象推导，同一引导成立。

**兜底**：`launcher_failed` = 随包二进制损坏或运行库缺失，指引重装插件/安装运行库；`cli_error` = 表内 code 但 `kind` 未列出的分支，如实透传原始字段。

### 7.7 结果交付

1. v0.1 以**工具文本结果**交付：结构化 JSON 的紧凑摘要（文档状态、页数、耗时、产物路径列表）。DSH 内置 Web 客户端不消费 `presentCall`/`presentResult`，卡片不作为 v0.1 交付物。
2. 专用结果卡片需要独立 Client 插件（`packages/client` 半区），列入 P2。
3. `password` 无脱敏通道（参数进入会话记录与模型上下文），工具描述与 README 如实声明（§11.4）。

## 8. 首次使用漏斗（开箱体验）

### 8.1 漏斗定义

| 环节 | 用户动作 | 系统行为 | 失败模式（必须避免） |
| --- | --- | --- | --- |
| 发现 | npm / GitHub / （可选）自托管 index | keywords 命中；README 30 秒上手块含 pnpm 前提 | 需翻文档才知道能干嘛 |
| 安装 | `dsh plugin add ocrx-dsh` | npm 拉取插件 + 平台子包，无构建脚本；不支持平台安装期明确拒绝 | postinstall 白名单提示吓退用户 |
| 首次感知 | `/ocrx` 或直接提需求 | 状态总览文本；工具 schema 进系统提示 | 工具存在但模型不知道何时用 |
| 首次扫描 | 对 Agent 说"把 x.pdf 转 markdown" | `ocrx_scan`（恒 `--offline`）直接执行 | 让用户先手动装模型 |
| 模型自举 | 对插件发起的同意询问说"同意" | `ocrx_model_install`（含真实体积/目录/不可续传提示）→ 完成后自动重试 scan | 让用户去终端敲下载命令；静默下载 |
| 交付 | 无 | 文本摘要 + 产物路径 | 把全文糊进对话 |

### 8.2 就绪度推导与自举流

1. **不做 scan 前预检**。理由：(a) doctor/model list 在显式 model/profile 组合、自定义缓存目录下会误判；(b) 纯文本 PDF 不需要模型，预检会强迫秒出文档先下载权重。就绪度一律从 **scan 的结构化失败**推导（失败对象 `code: 4, kind: "model"` 等）。
2. `ocrx_scan` 恒带 `--offline`：模型缺失时得到结构化失败，工具层映射 `model_missing`，结果附 `readiness`：

```json
{ "error": { "reason": "model_missing", "message": "...", "guidance": "..." },
  "readiness": { "model_missing": true,
                 "recommend": { "model": "ovisocr3", "profile": "quality",
                                "size_bytes": 1016830848, "target_dir": "..." } } }
```

   `size_bytes` / `target_dir` 来自 §13-C2 的安装计划；C2 落地前如实标注"体积待确认（约 1.0 GB，见官方文档）"。示例值仅代表二者均缺省组合（默认路径）的典型输出；自举目标遵循 §7.5 的四组合解析矩阵，**不固定 OvisOCR3 quality**。
3. Agent 据此向用户说明并征得同意 → 调用 `ocrx_model_install`（同意门在 `execute` 内经 `ctx.get('approval')` 发起，§7.5.3）→ 成功后工具结果明确"已就绪，可重试" → Agent 重试 `ocrx_scan`。**默认配置（零改动）下此流成立**：`offline` 缺省 `false`（§6.4），同意后安装不会被 CLI 拒绝。**重试的批量策略由插件保证**：以**原参数 + `resume: true`** 重试——首次批量失败已写出 `scan-summary.json`，不带 `resume`/`overwrite` 的再次批量调用会被 CLI 以退出码 9 / kind `output` 拒绝；原调用若带 `overwrite: true`，内部重试改为仅带 `resume`（二者互斥）。该规则写入工具结果 guidance 与 Skill（§10.2），不依赖 Agent 自行推断；不修改 CLI 的失败汇总重入规则。
4. **审批被拒或不可得**（`rejected`：用户拒绝或 `never` 策略；`unavailable`：无审批服务、`request` 异常或前置不满足）：`ocrx_model_install` 返回 `consent_denied` + 离线放置指引（手动下载 + 缓存目录说明），不重试、不静默下载；指引文案由插件生成（§7.5.3），不依赖管线模板。
5. 显式 `model`/`modelProfile` 与已装档位不一致：同样由 scan 失败推导并进入上述引导流，不做工具层预校验。

### 8.3 `/ocrx` slash 命令

经 `ctx.commands` 注册（turn 之外直执行，结果为文本，**无确认通道**）：

1. 无参数：状态总览文本——插件版本、随包二进制版本与平台、当前生效二进制来源（bundled/PATH/config）、模型安装状态（标注 doctor 局限）、缓存根路径、工具开关。ocrx 真实版本依赖 §13-C3。
2. `/ocrx doctor`：完整自检 + 修复动作清单（可执行的下一步：在对话中让 Agent 安装模型 / 检查磁盘 / 重装插件）。
3. **不提供 `/ocrx install-model`**：命令无确认通道（审批 `request` 要求会话处于打开的 turn 内，命令在 turn 之外直执行），模型安装一律回到对话轮内经 `ocrx_model_install` 完成。

## 9. 长任务与进度

1. `ocrx_scan`：同步工具，execute 内默认 30 分钟、注册上限 4 小时（§6.4/§7.3.6）；引导大 PDF 先 `pages` 抽样。
2. `ocrx_model_install`：GB 级下载（默认 OvisOCR3 quality 约 1.0 GB）。它是**同步工具**：§13-C2 的进度输出在**最终结果**中呈现（阶段、字节、速率、校验统计）；下载过程中的实时可见性属 P2 流式进度，v0.1 不承诺。同意文案注明体积与目录。缺省不设工具超时，取消由用户主动发起。
3. 取消与恢复：**扫描**中断支持 `resume: true`（页级 checkpoint）；**模型下载不可续传**（CLI 现状），中断即从零开始；取消后 `.part-<role>` 暂存文件**残留**且当前无清理路径（§7.5.3），由 §13-C2 的"安装开始前清理陈旧 `.part-*`"闭环；`/ocrx` 与同意文案如实注明。续传为 §13-C7（P2）。

## 10. Skill 供给与安装

1. 插件经 `ctx.skills.registerProvider` 供给 Skill（manifest 无 skills 字段，必须代码注册；`inject` 含 `skills`）。装完插件模型即可获得用法，用户无需再跑安装命令。
2. 内容与 `assets/skills/ocrx/SKILL.md` **对齐**（不是自动同源生成——现无生成机制）：以工具调用为优先路径（先 `ocrx_doctor`、大 PDF 先抽样、`model_missing` 引导流——含安装后以原参数 + `resume: true` 重试的批量规则、resume 用法），裸 CLI 作为降级说明；默认模型表述跟随当前产品（OvisOCR3）。源 SKILL.md 的过期表述由 §13-C6 修正。
3. `ocrx skill install` 新增 `dsh` 目标（`<project>/.dsh/skills/ocrx`、`$DSH_HOME/skills/ocrx`，经 `OCRX_SKILL_TARGET=dsh`，§13-C1），作为不用插件场景的对齐路径；现有 `.codex` / `.claude` 目标行为不变。`$DSH_HOME` 未设置或空白时回落 `~/.dsh`（已核实 DSH `packages/util/home-paths`，原待决策 §16.9 结案）。

## 11. 安全、权限与沙箱

### 11.1 执行权限模型（选定方案，如实声明）

1. **插件 spawn 的 `ocrx` 进程不在 DSH 沙箱内**：DSH 沙箱需调用方显式 `ctx.sandbox.confine` 包裹 argv 才生效，subprocess seam 不引用沙箱。v0.1 选定**不做 confine 包装**——OCR 需写工作区外产物、读用户目录下模型缓存、安装需写用户目录，confine 会把安装与扫描卷入升级审批且 Windows ACL 后端不支持额外可写根，与零摩擦漏斗冲突。
2. 由此的如实声明（README 信任模型章节 + `/ocrx` 总览）："ocrx 工具以当前用户完整文件权限运行，可写任意输出目录、可读任意输入路径；对它的信任等同于对本地安装的 ocrx 本体的信任。" `additionalWritableRoots` / "检查沙箱可写根"类表述从本需求全部移除。
3. 经 `ctx.sandbox.confine` 包装的可选沙箱模式列入 P2 待决策（§16），若 DSH 未来提供按工具声明沙箱的机制则跟进。

### 11.2 文件效应

`ocrx_scan` 写 `outputDir`、读输入与模型缓存；`ocrx_model_install` 写模型缓存目录。目录选择权完全在调用参数与 CLI 默认规则，插件不代写路径。

### 11.3 下载同意（架构性保证）

1. **`ocrx_scan` 恒带 `--offline` 执行**——OCR 模型的静默安装路径（`--non-interactive` + `license_accepted` 直通）被参数面封死。**成立边界**：`--offline` 仅在模型安装策略中被读取；CLI 尚有不受它约束的网络路径——Rust 侧布局资产下载（`ensure_layout_assets`，当前未接入 CLI、仅测试调用）与经 `OCRX_TABLE_ENGINE_CMD` 启动的 Python 表格引擎。v0.1 的工具参数触达不到它们，保证当前成立；但 P1 暴露 `layoutRegions` 或默认扫描接入布局路径的任一落地都会打破该保证——**此前必须先落地 §13-C9 与 §14.3 的零网络请求门槛**。
2. **`ocrx_model_install` 是唯一下载入口**，且自带同意门（§7.5.3）；`enableModelInstall: false` 时该工具不注册。
3. 该保证由插件自身实现，不依赖 DSH 审批预设。

### 11.4 敏感信息

`password` 指**加密 PDF 的打开密码**（CLI 既有参数 `--password`），由用户在对话中告知 Agent 后经工具参数传入——因此它本就会随用户消息进入会话记录，工具参数是第二处记录点。该参数会随工具调用进入**会话记录与模型上下文**，DSH 内置客户端不消费呈现卡片，插件无脱敏手段。不改为经 Config 传入（Config 同样被记录，且破坏零配置漏斗），也不移除（加密 PDF 场景将无工具可用）。处理方式：工具描述与 README 安全章节如实声明；建议高敏感文档用户自行权衡（离线自行调用 CLI 或接受该暴露面）。删除"不入日志/不回显"的一切承诺。

### 11.5 npm 供应链

1. 平台包与 GitHub Release 同源（§5.1）：CPU 变体同一 CI 构建、同一文件集，包内 SHA256SUMS 随附并在安装冒烟中校验。
2. 禁止 postinstall 脚本；许可文件随包完整分发（llama.cpp、PDFium、MS VC 运行库）。
3. 信任模型如实声明：DSH 插件机制不是恶意包沙箱；结合 §11.1，安装本插件 ≈ 授予 ocrx 以用户权限运行。

## 12. 兼容性与版本策略

1. 跨进程契约 = CLI **实际**稳定面：CLI 参数、`result.json`（schema v3）、失败对象（schema v1）、退出码。进度 JSONL 不在契约内（插件以 `--progress off` 执行，不消费进度）。`ResultReport` 格式化器是死代码，不作为契约。插件对未知字段宽容。
2. 插件与平台子包版本同节奏发版；README 维护兼容矩阵（插件 ↔ `ocrx` 最低版本 ↔ `engines.dsh` 冻结窗口）。
3. **`engines.dsh` 冻结规则**：支持范围 = CI 实测窗口（初始 `>=0.2.2-rc.5 <0.2.3-0`，上界 `-0` 排除 `0.2.3-alpha.x`）；窗口扩大必须伴随该版本的 CI 验证，随插件发版承载。不做宽范围 + 无矩阵的"范围内保证可用"承诺。
4. 发布时间窗：pnpm `minimumReleaseAge`（11 默认 24 小时）可能延迟新版本可安装性；公告与可安装之间留窗，实现期验证。
5. 上游破坏性变更（`docs/upgrade-guide/` 多处涉及插件面）由"窗口冻结 + 逐版本验证"消化，不依赖语义化版本推断。

## 13. CLI 侧配套小扩展

为支撑分发目标，`ocrx` 仓库需要以下配套改动（均不改变既有扫描语义与退出码）：

| # | 扩展 | 目的 | 优先级 |
| --- | --- | --- | --- |
| C1 | `skill install` 增加 `dsh` 目标 | 非插件路线的对齐安装 | P1 |
| C2 | **机器可读安装计划与实时进度**：按 model×profile 输出字节数、revision、**目标目录（定义到 revision 目录——缓存布局中 `model.gguf` 在 profile 目录、共享 `mmproj.gguf` 及其暂存在上一级的 revision 目录；`target_dir` 只给 profile 目录会漏掉 mmproj 一侧约 205 MB）**（供 `ocrx_model` 与 `readiness` 使用，插件不得硬编码目录表）；`model install` 进度逐行实时写出（含阶段、字节、速率、校验）；下载传输增加超时与重试上限；**安装开始前清理陈旧 `.part-*`，覆盖 profile 与 revision 两个目录**，并为 `model install` 增加终止信号处理（尽力删除自身 `.part-*`）；**为 `model install` 增加结构化失败输出**（schema v1 失败对象写 stdout，对齐单文档 scan 错误契约，kind 区分 `transport` / `hash` / `lock` / `offline` / `disk`） | 同意文案的真实数据 + 安装可观测 + 取消残留的确定性回收 + 安装失败的确定错误归属 | **P0（随 install 工具）** |
| C3 | 版本数据源：`ocrx version` 接入 `CARGO_PKG_VERSION`（现为硬编码 `0.1.0`），doctor JSON 增加版本字段 | `/ocrx` 版本偏差报告 | **P0** |
| C4 | 发布流水线：CI 产物作为 Release 归档与 npm 平台包的**唯一来源**；npm 包按 §5.1 文件集打包（Windows 含 CRT DLL 与 ms-vc-runtime 许可，macOS `bin/ + lib/` 布局并补齐许可）；CUDA 变体的发布路径单独说明（本地工具链构建，不作同源声明） | 同源分发 | **P0** |
| C5 | `scan` 尊重 `OCRX_MODEL_CACHE`（当前 `scan` 走默认缓存根，与 doctor/model 不一致） | 消除缓存目录双标；落地后 Config 才可恢复 `modelCacheDir` | P1 |
| C6 | `assets/skills/ocrx/SKILL.md` 内容更新至当前默认模型（OvisOCR3）与实际行为 | Skill 对齐的事实源 | P1 |
| C7 | 模型下载字节级续传（curl `--continue-at` + `.part` 保留策略） | 安装可靠性 | P2/待决策 |
| C8 | 模型下载镜像端点透传（HF 镜像环境变量） | 国内网络下的自举成功率 | P1/待决策 |
| C9 | `--offline` 约束扩展到布局资产下载（`ensure_layout_assets` 接入安装策略检查），保证任意 scan 参数组合下 `--offline` 即零网络请求 | 恢复"恒 offline 即零下载"的全量保证；P1 暴露 `layoutRegions` 或默认扫描接入布局路径的**前置条件** | P1（先于 `layoutRegions` 落地） |

## 14. 测试与验收

### 14.1 测试

1. 单元（不依赖真实 CLI）：参数→argv 映射（含恒 `--offline`）、互斥校验、**按三种 stdout 形态**的归一化提取（含批量部分失败的 `documents[]` 表达）、**失败对象解析按流分流：单文档经 stdout（成功/失败形态判别）、批量经 stderr（多文档、多失败原因）**、退出码→reason 映射（含 `5/output`、`3/pdf_native`、表内 code 未知 kind → `cli_error`（scan 路径）/ `install_failed`（install 路径例外）、表内 code 无失败对象 → 按 code 默认 + `details_unavailable`）、`readiness` 生成、二进制定位顺序与 `launcher_failed` 检测（fake binary 桩）、同意门（`ctx.get('approval')` 三分支：`rejected`/`unavailable` → `consent_denied` + 离线指引；`cancelled` → `consent_cancelled`）、`.part` 残留提示与（桩验证）C2 清理调用、`ocrx_model` / `ocrx_model_install` 参数与 argv 映射（profile 缺省填充规则、显式 model 透传、model 子命令禁用旗标）、install 失败映射桩（offline / 磁盘不足 / 传输 / 哈希 → 规定结果；传输与哈希失败不自动重试、不再发起下载同意）、自举目标继承四组合矩阵（双显式沿用；仅 model 经 `recommend --model`；仅 profile = CLI 默认模型 + 该 profile、不经无参数 recommend；均缺省经无参数 recommend）。
2. 集成（真实 `ocrx`）：小 PDF 端到端；纯文本文档零模型成功；模型缺失 → `model_missing` → 安装 → 重试成功完整链路（小体积测试模型或桩下载）；doctor JSON 解析；批量含失败文档的逐文档推导；**批量自举重试链路**：多文档/目录在空模型缓存下首次扫描失败 → 同意安装 → 自动重试成功（断言第二次调用未被退出码 9 / `scan-summary.json` 拦截、既有成功文档按 resume 语义复用）；显式非默认 model/profile（ovisocr2/balanced）缺失时安装计划、同意文案、install argv 与重试 scan 全链路保持该目标；两个混合组合（仅 model；仅 profile）在**无参数 recommend 会跨到另一模型内存档位**的受控环境（低内存桩）下，readiness/安装计划/同意文案/install argv/重试 scan 始终等于原 scan 的解析目标。
3. 插件级：DSH（锁定版本）装载插件，验证工具/命令注册、Config 校验、卸载回滚、无平台包时的 `binary_not_found` 指引；无审批服务的部署中插件正常加载（§14.2）。验证清单含 PTC（经 `run_code` 调用工具）路径下 `ctx.get('approval')` 之 `request` 的行为（未核实项）。
4. CI：Windows x64 + macOS arm64 矩阵；锁定 `@deepseek-ai/dsh@0.2.2-rc.5`；`npm pack` 后 `dsh plugin add <tarball>` 安装冒烟 + 首次扫描漏斗冒烟；`engines.dsh` 验证接受 `0.2.2-rc.5`、拒绝真实上沿 `0.2.3-alpha.1`（与 §14.2 一致）。

### 14.2 功能验收

**漏斗验收（核心）**

- 干净环境模拟（无 ocrx、无模型、无配置；Node + pnpm 前提满足）：`dsh plugin add` 一条命令启用；`/ocrx` 输出状态总览文本，指出模型未装与推荐档位。
- **默认配置、不改任何 Config** 完成一次真实自举（同意 → 下载 → 安装成功）。
- 首次扫描在 **≤ 2 轮对话**内完成：提出需求 → 同轮收到含真实体积/目录/"中断需重新下载且暂存占盘"提示的同意询问 → 同意 → 安装完成并自动重试 → 文本摘要 + 产物路径；期间无人工终端命令、无需阅读文档、无任何未经同意的下载。
- 同意被拒或审批不可得（`rejected`：用户拒绝或 `never` 策略；`unavailable`：服务缺席/请求异常）：`ocrx_model_install` 返回 `consent_denied` 结构化结果与离线放置指引（插件生成文案，非管线模板），不重试、不静默下载。
- 无审批服务的部署：插件**正常加载**，`ocrx_scan` 等工具可用；`ocrx_model_install` 返回 `consent_denied`（`unavailable` 归入同一指引路径），不影响其他工具。
- 纯文本文档在模型未装时直接成功，全程不触发下载询问。
- 单文档模型缺失（首次扫描单个 PDF 的主路径）：`model_missing` 经 **stdout** 失败对象推导，自举引导成立；不误报为 `input`。
- 批量含模型缺失：`model_missing` 经 **stderr** 逐文档失败对象推导，不误报为 `input`。
- **批量自举重试**：多文档/目录空模型缓存首次失败（逐文档 `model_missing`）→ 同意安装 → 自动重试成功；第二次调用未被退出码 9 / `scan-summary.json` 拦截，既有成功文档按 `resume` 语义复用；原 `overwrite: true` 调用的重试仅带 `resume`（互斥不违反）。
- **install 失败映射**：桩注入 offline / 磁盘不足 / 传输失败 / 哈希失败，分别得到 `install_blocked`（附可区分原文）/ `install_failed`；传输与哈希失败后无自动重试、无再次下载同意。
- **目标继承四组合**：双显式（`ovisocr2` + `balanced`）缺失时 readiness、同意文案、install argv、重试 scan 均保持该对值；仅显式 `model: "ovisocr2"` 时经 `recommend --model ovisocr2` 解析且 model 不变；仅显式 `modelProfile: "balanced"` 时目标为 **CLI 默认模型 + balanced**——即使在无参数 recommend 会跨到另一模型档位的低内存受控环境下也不偏离；二者均缺省时采用无参数 recommend 的实际返回；零参数安装路径产生 CLI 必需的 profile。
- 批量两文档分别因模型缺失与输入错误失败：`documents[]` 各自带正确 `reason`，顶层 `error` 取首个失败文档原因，可完整表达批量部分失败。
- `offline` 或用户拒绝下载：返回离线放置指引，不重试下载。
- `enableModelInstall: false` 时 Agent 无任何可发起下载的工具。
- Linux 宿主：**安装期被明确拒绝**（`UnsupportedBundlePlatformError` 提示），工具不注册；不以"工具仍注册"为验收。
- 卸载 `dsh plugin` remove 后**插件自身**无残留文件；模型缓存按范围限定保留（与直装 CLI 共享，卸载不删除，§5.3.3）。

**工具与契约验收**

- `engines.dsh` 冻结窗口内装载可用：`0.2.2-rc.5` 被接受；真实上沿 `0.2.3-alpha.1` 被拒绝并给出支持范围。
- 无平台包匹配（同平台架构不匹配）时工具仍注册，调用返回 `binary_not_found` 指引。
- `ocrx_scan` 合法调用：产物目录含 Markdown 与 `result.json`；返回归一化摘要（单文档取自 stdout v3；多文档逐文档 exit_code + 读取各 `result.json` 补齐页数/warnings），不含正文。
- 退出码 10 时 `ok: true` + `partial: true` + 逐文档失败原因。
- `resume`/`overwrite` 同给被工具层拒绝；`overwrite` 工具描述不承诺清理旧产物。
- Windows CPU 包上 `device: cuda` 被**工具层拒绝**（`device_unavailable`），不启动进程。
- scan 路径写 `result.json` 失败（桩注入，退出码 5 / kind `output`）映射为 `output`，不误判为 `install_blocked`；`pdfMode: "text"` 遇非可靠文本页（退出码 3 / kind `pdf_native`）映射为 `pdf_native` 并建议 `auto`/`ocr`。
- sleep 桩触发超时：进程树被终止，返回 `timeout` reason、已等待时长与 resume 提示；**不包含进度尾部断言**（真实 CLI 的进度在进程结束时才写出）。
- 插件取消：ocrx 进程终止（Windows 进程树；macOS 按降级路径以主进程终止为准），返回 `interrupted` reason + resume 提示。
- `ocrx_model_install` 取消后：结果**如实报告 `.part-<role>` 残留占盘**（C2 落地后含目录与体积，并提示下次安装开始时自动清理）；（依赖 C2，真机验收）取消后再次安装，安装开始时陈旧 `.part-*` 被清理且安装成功。
- Config schema 非法值在装载期拒绝；`timeoutMs` 语义符合 §7.3.6（管线 4h 上限、execute 内 30 分钟默认）。
- 版本可见（依赖 C3）：`/ocrx` 与 doctor 报告真实 ocrx 版本、插件版本、二进制来源。

**分发验收**

- 平台包 SHA256SUMS 校验通过；Windows 包文件集与同 tag Release 归档 CPU 部分一致（含三个 CRT DLL 与 ms-vc-runtime 许可）；macOS 包为 `bin/ + lib/` 布局且许可文件齐全。
- 干净 profile `npm pack → dsh plugin add <tarball>` 冒烟通过，零手改配置；发布 24 小时后（`minimumReleaseAge` 窗口外）经 npm 名安装成功。

### 14.3 发布门槛

1. Windows x64（CPU）与 macOS arm64（Metal）真机各完成完整漏斗一次（含真实模型下载自举与同意流），产物与直连 CLI 一致（diff 通过）。
2. 干净 profile `npm pack → dsh plugin add <tarball>` 冒烟通过。
3. npm 包发布成功且 keywords/topic/README 上手块就位（可发现性三层之 1、2）；自托管 index 不作为发布门槛。
4. **零网络请求回归门槛**：随包 CLI 在插件全部工具 argv（含 `--offline`）下对典型输入完成扫描，过程零网络请求（断网或进程级网络监控验证）；P1 暴露 `layoutRegions` 或默认扫描接入布局路径时**必须重跑**本门槛（前置：§13-C9）。

## 15. 实施优先级

| 阶段 | 内容 |
| --- | --- |
| P0 | npm 包矩阵与平台二进制包（§5.1）；CLI 配套 C2/C3/C4；插件骨架与四工具（scan/doctor/model/model_install）；同意门与恒 `--offline` 策略；就绪度推导自举流；`/ocrx` 命令（无 install-model）；错误映射（含批量推导与兜底）；文本结果交付；测试与 CI；npm 发布 |
| P1 | 自托管 market index + 图标；SkillProvider 供给细化；C1（skill install dsh 目标）、C5（scan 读缓存变量）、C6（SKILL.md 更新）、C8（镜像透传）、**C9（`--offline` 约束布局资产，先于 `layoutRegions` 落地）**；`keepPages`/`layoutRegions` 参数扩展（落地前重跑 §14.3 零网络请求门槛）；CUDA 变体分发方案 |
| P2 | 专用结果卡片（独立 Client 插件）；后台任务与流式进度；`ctx.sandbox.confine` 可选沙箱模式；C7（模型下载续传）；MCP server 形态（形态 B）；轻量更新检查；`ctx.ocr` seam（形态 E） |

## 16. 待决策事项

1. 插件与平台包命名：`ocrx-dsh` + `@ocrx/ocrx-<platform>` 占位（npm 查询未被占用）；scope 取舍影响 npm 检索与 README，**阻塞一切发布动作**。
2. CUDA 变体：独立平台包（体积/驱动依赖，且 CUDA 为本地工具链构建、不在 CI 同源范围内）还是保持 GitHub Release + `Config.command` 路线；不阻塞 v0.1。
3. macOS 签名与公证：现无 codesign/notarization 步骤，npm 分发的未签名二进制与 dylib 在 Gatekeeper 下的实际行为需真机验证；是否补签名流程待决策。
4. 自托管 market index 的托管位置与维护成本（GitHub Pages / 独立域），与 §5.2.3 的 P1 排期。
5. 模型下载镜像（C8）：环境变量命名与默认值；影响国内用户首次自举成功率，建议 P1 尽早定。
6. `ctx.sandbox.confine` 可选沙箱模式：若提供，模型缓存与安装需走升级审批、Windows ACL 不支持额外可写根——价值与摩擦需一起评估。
7. 模型下载续传（C7）：`.part` 保留策略与磁盘占用上限。
8. 轻量更新检查：`/ocrx` 是否查询 npm registry 提示新版本（仅用户触发、不上报本机信息）；默认不做静默检查。
9. `engines.dsh` 窗口演进节奏：跟随 npm `latest` 线还是跟随上游稳定 tag；每次扩窗的 CI 矩阵成本。（原第 9 项"user scope Skill 的 `$DSH_HOME` 解析"已结案：未设置或空白时回落 `~/.dsh`，见 §10.3。）

## 17. 与现有需求的关系

- `requirements-v0.1.md`：本需求是其"Agent Skill"与"稳定机器可读输出"章节向 DSH 生态的**分发延伸**；CLI 契约复用以实际输出面为准（§12.1），新增仅限 §13 配套扩展。
- `pdf-content-aware-processing-v0.2.md`：`pdfMode` 参数透传通道；纯文本文档零模型路径是自举流"不做预检"设计的直接受益者。
- `regression-workbench-v0.1.md`：漏斗端到端验收取材来源。

## 18. 参考依据与核对基线

**DSH 事实基线**：npm 发布线 `@deepseek-ai/dsh@0.2.2-rc.5`（`latest`），对应 §6.2 初始 `engines.dsh` 窗口，重审已在该 tag 上逐条复核本需求引用的 DSH 事实成立；本地检出快照（`0.2.3-alpha.2` @ `4b399c39`）仅用于定位（其行号与 rc.5 个别条目略有出入，如 `docs/subsystems/sandbox.md` 为 195 行），API 事实以锁定发布版本为准，不在移动的 master 上引用。评审（`docs/review/dsh-plugin-v0.1-requirement-review-2026-11-26.md`）的关键行号依据：工具门兜底 `allow`（`packages/core/tools/src/pipeline.ts:202`）、`ask`/审批契约（`packages/core/tools/src/contracts.ts:375-379`、`docs/subsystems/approval.md`）、命令在 turn 外执行（`docs/subsystems/commands.md:173`）、`engines` 校验与 prerelease 语义（`packages/boot/app-boot/src/plugin-compatibility.ts:52-61`）、平台拒绝（`packages/boot/app-boot/src/bundle-platform.ts:28-34`）、沙箱为调用方显式 confine（`docs/subsystems/sandbox.md:164`）、无 `timeoutMs` 不设上限（`packages/guard/timeout-policy/src/index.ts:59`）、`timeoutMs` 为注册期静态值（`contracts.ts:70`）、`isConcurrencySafe` 为函数（`contracts.ts:84`）、`cwd` 必填（`docs/subsystems/subprocess.md:93`）、macOS 进程树降级（`packages/subprocess/subprocess-local/README.md:86`）、内置客户端不消费呈现卡片（`docs/cookbook/adding-a-tool.md:96`）、Skill 需 registerProvider（`packages/skill/skill-office/src/index.ts:110`）、market 自托管与 index format（`docs/user/guide/plugin-market.md`）、生成器 64 MiB 上限（`apps/plugin-market-generator/src/cli.ts:80`）、pnpm 透传与前提（`apps/cli/src/plugin.ts:99`、`apps/cli/src/args.ts:194`）、`minimumReleaseAge`（`apps/desktop/packaging/README.md:245`）、官方 bundle 清单编译进安装包（`packages/util/official-bundles/README.md:82`）、平台包先例（`native/system/packages/entry/package.json:40-45`、`native/system/docs/packaging.md:16`、`packages/fs/tool-fs-search/package.json:68`）。

**ocrx 事实基线**：行号经工作树 `feat/m43-table-rescue` @ `dcf925b` 核对（提交号仅作参考，工作树前进不影响所引事实，除非对应逻辑变更；所引逻辑与 `main` 一致）：scan 静默下载策略（`src/ocrx-cli/src/main.rs:1813-1817`）、确认分支仅非 interactive（`src/ocrx-models/src/install.rs:80`）、多文档退出码聚合（`main.rs:2124-2134`）、逐页失败进 10（`src/ocrx-scan/src/window.rs:1019-1061`）、PDFium 缺失走 3/kind=pdf（`main.rs:1896-1911`）、install 的 5 仅 offline/磁盘（`main.rs:1279-1293`）、失败对象 schema（`src/ocrx-output/src/lib.rs:272`）、result.json v3 与多文档聚合输出（`main.rs:1939-1946, 2020-2027`）、进度 JSONL 结束时统一写出（`main.rs:1840-1855`）、`ResultReport` 死代码（`main.rs:904, 4368-4372`）、下载无续传（`src/ocrx-models/src/transport.rs:41-59`、`src/ocrx-models/src/install.rs:121-162`）、doctor 无体积/revision/目录（`main.rs:2604-2612`）、model 子命令旗标受限（`main.rs:976-1007`）、scan 不读 `OCRX_MODEL_CACHE`（`main.rs:1796`、`src/ocrx-cli/src/compose.rs:40`）、版本硬编码（`main.rs:1063`）、`--overwrite` 不清理（`src/ocrx-output/src/lib.rs:2954`、`main.rs:1768`）、Windows 打包含 CRT DLL（`scripts/package-windows-x64.ps1:212-223`）、macOS PDFium 查找布局（`main.rs:1537-1546`）、CUDA 为本地构建（`.github/workflows/release.yml:137-148`）、默认下载体积（`src/ocrx-models/src/builtin/ovisocr3.rs:66`）。

**既有需求**：`docs/requirement/requirements-v0.1.md`（CLI 命令、输出规格、退出码、Agent Skill、安装与更新）、`pdf-content-aware-processing-v0.2.md`、`regression-workbench-v0.1.md`。
