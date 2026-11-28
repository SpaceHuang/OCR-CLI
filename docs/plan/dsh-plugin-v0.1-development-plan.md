# OCRX DSH 插件 v0.1 详细开发计划

状态：定稿基线（待开发；2026-11-26 评审通过：初评 BLOCK×5 → 修订 → 重审 BLOCK×2 → 修订 → 复审 PASS，通过轮按约定未新增报告。评审与修订依据见下方“修订”行及 `docs/review/dsh-plugin-v0.1-development-plan-review-2026-11-26.md`、`-re-review.md`）
日期：2026-11-26
修订：同日依评审 `docs/review/dsh-plugin-v0.1-development-plan-review-2026-11-26.md`（BLOCK×5）修订——批量失败对象 FIFO 关联与数量不匹配兜底（阶段 4）、安装最终进度统计交付面（D-5/阶段 2/阶段 6）、跨平台 artifact 汇聚与 npm 认证配置（阶段 8）、版本准备与 tag/npm 冲突预检（runbook）；同日重审 `docs/review/dsh-plugin-v0.1-development-plan-review-2026-11-26-re-review.md`（BLOCK×2）修订——批量配对对 130 取消路径的保守算法（阶段 4）、发布预检的 tag 存在性断言与首跑/重跑分离（阶段 8/runbook）、`stats.download.downloaded` 字段统一与旧状态清理。
依据：`docs/requirement/dsh-plugin-v0.1.md`（六轮评审共识基线，2026-11-26 PASS）
迭代范围：需求 §15 **P0 全量**——npm 包矩阵与平台二进制包（需求 §5.1）、CLI 配套 C2/C3/C4（需求 §13）、插件骨架与四工具、同意门与恒 `--offline` 策略、就绪度自举流、`/ocrx` 命令、错误映射、文本结果交付、测试与 CI、npm 发布。P1/P2 仅列 backlog 与前置约束（§10）。
代码基线：`feat/m43-table-rescue` 工作树（事实与 `main` 一致，行号以需求 §18 核对基线为准；本计划引用的行号经 2026-11-26 复核）。

---

## 1. 迭代结果（完成后仓库与生态中存在什么）

1. **仓库新增 `plugins/` 半区**（与 Rust workspace、`sidecar/` 平行的第三种工程）：
   - `plugins/ocrx-dsh/`：DSH 插件源码（TypeScript → `lib/index.js`，零运行时依赖），含 manifest、`cordis.patch.yml`、Config schema、四工具、`/ocrx` 命令、SkillProvider、单测与 fake binary 测试设施。
   - `plugins/npm/`：两个平台二进制包的 `package.json` 模板与打包器（CI 从 Release 归档同一暂存目录生成 tgz，不入库源码）。
2. **CLI 配套落地**（不改变既有扫描语义与退出码，需求 §13 前提）：
   - C3：`ocrx version` 输出真实版本；doctor JSON 增加版本字段。
   - C2：`model recommend --report json` 输出机器可读安装计划；`model install` 实时进度、传输超时/停滞检测、安装前陈旧 `.part-*` 清理、终止信号尽力清理、结构化失败对象（schema v1 写 stdout）。
   - C4：CI 产物成为 Release 归档与 npm 平台包的唯一来源；macOS 归档补齐许可文件。
3. **CI 扩展**：插件单测/集成（Windows x64 + macOS arm64 矩阵）、DSH 装载冒烟（锁定 `@deepseek-ai/dsh@0.2.2-rc.5`）、npm pack/发布流水线。
4. **npm 上存在三个包**（命名已结案 D-1）：`@ocrx/dsh`、`@ocrx/ocrx-win32-x64`、`@ocrx/ocrx-darwin-arm64`；DSH 用户 `dsh plugin --profile <name> add @ocrx/dsh` 一条命令完成安装，首次扫描漏斗（≤ 2 轮对话）成立。

## 2. 严格范围

### 2.1 本迭代必须完成

对应需求 §3.1，逐条落地：

1. 插件包：manifest、bundle patch、入口、Config schema、`/ocrx` 命令、Skill provider（需求 §6、§8.3、§10）。
2. npm 平台二进制包（Windows x64、macOS arm64）：同源同文件集、`optionalDependencies` 携带、许可齐备（需求 §5.1）。
3. 四个模型工具：`ocrx_scan` / `ocrx_doctor` / `ocrx_model` / `ocrx_model_install`（需求 §7）。
4. 同意机制：`ocrx_model_install` 询问门（`ctx.get('approval')` 三分支）、审批不可得拒绝路径、`ocrx_scan` 恒 `--offline`（需求 §7.5.3、§11.3）。
5. 就绪度推导与自举流：从 scan 失败对象推导、四组合目标继承矩阵、批量 `resume: true` 内部重试（需求 §8.2）。
6. CLI 配套 C2（P0，随 install 工具）、C3（P0）、C4（P0）。
7. 错误映射全表（需求 §7.6）：按 (代码路径, code, kind) 三元组判定、流分流数据来源、批量逐文档推导、兜底。
8. 测试三层（fake binary 单测、真实 CLI 集成、DSH 装载与漏斗冒烟）与 CI 矩阵（需求 §14.1）。
9. npm 发布与可发现性三层之一、二层（keywords/topic/README 上手块；自托管 index 是 P1，不阻塞，需求 §5.2）。

### 2.2 本迭代明确不做

继承需求 §3.2 全部非目标，另加工程侧约束：

- 不做进程内集成、MCP server、Client 插件卡片、后台任务与流式进度推送、遥测、模型续传（C7）、镜像透传（C8）、`layoutRegions` 暴露。
- 不改 `ocrx scan` 语义、参数与退出码；不改 CLI 既有退出码分配（C2 的失败对象是**新增输出**，不是改码）。
- 不把 `ResultReport`/`format_cli_result`（死代码，`src/ocrx-output/src/lib.rs:24-49`、`src/ocrx-cli/src/main.rs:904` 附近，仅测试引用）纳入契约或复活它。
- 插件不引入运行时第三方依赖；不为插件引入 monorepo 级 Node 工具链（pnpm workspace、changesets 等），`plugins/` 内自洽。
- 平台包不携带模型权重、不携带 CUDA 变体（CUDA 仍是本地工具链构建 + Release 附件 + `Config.command` 路线）。

## 3. 仓库布局与新增产物

```text
OCR-CLI/
├── plugins/                          # 新增顶层目录（gitignore：plugins/*/lib、node_modules、.tgz）
│   ├── ocrx-dsh/                     # 插件源码目录（发布包名 @ocrx/dsh，D-1 已结案；目录名与包名解耦）
│   │   ├── package.json              # dsh.bundle.patch、engines.dsh、platforms、optionalDependencies（精确版本钉）
│   │   ├── cordis.patch.yml          # insert: [{ id: ocrx, name: ocrx-dsh, config: {...} }]
│   │   ├── tsconfig.json             # → CommonJS lib/index.js（DSH 以 require 装载，阶段 0 复核）
│   │   ├── README.md                 # 30 秒上手块 + pnpm 前提 + 信任模型 + 密码暴露面 + 兼容矩阵
│   │   ├── src/
│   │   │   ├── index.ts              # apply(ctx, config)：注册四工具、命令、SkillProvider
│   │   │   ├── config.ts             # Config schema、默认值、装载期校验（非法值拒绝装载）
│   │   │   ├── types.ts              # 归一化结果 schema、reason 枚举、DSH seam 最小类型
│   │   │   ├── binary.ts             # 二进制定位（config → bundled → PATH/已知位置）+ launcher_failed 检测
│   │   │   ├── run.ts                # spawn 封装：cwd/env/有界 stdout/stderr 收集/超时/取消
│   │   │   ├── argv/
│   │   │   │   ├── scan.ts           # 参数→argv（恒 --offline、恒 --non-interactive --report json --progress off、互斥校验）
│   │   │   │   ├── model.ts          # ocrx_model argv（--progress/--model-profile 禁用；--report 仅 recommend·json，D-4）
│   │   │   │   └── install.ts        # ocrx_model_install argv + profile 缺省填充
│   │   │   ├── parse/
│   │   │   │   ├── streams.ts        # stdout 形态判别（v3 字符串 / 聚合 / 失败对象）、stderr JSON 行解析
│   │   │   │   ├── result.ts         # → documents[] 归一化（多文档读各 result.json 补齐）
│   │   │   │   └── errors.ts         # (路径, code, kind) → reason 纯函数 + guidance 文案
│   │   │   ├── readiness.ts          # model_missing → readiness；四组合目标继承矩阵
│   │   │   ├── consent.ts            # ctx.get('approval') → rejected/unavailable/cancelled 分类
│   │   │   ├── residual.ts           # .part-* 残留扫描（profile + revision 两目录，体积合计）
│   │   │   ├── tools/
│   │   │   │   ├── scan.ts           # 注册：timeoutMs=maxTimeoutMs；execute 内 defaultTimeoutMs 计时
│   │   │   │   ├── doctor.ts
│   │   │   │   ├── model.ts
│   │   │   │   └── modelInstall.ts   # 同意门、安装计划、残留报告
│   │   │   ├── command.ts            # /ocrx、/ocrx doctor（无 install-model）
│   │   │   └── skill.ts              # registerProvider
│   │   ├── skills/ocrx/SKILL.md      # provider 内容（与 assets/skills/ocrx/SKILL.md 对齐 + 插件路径，见 §6 阶段 7）
│   │   └── test/
│   │       ├── fake-ocrx.mjs         # 场景化桩二进制（env OCRX_FAKE_SCENARIO 驱动）
│   │       ├── fake-wrappers/        # 跨平台包装（win .cmd / unix sh → node fake-ocrx.mjs）
│   │       ├── harness.ts            # 假 ctx（tools/commands/skills/subprocess/approval 桩）
│   │       └── *.test.ts             # 单测（node:test）
│   └── npm/
│       ├── package-win32-x64.json    # 模板：name/os/cpu/files（bin/*.exe、*.dll、SHA256SUMS、licenses/）
│       ├── package-darwin-arm64.json # 模板：bin/ocrx、lib/libpdfium.dylib、SHA256SUMS、licenses/
│       └── build-platform-packages.mjs  # 按平台调用：从本平台 dist/ocrx/ 暂存生成本平台 tgz（单平台单产物）+ 文件集断言
├── scripts/
│   └── package-macos-arm64.sh        # 修改：补 LICENSE/THIRD_PARTY_NOTICES/licenses/（C4，修 macOS 许可缺口）
└── .github/workflows/
    ├── release.yml                   # 修改：打包后追加 npm 平台包生成 + 发布门（C4）
    └── ci.yml                        # 修改：新增 plugin / dsh-loading / engines-window 作业
```

Rust 侧改动集中在 `src/ocrx-cli/src/main.rs`、`src/ocrx-models/`（C2/C3），不新增 crate。

## 4. 关键实现决策

以下决策填补需求留给实现期的空间；每条标注服务的需求条款。评审若否决某条，仅影响对应阶段任务，不动摇阶段结构。

| # | 决策 | 理由与需求依据 |
| --- | --- | --- |
| D-1 | **命名已结案（2026-11-26）**：插件 `@ocrx/dsh`、平台包 `@ocrx/ocrx-win32-x64` / `@ocrx/ocrx-darwin-arm64`；占用核查通过，决策记录见 `docs/develop/dsh-plugin-naming-decision-2026-11-26.md`；目录名 `plugins/ocrx-dsh` 与包名解耦，包名只出现在 package.json/模板中。`@ocrx` 组织已于 2026-11-26 创建（npm org 页已验证），scope 永久锁定，本决策完全生效 | 需求 §16.1 曾「阻塞一切发布动作」；插件进 scope 使组织令牌从首次发布起覆盖全部三包，CI 首发即全自动 |
| D-2 | **插件语言与工具链**：TypeScript + `tsc` 编译为 CommonJS `lib/index.js`；测试用 Node 内置 `node:test` + `node:assert`；**零运行时依赖**；devDependencies 仅 `typescript`、`@types/node`、`@deepseek-ai/dsh@0.2.2-rc.5`（类型与装载测试宿主）。若 dsh 包不带类型（阶段 0 复核），在 `src/dsh-seams.d.ts` 手写最小接口并在 CI 装载测试中实测校验 | 需求 §5.1「零或最少第三方依赖」、§6.1；DSH 装载机制以 require 为主，CJS 最稳；不引入 jest/vitest |
| D-3 | **平台包版本与钉法**：平台包版本 = 所携带 ocrx 版本（workspace 版本）；插件 `optionalDependencies` 用**精确版本**（无 `^`），三包同一次发布动作发版；插件自身版本独立演进但 README 兼容矩阵声明（插件 ↔ ocrx 最低版本 ↔ engines.dsh 窗口） | 需求 §5.1「版本与插件同节奏」、§12.2；精确钉保证「同一 CI 构建」配对不被 pnpm 漂移解成不同构建 |
| D-4 | **C2 机器可读安装计划的 CLI 表面**：扩展 `model recommend` 接受 `--report json`（当前 model 子命令拒绝未知旗标，`src/ocrx-cli/src/main.rs:1033`），stdout 输出安装计划 JSON；**不新增子命令**、不动 `list/info/verify` 的文本输出。计划 schema（首版 `schema_version: 1`）：`{ schema_version, action: "recommend", model, profile, default_model, revision, size_bytes, target_dir, assets: [{ role, path, size_bytes }], installed, cache_root }`。`target_dir` = **revision 目录**（`<root>/ocrx/models/<model>/<rev>`），`assets` 分别给出 profile 目录内 `model.gguf` 与 revision 目录内 `mmproj.gguf` 的路径与字节，覆盖 mmproj 一侧约 205 MB（需求 §13-C2 明确要求）；`default_model` 服务「仅 profile 显式」组合（§7.5.3 矩阵），**插件不得硬编码默认模型**。**与需求 §7.5.2「禁止使用 `--report`」的关系**：该禁令的依据是「CLI 对 model 子命令一律报 unsupported model flag」——即 C2 前的 CLI 现状；C2 与插件同发布（P0），落地后 `recommend`/`install`（D-5）**仅这两个子命令**接受 `--report json`，`list/info/verify` 与 `--progress`/`--model-profile` 的禁令维持不变。插件仅在 `recommend`/`install` 上携带该旗标，旧二进制上的回退见 D-11 | 复用既有 recommend 解析逻辑（`main.rs:1131-1150`、`src/ocrx-models/src/lib.rs:225-245`）不另造第二条解析路径；插件侧 `ocrx_model` 的 `list/info/verify` 仍走文本透传 |
| D-5 | **C2 install 输出**：`model install` 增加 `--report json`：成功时 stdout 末行输出安装摘要 JSON——`{ model, profile, revision, size_bytes, elapsed_ms, stage: "done", download: { downloaded: bool, per_asset: [{ role, bytes, elapsed_ms, avg_rate_bps }], total_bytes, avg_rate_bps }, verify: { per_asset: [{ role, sha256_ok, size_ok }] } }`（**最终结果承载进度统计**：实际传输字节、耗时/平均速率、逐资产校验结果与完成阶段，需求 §9.2；幂等安装 `downloaded: false`、字节/速率计 0、`verify` 仍如实给出；`size_bytes` 为计划总量，供同意文案对照）。失败时 stdout 输出 **schema v1 失败对象**（对齐单文档 scan 错误契约），kind ∈ `transport/hash/lock/offline/disk`；**退出码不变**（offline/磁盘=5，其余=4，`main.rs:1286-1322`）。进度逐行实时写 **stderr**（过程可见性，非统计来源）：阶段事件（lock/acquire、下载 per-asset、校验、提交）+ 下载期以 ~500ms ticker stat `.part` 文件体积推字节与速率——不解析 curl stderr（当前 `CurlTransport` 无进度回调，`src/ocrx-models/src/transport.rs:21-70`；stat 轮询对 curl/未来传输均成立）。携带 `--report json` 的合法性与旧二进制回退同 D-4/D-11 | 需求 §13-C2「进度逐行实时写出（含阶段、字节、速率、校验）」+ §9.2「进度在最终结果中呈现」+ §7.5.3「C2 落地后按其定义追加机器可读旗标」；进度与失败对象分流（stderr/stdout）与 scan 契约同构 |
| D-6 | **C2 传输加固**：curl 追加 `--connect-timeout 30 --speed-time 60 --speed-limit 10240`（连接超时 + 停滞中止）；每资产下载尝试上限 3（现有 `--retry 2` 为 curl 层，外层再封顶），超限报 `transport` | 需求 §13-C2「下载传输增加超时与重试上限」 |
| D-7 | **C2 残留清理模式**：安装获得 revision 锁后、下载开始前，清扫 profile 目录与 revision 目录两个 scope 内陈旧 `*.install-*.part-*`、`*.part`、`*.gguf*.bak`、`*.etag`（跳过本事务文件）。注意实际暂存名为 `model.gguf.install-<pid>-<x>.part-<role>`（`src/ocrx-models/src/install.rs:121-131, 393-399`），现存 `clean_transaction_residue`（`install.rs:370-391`，零调用方）的模式不匹配——**按实际模式重写并接线**，不复活旧函数签名。终止信号（Windows Ctrl/Ctrl+Break、POSIX INT/TERM）尽力删除本事务 `.part-*` 后退出 | 需求 §13-C2「安装开始前清理陈旧 .part-*，覆盖 profile 与 revision 两个目录」+「终止信号处理」；§7.5.3 残留闭环 |
| D-8 | **插件 spawn 的 cwd/env 基线**：`cwd` 取 `ctx.workingDirectory.ensure(..., { agent })`，agent 判空回落 `process.cwd()`（与官方 bash 工具一致，不抛错）；env = seam 清洗后环境 + `extraEnv`，不新增透传通道 | 需求 §7.3.1/7.3.3 |
| D-9 | **fake binary 形态**：`test/fake-ocrx.mjs`（Node 脚本）+ 平台包装器（`.cmd`/sh），使插件以「真实可执行路径」方式 spawn 它；场景由 env `OCRX_FAKE_SCENARIO` 选择（argv 必须保持干净，供 argv 映射断言）。场景枚举与需求 §14.1.1 测试项一一对应 | 单测不依赖真实 CLI（需求 §14.1.1）；env 驱动避免污染 argv 断言 |
| D-10 | **engines 窗口验证方式**：CI 用单元测试复刻 DSH 的 `semver.satisfies(range, { includePrerelease: true })` 语义，断言声明窗口 `>=0.2.2-rc.5 <0.2.3-0` 接受 `0.2.2-rc.5`、拒绝 `0.2.3-alpha.1`；不为此安装 0.2.3-alpha.1 宿主 | 需求 §14.1.4；上沿拒绝在真实宿主上不可测（要装两个 dsh 版本），语义复刻等价且可重复 |
| D-11 | **插件对旧 CLI 的降级**（`Config.command` 指向无 C2/C3 的旧二进制时）：`recommend`/`install` 携带 `--report json` 得到退出码 2 且报 unsupported model flag → **同一 execute 内去掉该旗标重试一次**（首次调用在 argv 解析期即失败、无副作用；install 的重试发生在同意门之后、同一执行上下文内）；计划字段因此缺失 → readiness 如实标注「体积待确认（约 1.0 GB，见官方文档）」（需求 §8.2.2）；doctor 版本字段缺失 → `/ocrx` 标注「版本未知（旧二进制）」；install 失败无失败对象 → 走 §7.6 C2 前过渡规则（code + stderr 文本映射）。随包二进制与插件同发布，正常路径永不触发本条 | 需求 §6.5 回退路径的服务对象；避免插件对随包二进制之外的 CLI 假设过强；重试一次无状态、不放大副作用 |
| D-12 | **测试对 real CLI 的接入**：集成测试经 `OCRX_TEST_BINARY=<path>` 注入 `target/release/ocrx`（CI 先 `cargo build --release --locked -p ocrx`）；DSH 漏斗冒烟同理以 `Config.command` 指向该二进制（发布后的真 npm 漏斗属 §8 runbook 的人工/后置验证） | CI 内无法解析未发布的平台包版本（optionalDependencies 会按设计跳过 → `binary_not_found`，这本身是另一条被测路径） |

## 5. 现状落点对照（改动在哪里）

| 现状（已核对） | 本迭代落点 |
| --- | --- |
| `ocrx version` 硬编码 `0.1.0`（`main.rs:1090`），doctor JSON 无版本字段（`main.rs:2663-2716`，模板 `2703`） | C3：改用 `env!("CARGO_PKG_VERSION")`（workspace 版本经 `src/ocrx-cli/Cargo.toml` 继承）；doctor JSON 增补 `version` 字段（schema_version 保持 2，加字段为增量，插件侧本就宽容未知字段，需求 §12.1） |
| `model recommend` 纯文本输出、model 子命令拒绝 `--report`（`main.rs:1003-1047, 1231-1240`） | C2-D4：recommend 接受 `--report json` 输出安装计划；新增 `src/ocrx-models/src/plan.rs` 承载计划构造（输入 = catalog 解析结果 + cache 布局），CLI 只做参数与序列化 |
| install 无进度、无结构化失败、无残留清扫、传输无超时（`install.rs:45-166`、`transport.rs:21-70`） | C2-D5/D6/D7：进度 ticker、`--report json` 失败对象、锁内清扫、curl 旗标与尝试上限、信号处理 |
| `clean_transaction_residue` 零调用方且模式不匹配（`install.rs:370-391`） | C2-D7 重写并接线（保留测试） |
| 扫描 stdout 契约：单文档 v3 字符串/失败对象在 stdout；多文档聚合在 stdout、逐文档失败对象在 stderr（`main.rs:1639-1647, 2020-2034, 2091-2095, 2141-2144`；`ocrx-output/src/lib.rs:150-169`） | 插件 `parse/streams.ts` 按此实现形态判别与流分流；**CLI 零改动** |
| scan `--non-interactive` 静默下载（`main.rs:1911-1915` → `ocrx-inference/src/service.rs:127-137`） | CLI 零改动；插件 `argv/scan.ts` 恒 `--offline` 封死（构造上不可关闭 + 单测钉死） |
| `scan` 组装走 `compose(None)`，不读 `OCRX_MODEL_CACHE`（`main.rs:1796, 1894`） | 本迭代不改（C5 是 P1）；插件 v0.1 不提供 `modelCacheDir` 配置（需求 §6.4 注），文档如实说明 |
| Windows 归档含 CRT DLL 与 licenses（`scripts/package-windows-x64.ps1:214-242`）；macOS 归档缺许可（`scripts/package-macos-arm64.sh` 全文无 licenses 拷贝） | C4：macOS 脚本补 `LICENSE`、`THIRD_PARTY_NOTICES.md`、`licenses/`（pdfium 许可集 + llama.cpp）；新增 `plugins/npm/build-platform-packages.mjs` 从同一 `dist/ocrx/` 暂存出包 |
| CI 无任何 Node 步骤（`.github/workflows/ci.yml`、`release.yml`） | ci.yml 新增 plugin 测试与 DSH 装载作业；release.yml 新增平台包生成 + npm 发布门 |
| `skill install` 目标仅 codex/claude/all（`src/ocrx-skill/src/lib.rs:22-27, 83-93`） | 本迭代不改（C1 是 P1）；插件 Skill 经 provider 供给不落盘（需求 §10.1） |
| SKILL.md 只提 OvisOCR2、21 行（`assets/skills/ocrx/SKILL.md`） | 本迭代不改该文件（C6 是 P1）；插件 `skills/ocrx/SKILL.md` 从第一天起按当前产品事实写（OvisOCR3 默认），两份的对齐在 C6 结案 |

## 6. 实施阶段

依赖顺序（可双泳道并行）：

```text
阶段0 ──┬─→ 阶段1(C3,Rust) ─┐
        ├─→ 阶段2(C2,Rust) ─┼─→ 阶段8(C4,流水线) ─→ 阶段9(CI+冒烟+发布门槛)
        └─→ 阶段3(插件骨架,TS) ─→ 阶段4(scan) ─→ 阶段5(doctor/model) ─→ 阶段6(install+同意门) ─→ 阶段7(命令+Skill) ─┘
```

Rust 泳道（阶段 1/2）与 TS 泳道（阶段 3-7）在阶段 0 后可并行；阶段 4 起插件测试对 C2 的依赖用「桩 JSON 按 D-4 schema 提供」解耦，真实 CLI 集成测试在两泳道合流后补齐。每项工作遵循 Red → Green → Refactor；Rust 侧合并点跑 workspace 门禁（fmt/clippy -D warnings/test，对齐 ci.yml 现状），TS 侧合并点跑 `npm run build && npm test`。

### 阶段 0：前置决策与基线 spike（≈ 0.5–1 天）

目的：把需求 §16 中阻塞发布或影响结构的未决项在写代码前收敛。

任务：

1. **命名决策（D-1，已结案 2026-11-26）**：插件 `@ocrx/dsh`、平台包 `@ocrx/ocrx-<plat>`；决策记录 `docs/develop/dsh-plugin-naming-decision-2026-11-26.md`。本项剩余动作仅一个：在 npm 创建 `@ocrx` 组织（`docs/develop/dsh-plugin-npm-setup-guide-2026-11-26.md` 第 4 步）。
   - 2026-11-26 预核查结果：registry 包名 `ocrx-dsh`、`ocrx`、`@ocrx/ocrx-win32-x64`、`@ocrx/ocrx-darwin-arm64` 及无 scope 备选 `ocrx-win32-x64`、`ocrx-darwin-arm64`、`ocrx-dsh-plugin` 全部 404（无既有包）；npm 站点 `~ocrx`（用户）与 `org/ocrx`（组织）均 404（差分对照 `org/babel` = 200）→ 占位名全套可用。**后续进展（同日）**：`@ocrx` 组织已创建并经独立验证（org 页 Packages 0），D-1 完全生效；裸名 `ocrx` 占用已决策为 TODO（npm 启动器包，见 §10 P1）。
2. **DSH 0.2.2-rc.5 事实复核 spike**：本地 `npm i @deepseek-ai/dsh@0.2.2-rc.5`，逐项核实并记录到 `docs/develop/dsh-plugin-spike-<date>.md`：
   - 插件装载形态（require CommonJS `main`；`dsh.bundle.patch` 的 insert row 字段；`cordis.patch.yml` 语法）；
   - `inject` seam 签名：`tools.register`（含 `timeoutMs` 注册期静态值、`isConcurrencySafe` 函数形态）、`commands`（turn 外直执行）、`skills.registerProvider`、`subprocess.spawn`（cwd 必填、signal、进程树语义）、`workingDirectory.ensure(...)` 的 agent 参数；
   - `ctx.get('approval')` 可选取得与 `request` 契约（返回/异常形态），**含 PTC（`run_code` 路径）下 `request` 的表现**（需求 §14.1.3 未核实项）；
   - `dsh.bundle.platforms` 拒绝路径（`UnsupportedBundlePlatformError`）与 `engines.dsh` 校验语义（作为 D-10 单测的对照样本）；
   - 官方 bash/fs 工具的 `ctx.get('approval')` 调用方式（插件仿写基准）。
3. **pnpm 行为验证**：`optionalDependencies` 平台包（os/cpu 字段）在 Windows/macOS 上的跳过行为；`minimumReleaseAge`（pnpm 11 默认 24h）对新发版本可安装性的实际影响；`allowBuilds` 门禁下无 postinstall 的装载确认。
4. **Node 工具链落地**：`plugins/ocrx-dsh/` 初始化（package.json、tsconfig、tsc 出 `lib/index.js`、node:test 跑通空测试）；确认 dsh 包类型可用性，不可用则手写 `src/dsh-seams.d.ts`。
5. **测试资产清点**：从 `test-data/` 与既有集成测试选漏斗冒烟用的小 PDF 与纯文本文档各一（纯文本零模型路径验收需要）；确认小体积模型或桩下载方案（ovisocr3 quality 唯一 profile ≈ 0.97 GB，`src/ocrx-models/src/builtin/ovisocr3.rs:50-68`——真机门槛用真实下载，CI 用桩）。

验收：spike 文档落盘且上述各项有「已核实 + 出处」结论；命名决策结案或明确挂起（挂起则阶段 8 的 publish 保持禁用）；`plugins/` 半区可构建可测试。

### 阶段 1：C3 版本数据源（≈ 0.5 天）

改动：

1. `src/ocrx-cli/src/main.rs:1090`：`println!("ocrx {}", env!("CARGO_PKG_VERSION"))`。
2. `doctor_json_report_with_paths`（`main.rs:2663-2716`）模板增 `version` 字段。

测试：`ocrx version` 输出与 `cargo metadata` 工作区版本一致（对齐 release.yml:68-87 的 tag 校验口径）；doctor JSON 含 `version` 且与前者一致；既有 doctor 测试快照更新。

验收：需求 §13-C3；`/ocrx` 与 doctor 的版本数据源就绪（下游阶段 5/7 消费）。

### 阶段 2：C2 机器可读安装计划与 install 加固（≈ 4–6 天）

按 D-4/D-5/D-6/D-7 落地，全部在 `src/ocrx-models/` + `src/ocrx-cli/src/main.rs`：

1. **安装计划（D-4）**：新增 `src/ocrx-models/src/plan.rs`——`InstallPlan::resolve(model_opt, cache_root) -> InstallPlan`，字段按 D-4 schema；复用 catalog 的按内存推荐（`src/ocrx-models/src/lib.rs:225-245`）与缓存布局（`src/ocrx-models/src/cache.rs:34-44`）；`installed` 以 `file_matches`（size+sha256，`install.rs:288-305`）判定。CLI：`parse_model_command` 允许 `recommend` 携带 `--report json`（其余子命令维持拒绝），dispatch 输出计划 JSON 到 stdout。
2. **install 结构化输出（D-5）**：`model install --report json`——成功摘要 JSON 末行（stdout），**必须包含 D-5 定义的 `download`/`verify` 进度统计**（实际传输字节、耗时/平均速率、逐资产校验结果与完成阶段；幂等安装 `downloaded: false` 计 0、校验仍如实输出）；失败对象（schema v1，`ocrx-output` `ErrorReport`）写 stdout，kind 映射：`Offline→offline`、`InsufficientDisk→disk`、传输/停滞/尝试超限→`transport`、sha256/size 校验→`hash`、`Lock Contention→lock`；退出码不变。
3. **进度 ticker（D-5）**：install 下载循环内起 ticker（~500ms stat `.part` 文件），stderr 逐行输出 `{stage, asset, bytes, total_bytes, rate_bps}` 人类可读行（非 JSON 契约，最终摘要承担机器可读）；阶段事件：`lock-acquired / download-start / download-progress / verify / commit / done`。
4. **传输加固（D-6）**：`CurlTransport` 追加连接超时与停滞旗标；`install.rs` 下载循环加每资产尝试上限（3），超限 → `transport` 失败对象。
5. **残留清扫与信号（D-7）**：锁内、下载前清扫两 scope；重写模式匹配实际暂存名；install 期间注册信号处理，尽力删除本事务 `.part-*`。
6. **锁竞争**：`lock` kind 下失败对象 message 带持有 PID（现状 `.install.lock` 已记录 PID，`install.rs:168-265`），供 guidance 提示。

测试（Rust 单测为主）：
- 计划 schema：两模型 × 已装/未装矩阵的字段正确性（ovisocr3 仅 quality profile、size 合计 ≈ 1,016,830,848 B = 811,843,712 + 204,987,136）；`target_dir` 为 revision 目录；`default_model` 与 CLI 默认一致（`main.rs:39` 冻结值）。
- 失败对象：桩 transport 注入传输错/哈希错/锁占用/offline/磁盘不足 → kind 与退出码；stdout/stderr 分流。
- 摘要统计（§9.2）：成功安装的 `download.per_asset`（字节/耗时/速率）与 `verify.per_asset`（sha256_ok/size_ok）字段齐套且数值与桩一致；**幂等安装**（预置完好文件）`downloaded: false`、字节/速率计 0、`verify` 仍完整输出。
- 清扫：预置陈旧 `model.gguf.install-<pid>-<x>.part-model` 与 `mmproj` 侧残留 → install 后消失；并发锁持有期间不清扫他人文件。
- 信号：POSIX 下发 TERM → 本事务 `.part` 被删（Windows 以单测覆盖处理函数逻辑，真机 Ctrl+C 走既有 `windows-ctrl-c-smoke.ps1` 模式补冒烟）。
- CLI 集成：`model recommend --report json` 输出可被 `serde_json` 解析；`model list/info/verify` 拒绝 `--report` 的既有行为不回归。

验收：需求 §13-C2 全部五个子项 + **§9.2 最终结果统计的 CLI 交付面**（摘要含阶段/字节/速率/校验，幂等路径取值有定义）；阶段 4/6 的插件侧消费面就绪。

### 阶段 3：插件骨架（≈ 2–3 天）

任务：

1. `package.json`：`name: "@ocrx/dsh"`、`main: lib/index.js`、`files: [lib, cordis.patch.yml, skills, README.md]`、`engines.dsh: ">=0.2.2-rc.5 <0.2.3-0"`、`dsh.bundle.platforms: ["win32","darwin"]`、`optionalDependencies`（`@ocrx/ocrx-win32-x64` / `@ocrx/ocrx-darwin-arm64` 精确版本，D-3）、`peerDependencies: @deepseek-ai/dsh*`、`publishConfig.access: "public"`（scoped 包必须显式公有）、**无 scripts.postinstall**；`cordis.patch.yml` insert row。
2. `src/index.ts`：导出 `name`、`inject: ['tools','commands','skills','subprocess','workingDirectory']`（**不含 approval**，需求 §6.3）、Config schema、`apply(ctx, config)`——本阶段仅打印装载日志，工具注册在阶段 4-6 逐个接入。
3. `src/config.ts`：Config schema 全字段（需求 §6.4 表：`command`、`extraEnv`、`offline=false`、`defaultTimeoutMs=1800000`、`maxTimeoutMs=14400000`、`installTimeoutMs?`、四个 `enable*=true`）+ 校验（类型、`defaultTimeoutMs ≤ maxTimeoutMs`、超时正整数；非法值抛错阻断装载，需求 §14.2）；`enableModelInstall: false` 时该工具不注册（§6.4）。
4. `src/binary.ts`：定位顺序 `Config.command → 随包子包 → PATH → 已知位置`（Windows `%USERPROFILE%\.local\bin\ocrx.exe`、unix `~/.local/bin/ocrx`，即 install.ps1/install.sh 目标，`scripts/install.ps1:13`、`scripts/install.sh:4`）；随包路径经 `require.resolve('@ocrx/ocrx-win32-x64/package.json')` 解析；返回 `{ path, source: 'config'|'bundled'|'path'|'known' }`；全未命中不阻断装载。`launcher_failed` 检测：spawn ENOENT 之外，识别立即退出且无输出的形态（Windows 加载器 `0xC0000135` = 3221225781）。
5. `src/run.ts`：spawn 封装——全显式 argv、cwd（D-8）、env 注入、stdout 有界收集（`maxBytes` + spill 文件）、stderr `{maxBytes, spill}` 全流、`exec.signal` 关联、execute 内计时器（到点主动终止并返回 `timeout`）。
6. 测试设施：`fake-ocrx.mjs` 场景框架 + 跨平台包装器 + 假 ctx harness（D-9）。

测试：Config 校验矩阵；定位顺序（桩目录布局，含「随包优先于 PATH」的遮蔽行为，需求 §6.5.6）；`binary_not_found` 指引文本；spawn 封装的 stdout 截断/spill 行为；装载冒烟（apply 不抛错、inject 列表正确）。

验收：需求 §6.1/§6.2/§6.3/§6.4/§6.5；DSH 装载插件成功（阶段 0 spike 宿主上手验证）。

### 阶段 4：`ocrx_scan`（≈ 4–6 天，插件核心）

任务：

1. `argv/scan.ts`：参数→argv 映射（需求 §7.2 全表）；无条件追加 `--offline --non-interactive --report json --progress off`；`resume`/`overwrite` 互斥校验（工具层拒绝，不发 argv）；`device: cuda|vulkan` 仅在 **Windows 随包平台包来源**上工具层直接拒绝（`device_unavailable`，静态能力事实，不启动进程）——`Config.command` 来源放行；PATH 来源不做静态预判，交由运行期 §7.6 退出码 6/`device_unavailable` 行映射；schema 不暴露 `offline` 参数（构造性封死）；`overwrite` 与 `password` 的工具描述如实表述（前者不清理旧产物、后者会进入会话记录与模型上下文，需求 §7.2/§7.7.3）。
2. `parse/streams.ts`：stdout 形态判别（先试 v3/聚合 JSON，失败再试失败对象；单文档失败对象就在 stdout）；stderr 全流逐行解析失败对象（批量）；有界尾部兜底保留。
3. `parse/result.ts`：归一化 `documents[]`（需求 §7.4 schema）——单文档从 v3 提取 pages/markdown/warnings/duration；多文档从聚合 + 逐文档读 `<output_dir>/result.json` 补齐。**批量失败对象与文档的关联规则（冻结，不改 scan 契约；含 130 取消路径的保守处理）**：
   - 候选 = 聚合中 `exit_code` ∉ {0,10} 的文档（按聚合顺序）；对象 = stderr 逐文档失败对象（按发出顺序）。CLI 两侧同源 FIFO——失败对象按 `outcome.documents` 顺序发出（`src/ocrx-cli/src/main.rs:1919-1935`）、聚合按同一 `doc_results` 顺序生成（`main.rs:2013-2029`）；schema v1 失败对象无 `input` 字段可依（`src/ocrx-output/src/lib.rs:252-279`）。
   - **130 是可选失败对象路径**：纯取消文档可只有 `summary.cancelled` 而 `failure = None`（`src/ocrx-scan/src/app.rs:1260-1261, 1382-1403`；CLI 只对 `failure = Some` 的文档发对象（`main.rs:1921-1935`），但 `document_exit_code` 把取消文档映射为 130（`main.rs:2074-2079`））。130 文档一律按表映射 `interrupted`，**不假定 stderr 必有对象、不主动消费对象**。
   - **配对算法（保守，宁缺勿错）**：设 N = 非 130 失败文档数（`exit_code` ∉ {0,10,130}——此类文档 `failure` 必为 `Some`，各必发且仅发一个对象），M = 对象总数。无 130 候选 → 维持 FIFO 一一配对（顺序唯一可靠），仍逐对校验 `object.code == exit_code`；有 130 候选 → 三分支：**M == N**：对象全属非 130 文档（非 130 文档必有对象、总数恰等，归属唯一），按序配对 + 逐对 code 相容校验，任一失配 → 保守降级；**M > N**：多出对象属于某个 130 文档但无法定位（归属不唯一）→ **全部不配对**——非 130 失败文档回落 code 默认 reason + `details_unavailable`，全部对象计数与原文保留在顶层 `error` 与 stderr 有界尾部，标注 `attribution_degraded`；**M < N**：必有非 130 文档缺对象（流截断类异常）→ 耗尽点起回落 `details_unavailable`，已配对部分仍须逐对相容。
   - **禁止按 code/kind 猜测配对**（两文档可同 code/kind 而不同 cause）；任何情况下不得为保持一一配对而错挂原因。
   `ok`/`partial`/顶层 `error` 取首个失败文档；不内联正文。
4. `parse/errors.ts`：reason 判定纯函数（需求 §7.6 全表 + 五条判定规则）：(路径, code, kind) 三元组、kind 优先、install 路径例外、code 表内无失败对象 → code 默认 + `details_unavailable`、表外/spawn 失败 → `launcher_failed`；guidance 文案由插件生成（按 cause 类别前缀分支，不依赖 CLI suggestion）；批量整体退出码聚合**不参与**原因推导。
5. `readiness.ts`：`model_missing` → 附 `readiness`（目标解析四组合矩阵，需求 §7.5.3：双显式沿用 / 仅 model 经 `recommend --model` / 仅 profile 取 CLI `default_model` + 该 profile / 均缺省经无参数 recommend）；C2 计划可得时带 `size_bytes`/`target_dir`，不可得时按 D-11 降级标注。
6. `tools/scan.ts`：注册（`timeoutMs = maxTimeoutMs` 注册期静态值、`isConcurrencySafe` 恒 false 函数）；execute 内 `defaultTimeoutMs` 计时 → 主动终止 → `timeout` reason + 已等时长 + resume 提示；取消经 signal → `interrupted`（插件自身状态判定，不看退出码 130）；进程树终止由 subprocess seam 承担，macOS 降级路径验收以「ocrx 主进程已终止」为准、不承诺 sidecar 级联（需求 §7.3.4）；文本结果 = 紧凑摘要 + 产物路径。

测试（全部 fake binary 场景化，对应需求 §14.1.1）：
- argv：恒 `--offline` 钉死断言、互斥拒绝、device 预判、全参数映射表。
- 归一化：三种 stdout 形态 × 成功/失败、批量部分失败（`model_missing` 与 `input` 混合 → 各自带正确 reason、顶层取首个）、退出码 10 → `ok:true, partial:true`、退出码 {0,10} 外 → `ok:false`。
- 批量关联（冻结规则的回归钉）：**两文档同 code/kind、不同 cause** → FIFO 配对各自正确（防 kind 猜测回退）；**失败对象少于失败文档** → 未配对者 `details_unavailable` 且不吞掉其退出码；**失败对象多出** → 多出对象不挂到任何文档、顶层 error 注明数量；**取消文档无对象 + 后续失败文档有对象**（`[130, 失败]`、M==N）→ 取消文档 `interrupted`、失败原因**不前移**到取消文档；**取消与文档失败并发**（130 文档带对象、M>N）→ 全部不配对、`attribution_degraded`、对象不丢失。
- 错误映射：表内每行至少一例（含 `5/output` scan 路径 vs `5` install 路径区分留给阶段 6 桩、`3/pdf`、`3/pdf_native`、`6/device_unavailable` 文本匹配、表内 code 未知 kind → `cli_error`、code 6 无失败对象 → `runtime` + `details_unavailable`、130 → `interrupted`）。
- readiness：四组合矩阵（含「无参数 recommend 会跨模型」的低内存桩场景）。
- 超时/取消：sleep 桩 + 缩小 defaultTimeoutMs → 进程终止、`timeout` reason、无进度尾部断言；AbortController → `interrupted`。
- 真实 CLI 集成（D-12）：小 PDF 端到端产物 diff、纯文本零模型成功、单文档/批量 `model_missing` 推导（空缓存目录环境变量隔离）。

验收：需求 §7.2/§7.3/§7.4/§7.6/§8.2.1-8.2.3；§14.2「工具与契约验收」中 scan 相关各条。

### 阶段 5：`ocrx_doctor` / `ocrx_model`（≈ 1–2 天）

任务：

1. `tools/doctor.ts`：argv `doctor --report json`（不追加 `--non-interactive`——doctor 无此旗标）；结果附加插件视角字段（二进制路径与来源、随包版本、插件版本、真实 ocrx 版本（C3，缺失按 D-11 标注））；工具描述如实标注 doctor 局限（`installed_profiles` 单模型、不自定义缓存目录）。
2. `argv/model.ts` + `tools/model.ts`：action 四值映射；无条件追加 `--non-interactive`；**禁止** `--progress`/`--model-profile`，`--report` 按 D-4 仅在 `recommend`（且仅 `--report json`）携带——`list/info/verify` 一律文本透传；`recommend` 解析计划 JSON 并在结果中给出体积/revision/目录（C2 计划 schema，D-4），同时保留人类可读摘要；旧二进制（退出码 2 unsupported flag）按 D-11 去旗标重试并降级为「体积待确认」文案。

测试：argv 映射（禁用旗标断言）；doctor JSON 解析与附加字段；model recommend 计划字段解析桩；真实 CLI 集成（doctor JSON 解析、model list/info/recommend/verify 透传）。

验收：需求 §7.5.1/§7.5.2；§14.2 版本可见相关条目（命令侧展示在阶段 7 收口）。

### 阶段 6：`ocrx_model_install` 与同意门（≈ 3–5 天）

任务：

1. `argv/install.ts`：argv `model install [--model <m>] [--offline(按 Config.offline)] --non-interactive [--report json(C2 后)] <profile>`；profile 缺省填充按四组合矩阵（复用 `readiness.ts` 的解析器，**不得发出缺 profile 的 argv**，无来源可得返回结构化错误）；`--offline` 与 Config.offline 的冲突在结果文案说明。
2. `consent.ts`：execute 内 `ctx.get('approval')` → 服务缺席/request 抛错/前置不满足（无 agent、不在打开 turn 内）→ `unavailable`；用户拒绝或 `never` 策略 → `rejected`；用户取消 → `cancelled`。`rejected`/`unavailable` → `consent_denied` + 插件生成的离线放置指引（手动下载 + 缓存目录说明），不重试不静默下载；`cancelled` → `consent_cancelled`。**不采用 `tools/pre-execute` ask 形态**（询问与拒绝文案是管线模板，execute 不运行，无法给结构化指引）。
3. 同意文案：真实体积 + 目标目录（C2 计划）+「中断需重新下载，暂存文件将占盘至下次安装开始时清理」。
4. `residual.ts`：安装计划 `target_dir`（revision 目录）与 profile 目录两 scope 扫描 `*.part-*`，合计体积；结果如实报告残留 + 「下次安装开始时自动清理」提示。
5. `tools/modelInstall.ts`：不注册管线 `timeoutMs`（缺省无上限，`Config.installTimeoutMs` 可选）；`isConcurrencySafe` 恒 false（长任务，需求 §7.3.5）；结果附 `installed: { model, profile }` 与 **`stats` 进度统计映射（需求 §9.2 最终结果交付面）**——CLI 成功摘要的 `download`（`downloaded`、实际字节、耗时、平均速率）与 `verify`（逐资产 sha256_ok/size_ok）原样进入工具结果 `stats` 字段，**不以解析 stderr 人类可读行作为统计来源**（实时 stderr 行仅为过程可见性）；幂等安装 `stats.download.downloaded: false` 并注明「已就绪，未发生下载」；C2 前旧二进制无摘要 → `stats` 缺省并如实标注（D-11）。错误映射 install 路径例外（未列 kind → `install_failed`；`5` + offline/磁盘文本 → `install_blocked` 附 stderr 原文保留可区分性；传输/哈希失败不自动重试、不再次发起同意）；`enableModelInstall: false` 时**不注册**。
6. 自举收口：安装成功结果明确「已就绪，可重试」；批量重试规则写进 guidance——原参数 + `resume: true`（原 `overwrite: true` 内部重试改为仅 `resume`）。

测试：同意门三分支（假 ctx 桩：resolve/拒绝/抛错/无服务）；残留扫描（预置假 `.part` 文件）；argv 填充（无来源 → 结构化错误、不发 argv）；install 失败映射桩（offline 文本/磁盘文本/传输一行文本/带 kind 失败对象四类 → `install_blocked`/`install_failed`，无自动重试断言）；**最终结果统计（§9.2）**：成功安装的工具结果含 `stats`（字节/速率/校验/阶段，取自 CLI 摘要而非 stderr 行）、幂等安装 `stats.download.downloaded:false` 且校验在位、旧二进制 `stats` 缺省标注；真实 CLI 集成（D-12）：完整自举链（空缓存 → scan `model_missing` → 桩同意 → install（桩下载或小体积受控源）→ resume 重试成功）；批量自举重试（断言第二次调用未被退出码 9/`scan-summary.json` 拦截、成功文档按 resume 复用）；四组合全链路（ovisocr2/balanced 显式、仅 model、仅 profile 低内存受控、均缺省）。

验收：需求 §7.5.3、§8.2.4、§9.2、§11.3；§14.2 漏斗验收的安装/同意/拒绝/批量/四组合各条。

### 阶段 7：`/ocrx` 命令与 SkillProvider（≈ 1–2 天）

任务：

1. `command.ts`：`/ocrx` 无参数 → 状态总览文本（插件版本、二进制版本与平台、来源 bundled/PATH/config、模型安装状态（标注 doctor 局限）、缓存根路径、工具开关、**信任模型声明**（需求 §11.1.2 原文要点））；`/ocrx doctor` → 自检 + 修复动作清单；**不提供** `/ocrx install-model`（命令在 turn 外无确认通道）。
2. `skill.ts` + `skills/ocrx/SKILL.md`：registerProvider 供给；内容以工具调用为优先路径（先 doctor、大 PDF 先 `pages` 抽样、`model_missing` 引导流含批量 resume 重试规则、resume 用法）、裸 CLI 降级说明、默认模型表述跟随当前产品（OvisOCR3）；随包分发不落盘。
3. 插件 README：30 秒上手块（含 Node.js + pnpm 前提）、keywords（`dsh-plugin`、`deepseek-harness`、`ocr`、`pdf-to-markdown`）、信任模型章节、密码暴露面声明（§11.4）、兼容矩阵、`minimumReleaseAge` 安装窗口说明。

测试：命令输出内容断言（无模型/有模型两态）；provider 注册与内容加载；卸载残留自检（插件不写 `$DSH_HOME` 之外位置——以装载测试的工作目录 diff 验证）。

验收：需求 §8.3、§10.1/§10.2、§5.2/§5.3 文档面；§14.2 卸载无残留。

### 阶段 8：C4 发布流水线（≈ 2–4 天）

任务：

1. `scripts/package-macos-arm64.sh`：补 `LICENSE`、`THIRD_PARTY_NOTICES.md`、`licenses/`（pdfium 许可集 + llama.cpp LICENSE）；SHA256SUMS 随暂存自动重生成（脚本现状已生成，确认覆盖新文件）。
2. `plugins/npm/build-platform-packages.mjs`（**按平台调用，单平台单产物**）：各平台矩阵 job 在自身 packaging 步骤后调用，输入**本平台** `dist/ocrx/` 暂存 + 版本号，只生成本平台的一个 tgz，并以不冲突的 artifact 名上传（`npm-ocrx-win32-x64` / `npm-ocrx-darwin-arm64`）——矩阵两 runner 各只有本平台文件（`release.yml:28-45,132-171`），跨平台汇聚不在此层完成；**文件集断言**——Windows：`bin/ocrx.exe`、`bin/pdfium.dll`、三个 CRT DLL、`SHA256SUMS`、`licenses/`（含 ms-vc-runtime）；macOS：`bin/ocrx`、`lib/libpdfium.dylib`、`SHA256SUMS`、`licenses/`；**断言包内 SHA256SUMS 与本平台 Release 归档一致**（同源证明）；断言单包 ≤ 64 MiB。
3. `release.yml` 新增 `publish-npm` **汇总 job**：`needs` 全部平台矩阵 job；`actions/download-artifact` 取得两个平台的 tgz 后**复核**（文件集、SHA256SUMS 一致、体积上限、两 artifact 为不同平台），再 `npm pack` 插件并发布。**发布顺序：平台包先于插件包**（插件 `optionalDependencies` 精确钉的版本必须已存在于 registry）。**认证配置（必须有，仅有 secret 名不够）**：`actions/setup-node` 带 `registry-url: https://registry.npmjs.org`，job 级 `env.NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}` 映射自受保护 environment `npm`（`npm publish` 不会因存在 `NPM_TOKEN` 变量自动认证）；令牌不写入仓库、日志或 tarball。**发布前预检（区分首跑与重跑；真失败即终止，不产生新的发布）**：`npm whoami` 认证有效且 token 对 `@ocrx` scope 具发布权限（`npm access` 复核）；**tag 存在**（本 job 由该 tag 触发，`release.yml:15-18`）且 == workspace 版本 == 平台包版本、指向本次 `github.sha`——「tag 尚不存在」的检查属于 runbook 打 tag **之前**的步骤，不作为发布任务条件；npm 版本检查分两形态——**首跑**（三个目标版本均 registry 404）→ 全量发布，**重跑**（部分已发布，如中断恢复）→ 已发布者经 tar 内容与版本一致性校验后跳过、仅补发缺失者，不得用「均未发布」阻断合法断点恢复；插件 `optionalDependencies` 版本 == 本次平台包版本；三包 `publishConfig.access: "public"`（scoped 包缺省 restricted）与 `repository`/`homepage`/`bugs`/`license`/`keywords`/`description` 元数据、`LICENSE` 随包。CUDA 路线在 workflow 注释与 README 单独说明（本地构建、不同源声明）。
4. 发布 runbook（`docs/develop/dsh-plugin-release-runbook.md`）：仓外前置确认（见下条）→ tag → CI → npm publish（手动门）→ 24h `minimumReleaseAge` 窗口说明 → 发布后抽验（干净 profile `dsh plugin add` + 漏斗 + `dsh plugin remove` 无残留）。
5. **npm 侧仓外前置（人为动作，阶段 0 启动、发布门前必须完成；逐步操作指南见 `docs/develop/dsh-plugin-npm-setup-guide-2026-11-26.md`）**：
   - npm 账号（邮箱已验证、开启 2FA）；若命名决策采用 scope（如 `@ocrx`），在 npmjs.com 创建对应 **organization**（公有包免费），确认 org 名可用；
   - 创建 **Granular Access Token**：仅授权三个目标包的 publish 权限（不用全权限 classic token），设定到期时间与轮换节奏；granular token 的 publish 不需要 OTP，2FA 不阻断 CI；
   - GitHub 仓库设置：创建 environment `npm`（配 required reviewers 作为手动门），`NPM_TOKEN` secret 挂在该 environment 而非 repo 级（最小权限）；
   - dist-tag 策略：三包均发 `latest`（平台包只被精确版本消费，tag 不参与解析，但 `latest` 须存在以免 npm 页面异常）；
   - 可选增强：npm **provenance**（`--provenance`，CI OIDC 签源）或 trusted publishing（免 token）——供应链可信度加分项，不阻塞 v0.1。

测试：`test-release-scripts.sh` 扩展——mjs 打包器的文件集断言自测（本地以 fixture 目录跑）；`npm pack` 与 `npm publish --dry-run` 干跑 + tar 内容清单断言；**无 postinstall 断言**（packed package.json 检查）；**workflow 结构干跑断言**——两平台 job 各自产出并以不冲突 artifact 名上传本平台 tgz（任一 runner 不得声称产出对方平台包）、`publish-npm` needs 全部平台 job 并实际下载到两个不同平台的 tgz、`setup-node`+`NODE_AUTH_TOKEN` 认证配置在位、预检步骤先于任何 `npm publish` 执行、预检含 **tag 存在性**与版本一致断言、npm 版本检查含**首跑（全 404 → 全量）与重跑（部分已发布 → 校验后跳过、补发缺失）两分支**。

验收：需求 §13-C4、§5.1、§14.2「分发验收」、§14.3.2/14.3.3。

### 阶段 9：DSH 装载、CI 矩阵与发布门槛（≈ 3–5 天）

任务：

1. ci.yml 新增作业：
   - `plugin-test`（windows + macos 矩阵）：setup Node LTS；`plugins/ocrx-dsh` 内 `npm ci && npm run build && npm test`；随后 `cargo build --release --locked -p ocrx` 并以 `OCRX_TEST_BINARY` 跑真实 CLI 集成测试。
   - `dsh-loading`（单平台起步，windows）：安装 `@deepseek-ai/dsh@0.2.2-rc.5`；`npm pack` 插件与平台包；`dsh plugin --profile ci add <插件 tarball>`；断言：工具/命令/Skill 注册、Config 校验生效、无平台包（optionalDependencies 解析失败模拟）→ 工具仍在、调用返回 `binary_not_found` 指引；`dsh plugin remove` 后无插件自身残留。漏斗冒烟以 `Config.command` 指向本地构建二进制（D-12）。
   - `engines-window`：D-10 语义复刻单测（接受 `0.2.2-rc.5`、拒绝 `0.2.3-alpha.1`）。
   - 无审批服务部署用例：装载 harness 不注入 approval → 插件正常加载、`ocrx_model_install` 返回 `consent_denied`（unavailable 归并）。
2. 真机发布门槛执行（需求 §14.3，人工 + 半自动清单）：
   - Windows x64（CPU）与 macOS arm64 各完成一次**真实漏斗**（干净环境、真实模型下载、同意流、产物与直连 CLI diff）；
   - 干净 profile `npm pack → dsh plugin add <tarball>` 冒烟；
   - npm 发布后 24h 窗口外经 npm 名安装复验；keywords/topic/README 上手块就位（GitHub 仓库打 `dsh-plugin` topic、主 README/官网加 DSH 入口）；
   - **零网络请求回归门槛**：随包 CLI 在插件全部工具 argv 下扫描典型输入，断网/进程级监控验证零请求（记录到门槛文档；P1 `layoutRegions` 落地前必须重跑，前置 §13-C9）。
3. 门槛执行记录：`docs/test/dsh-plugin-v0.1-release-gates-<date>.md`（沿用 docs/test 惯例）。

验收：需求 §14.1.3/§14.1.4、§14.2 漏斗与插件级验收、§14.3 全部门槛；发布完成。

## 7. 测试矩阵与需求条款映射

| 需求 §14.1 条目 | 落点 |
| --- | --- |
| 1 单元（fake binary） | `plugins/ocrx-dsh/test/*.test.ts`（阶段 4-6 逐项：argv/互斥/恒 offline、三形态归一化、流分流、退出码映射、readiness、定位与 launcher_failed、同意门三分支、残留提示、model/install argv、install 失败桩、四组合矩阵） |
| 2 集成（真实 CLI） | `test/integration/*.test.ts`（`OCRX_TEST_BINARY` 注入；CI `plugin-test` 作业执行） |
| 3 插件级（DSH 装载） | CI `dsh-loading` 作业 + 阶段 0 spike 宿主；PTC `run_code` 审批验证项在 spike 与本作业各记一次结论 |
| 4 CI | ci.yml 三新作业 + release.yml 发布门；engines 上沿/下沿用 D-10 单测 |

需求 §14.2 功能验收逐条分配：自动化（CI 可断言）约 80%；真机项（macOS Gatekeeper 下未签名二进制行为、两平台真实下载漏斗、卸载残留目检）归阶段 9 门槛清单人工执行并留记录。

## 8. 发布门槛与 runbook

发布顺序（一次性脚本化于 runbook，常态可重复）：

0. **仓外前置确认**（阶段 8.5 清单）：D-1 已结案（`@ocrx/dsh` 等，2026-11-26）；npm 账号 / `@ocrx` org / token 就位；GitHub environment `npm` + secret 配好。
1. Rust + 插件合流，CI 全绿（含新三作业）。
2. **版本准备提交（tag 前必做）**：当前 workspace 与 `Cargo.lock` 均为 `0.1.0` 且已存在 `v0.1.0` tag——直接打 tag 会冲突。选择**未使用**的 workspace 版本（含 C2/C3 的首发建议 `0.2.0`），同一次提交内更新：workspace 版本（`Cargo.toml`，`Cargo.lock` 随构建刷新）、插件版本与 README 兼容矩阵、插件 `optionalDependencies` 两个精确平台版本（= 新版本）；经 CI 合入 `main`。
3. 打 tag `v<ver>`，**预检失败即停**：tag 尚不存在（`git ls-remote --tags`）、tag == workspace 版本 == 平台包版本；release.yml 各平台 job 出归档 + **本平台** npm tgz（artifact 汇聚见阶段 8）。
4. 手动 promotion：`publish-npm` 预检——认证有效 / `@ocrx` 权限 / **tag 存在且三处版本一致并指向本次运行**（「tag 不存在」检查在步骤 3 打 tag 之前完成，不在此处）；npm 版本检查分形态：**首跑**（三版本均未发布 → 全量发布）与**重跑**（部分已发布 → 校验一致后跳过、仅补发缺失者）。通过后按「平台包 → `@ocrx/dsh`」发布。**部分发布失败的处理**：npm 版本不可变——修复后重跑，预检识别已发布者并跳过，仅补发未完成者；runbook 记录实际发版清单与差异原因。
5. GitHub：Release 附归档；仓库打 `dsh-plugin` topic；主 README/官网加 DSH 安装入口；runbook 登记「版本准备提交 ↔ tag ↔ 发版清单」对应关系。
6. 24h 窗口后：干净机器经 npm 名安装复验（`minimumReleaseAge`）。
7. 真机双平台漏斗 + 零网络门槛记录归档（§6 阶段 9.2）。

回滚：npm `unpublish` 72h 窗口内可用；插件与平台包同退；GitHub Release 附件可独立删除。兼容矩阵（README）随每次发版更新插件 ↔ ocrx ↔ engines.dsh 三列。

## 9. 风险与未决验证项

| 风险/未决 | 影响 | 处置 |
| --- | --- | --- |
| ~~`@ocrx` 组织未创建~~（已关闭：2026-11-26 创建并验证）；裸名 `ocrx` 在启动器包（TODO，§10 P1）落地前保持空闲 | scope 已锁定、无抢注风险；裸名落地前存在低概率被第三方抢注的窗口 | 落地即锁定（估时 +0.5–1 天）；窗口风险低，已接受 |
| PTC（run_code）路径下 approval `request` 表现未核实（需求 §14.1.3） | 同意门在 PTC 场景可能落入 `unavailable` | 阶段 0 spike + CI 各记结论；`unavailable` 分支本就安全降级（`consent_denied`，不静默下载） |
| pnpm 对 optionalDependencies os/cpu 的跳过行为差异 | 平台包未装 → `binary_not_found`（有指引，不阻断） | 阶段 0 实测两平台；CI `dsh-loading` 持续覆盖 |
| macOS 未签名二进制 Gatekeeper 行为（需求 §16.3） | 首次运行可能被拦 | 阶段 9 真机验证并记录；签名/公证为独立待决策，不阻塞 v0.1 发布门槛（如实文档化缓解） |
| `minimumReleaseAge` 24h | 公告与可安装之间有窗 | runbook 固化「公告预留窗口 + 窗口后复验」 |
| engines 窗口漂移（上游预发布高频） | 新 dsh 版本装不上插件 | 冻结窗口策略（需求 §12.3）：扩窗必经 CI 矩阵 + 插件发版；不做宽范围承诺 |
| C2 进度 stat 轮询在极端 FS 上的开销/精度 | 进度行不准 | 非契约输出（机器可读在最终摘要）；ticker 间隔 500ms 上限可调 |
| 插件与 SKILL.md 双源漂移（插件内一份、assets 一份） | 文档不一致 | 插件那份以本迭代事实写；C1/C6（P1）落 `dsh` 目标与更新 assets 版后，在 runbook 加「发版前三处版本口径核对」步骤 |

## 10. P1/P2 backlog（本计划不含，仅记前置约束）

- **P1**：**npm 启动器包 `ocrx`（裸名占用，2026-11-26 已决策、记 TODO）**：esbuild 模式——纯 JS 主包（`bin/ocrx.js` ≈60 行：定位平台二进制 → argv 全透传 → 转发退出码/信号），`optionalDependencies` 精确钉 `@ocrx/ocrx-<plat>`（复用 C4 平台包，不重发二进制），版本与平台包同节奏；发布后 `npm i -g ocrx` 即得可用 CLI（GitHub Release 路线之外的零摩擦安装通道），同时锁定裸名；估时 +0.5–1 天；若在 v0.1 发布前落地则 publish 作业发四包（平台包 → `@ocrx/dsh` + `ocrx`），否则独立小版本随后补发；自托管 market index + 图标（需求 §5.2.3、§16.4）；C1（`skill install dsh` 目标，`$DSH_HOME` 回落 `~/.dsh` 已结案）；C5（scan 读 `OCRX_MODEL_CACHE`，落地后 Config 才可恢复 `modelCacheDir`）；C6（assets SKILL.md 更新）；C8（镜像透传，尽早定）；**C9（`--offline` 约束 `ensure_layout_assets`）——`layoutRegions`/`keepPages` 暴露与默认扫描接布局路径的硬前置，且落地时必须重跑零网络门槛**。
- **P2**：Client 插件卡片、后台任务与流式进度、可选 confine 沙箱、C7 续传、MCP server、轻量更新检查、`ctx.ocr` seam（需求 §15）。

## 11. 里程碑与预估

| 里程碑 | 内容 | 预估 |
| --- | --- | --- |
| M0 | 阶段 0：决策结案 + spike 记录 + 插件半区可构建 | ≈ 1 天 |
| M1 | 阶段 1+2：C2/C3 合入 main（Rust 门禁全绿） | ≈ 5–7 天 |
| M2 | 阶段 3+4：插件骨架 + `ocrx_scan` 全量（单测 + 真实 CLI 集成） | ≈ 7–9 天 |
| M3 | 阶段 5+6+7：doctor/model/install+同意门/命令/Skill | ≈ 6–9 天 |
| M4 | 阶段 8+9：流水线 + CI + 真机门槛 + npm 发布 | ≈ 5–9 天 |

合计 ≈ **24–35 人日**（不含评审等待与真机窗口）。M1 与 M2 可并行压缩；M4 的真机门槛受设备与网络窗口制约，建议提前预约 macOS arm64 真机。

---

## 附：与需求条款的覆盖对照

| 需求章节 | 承接阶段 |
| --- | --- |
| §5 分发与可发现性 | 8（包矩阵/发布）、7（README/keywords）、9（topic/门槛） |
| §6 插件包设计 | 3（骨架）、8（平台包） |
| §7 工具设计 | 4（scan）、5（doctor/model）、6（install/同意门/错误映射 install 侧） |
| §8 漏斗 | 4（readiness/自举推导）、6（同意/重试）、7（/ocrx）、9（漏斗验收） |
| §9 长任务与进度 | 4（超时/取消）、2（C2 实时进度 + §9.2 最终统计的 CLI 面）、6（`stats` 映射到工具结果、install 无超时缺省） |
| §10 Skill | 7（provider）、P1（C1/C6） |
| §11 安全 | 4（恒 offline）、6（同意门）、7（README 声明）、9（零网络门槛） |
| §12 兼容与版本 | 3（engines/platforms）、8（兼容矩阵）、9（窗口单测） |
| §13 CLI 配套 | 1（C3）、2（C2）、8（C4）；C1/C5/C6/C8/C9 在 P1 backlog |
| §14 测试与验收 | 7（矩阵映射）、9（门槛） |
