# OCRX DSH 插件 v0.1 阶段 0 事实复核 spike

日期：2026-11-27
对象：`@deepseek-ai/dsh@0.2.2-rc.5`（npm 发布线，latest；engines `node >= 24.9.0`）
方法：完整安装 CLI 及其全部依赖（670 包）后直接检视 `node_modules` 内各包的 `src/`（未打包 TypeScript 源码）与 `package/` 内 docs；行为类结论另经本地实测。以下每项给出「结论 + 出处」。

## 1. DSH 装载形态（影响：插件语言与工具链，D-2）

- **装载机制是 Node ESM 动态 `import()`，不是 require**：`@deepseek-ai/cordis-plugin-loader/src/index.ts:338-355`（`import(name)`）、`:583-591`（Entry 装载 → `unwrapExports`）。
- **CJS 插件可装载**：`unwrapExports`（`src/index.ts:208-216`）做 `exports.default ?? exports` + `__esModule` 归一化，注释明确处理 esbuild CJS interop。TS→CommonJS 的 `lib/index.js`（`exports.__esModule=true`）取 `apply` 安全。
- 入口 specifier 由 Node ESM 解析：`exports` 优先、回退 `main`（官方 `dsh-tool-bash/package.json` 两者皆写）。本插件同时声明 `main` 与 `exports['.']`。
- `dsh plugin add` 经 profile 目录内 pnpm 安装并写入 `dsh.profile.bundles`（`dsh-plugin-manager/src/operations.ts:305-393, 95-125`）；无 `dsh.bundle` 声明的包只装不激活。

**结论**：D-2（TS + tsc → CommonJS `lib/index.js`）成立，无需改动。

## 2. `dsh.bundle.patch` 与 `cordis.patch.yml`（影响：manifest 结构）

- **patch 声明嵌套在 `"dsh"` 键下**：`"dsh": { "bundle": { "patch": "./cordis.patch.yml", "platforms?: [...] } }`（类型 `dsh-package-manifest/src/types.ts:71-75`；官方实例 `dsh-base/package.json:34-38`、`dsh-cua-bundle`）。`dsh.bundle` 只有 `patch` 与可选 `platforms` 两个字段。**计划 §3 中「`package.json # dsh.bundle.patch`」按顶层键理解是错的，已按嵌套结构实现。**
- `cordis.patch.yml` = 顶层 YAML 数组（`PatchOptions[]`），允许 `!!js` 表达式（`dsh-app-boot/src/profile.ts:591-596`、`cordis-plugin-include/src/patch.ts:5-16`）。insert row（`EntryOptions`）字段：`id`（insert 自动补随机 id）、`name`（模块 specifier）、`config`、`group`、`disabled`、`inject`（`cordis-plugin-loader/src/config/entry.ts:10-23`）。
- insert row 的 `name` 必须是 profile workspace 可解析的 specifier——本插件用包名 `@ocrx/dsh`（D-1），计划 §3 示例中的 `ocrx-dsh` 是目录名，不作为 specifier。

## 3. `engines.dsh` 校验语义（影响：D-10 单测）

- `semver.satisfies(runtimeVersion, range, { includePrerelease: true })`（`dsh-app-boot/src/plugin-compatibility.ts:51-53`）；prerelease 参与匹配。
- 不满足 → 启动期该入口行 `disabled`（stderr 警告 `dsh: disabling profile plugin ...`，`compatibility-preflight.ts:82,114-118`）；**安装期直接拒绝安装（exit 1）**（`dsh-plugin-manager/src/operations.ts:361-380`），除非 `dsh plugin allow-version` 显式豁免。
- 有 `engines.dsh` 时它单独决定（忽略 peerDependencies）。

**结论**：D-10 的语义复刻单测与真实行为一致；窗口 `>=0.2.2-rc.5 <0.2.3-0` 接受 rc.5、拒绝 `0.2.3-alpha.1`（includePrerelease 下 `<0.2.3-0` 的 `-0` 上界排除 0.2.3 线全部预发布）。

## 4. 平台拒绝（影响：§6.2 平台策略）

- `assertBundlePlatform`（`dsh-app-boot/src/bundle-platform.ts:27-35`）：`platforms` 省略 = 全平台；否则必须是 Node 平台名或 `wsl`。不匹配 → `UnsupportedBundlePlatformError`（"Bundle supports Host platforms X; the DSH Host is running Y"）。
- 安装期：bundle 激活被拒（"bundle activation rejected"，依赖已装但 selection 不变，exit 1）；启动期：该 bundle 被跳过（stderr `skipping profile bundle`）。
- 官方先例 `dsh-cua-bundle/package.json`：`platforms: ["darwin"]`。

## 5. 工具注册契约（影响：阶段 4-6 全部工具）

- `ctx.tools.register(definition, options?)`；`ToolDefinition`：`name`、`description`、`parameters`（dsh 自研 JSON-Schema DSL：每属性 `{type, required?, description?, enum?, items?}`，编译为 open-object root JSON Schema）、`output`（**必需**：`{ schema, render(args, value) => ContentBlock[] }`）、`execute(args, exec) => Promise<JsonValue>`（**返回 canonical JSON 值；失败靠 throw**）、静态 `timeoutMs?`（注册期单一静态值，从不下发模型）、`isConcurrencySafe?(args) => boolean`（仅 `true` 允许并行）。
- `defineTool` 是**纯转换**（DSL→JSON Schema + 参数校验包装，`dsh-tools/src/schema.ts:555-603`），产物是普通对象。
- ContentBlock 文本块形状 `{ type: 'text', text }`（`dsh-llm/src/types.ts:144-152`；官方 render 实例 `dsh-tool-fs/src/read.ts:109-119`）。

**实现决策**：插件**自编译**同形 `ToolDefinition`（`src/toolkit.ts`），零 dsh 包导入，参数校验在自身 execute 内完成；真宿主装载由 CI `dsh-loading` 作业实测验证（阶段 9）。这保持「零运行时第三方依赖」（D-2）且不依赖 pnpm 对宿主包 peer 解析的形态。

## 6. commands 契约（影响：阶段 7）

- `ctx.commands.register({ name, description, handler(invocation) => CommandResult })`；name 规则 `/^[a-z][a-z0-9_-]*$/`（无斜杠小写）；CommandResult = `{ kind:'success', text? } | { kind:'error', text }`（`dsh-commands/src/index.ts:61-78`、`types.ts:34-41`）。
- **turn 外直执行**（`docs/subsystems/commands.md:177-196`："no turn wraps them"）——印证 §8.3「不提供 /ocrx install-model」。

## 7. SkillProvider 契约（影响：阶段 7）

- `ctx.skills.registerProvider(control => ({ name, list(options) => SkillCandidate[], get(candidate, options) => SkillDefinition }))`，须在 apply 内同步注册（`dsh-skill/src/index.ts:390-428`、`:246-275`、`:57-100`）。
- SkillDefinition = summary + `content: string`（Markdown 正文）；官方实例 `dsh-skill-office/src/index.ts:111-140`（读 SKILL.md、frontmatter 提供 name/description）。

## 8. subprocess.spawn 契约（影响：run.ts，D-8）

- 服务 `ctx.subprocess`，`spawn(spec: SubprocessSpawnSpec) => SubprocessHandle`（同步返回 handle）。
- spec **全显式**：`argv: readonly string[]`（argv[0] = 程序，无 shell）、`cwd: string`（必填）、`stdio: { stdin, stdout/stderr: 'pipe'|'inherit'|{ maxBytes, spill?: { maxBytes } }, control? }`、`graceMs: number`、`signal?: AbortSignal`、`env?: ProcessEnv`（合并 scrubbed 父环境；undefined 值 = 删除墓碑）（`dsh-subprocess/src/types.ts:77-107`；`docs/subsystems/subprocess.md:85-138`）。
- 收集溢出**保留尾部**文本；spill 可选全量文件。handle：`collected.stdout/stderr`（offset 读取器）、`done: Promise<{exitCode, signal}>`、`terminate()`（幂等）、`waitForExit()`（等整个 managed range）。
- 进程树：Windows kill-on-close Job；Linux user-systemd scopes；macOS detached process-group/taskkill 降级 + 警告（`dsh-subprocess-local/README.md:86`）——印证 §7.3.4 的 macOS 验收口径（以 ocrx 主进程终止为准）。
- **注意**：spec 是对象签名（`{ argv, cwd, ... }`），不是 `(file, args, options)`；计划 run.ts 按此实现。

## 9. workingDirectory.ensure（影响：D-8）

- `ensure(agent, signal?) => Promise<string>`（`dsh-working-directory/src/index.ts:145-175`）；agent 缺席时官方 bash 工具的处理是 `exec.agent === undefined ? undefined : await ensure(...)`（`dsh-tool-bash/src/index.ts:512`），随后以插件进程 cwd 兜底——D-8 的「判空回落 process.cwd()」与官方一致。

## 10. approval / 同意门（影响：consent.ts，阶段 6）

- 获取：`ctx.get('approval')`，服务缺席返回 `undefined`（`cordis/src/reflect.ts:9-19`）。
- `request(req) => Promise<ApprovalOutcome>`；outcome ∈ `'allowed-once' | 'rejected' | 'cancelled' | 'unavailable'`（**唯一授予值是 'allowed-once'**）；req = `{ agent, toolName, callId?, reason?, displayReason?, signal? }`；**须在 open turn 内**，否则 throw（`dsh-user-approval/src/index.ts:242-261`）。
- **`never` 策略在 `decide()` 内、分发前直接返回 `'rejected'`**（`:296-301`）——归入拒绝分支（非 unavailable）。
- abort → `'cancelled'`；无 answerer/答者抛错/非法返回 → `'unavailable'`（fail closed）。`request` 抛错（含 turn 外）→ 按 unavailable 处理。
- 官方调用形态：`ctx.get('approval')` 作可缺席 approver 传入（`dsh-tool-bash/src/index.ts:244-267`、`dsh-sandbox/src/escalation.ts:209-263`）。

## 11. PTC（run_code）路径下的 approval（需求 §14.1.3 未核实项 → 已核实）

- **可用**：PTC 子调用重走完整执行管线（策略/guard/approval 全生效），denial 以 binding rejection 返回程序（`docs/tool-execution-pipeline.md:64`、`dsh-tool-executor-ptc/src/ptc.ts:126`）；嵌套审批等待计入 timeout 预算（`ptc.ts:115`）；run_code 自身升级也走 `ctx.get('approval')`（`ptc.ts:361-369`）。
- 结论：同意门在 PTC 场景落入 `rejected/cancelled/unavailable` 各分支均有定义，无静默下载路径。

## 12. Config schema 形态（影响：config.ts）

- Cordis 官方形态是 `export const Config: z<...> = z.object({...})`（`@deepseek-ai/schemastery`，Standard Schema；校验失败 → fiber FAILED → **插件装载失败**，`cordis/src/fiber.ts:50-62`；官方实例 `dsh-tool-bash/src/index.ts:58-62`）。
- **实现决策**：插件导出可选 `Config`（宿主有 schemastery peer 时构建 schema；纯 Node 单测环境无该包时为 undefined，cordis 亦不存在，跳过安全）——`resolveConfig()` 在 apply 内始终做完整校验并抛错阻断装载，两条路都满足 §14.2「非法值装载期拒绝」。

## 13. name/inject/apply 导出形态

- 三形态之一：函数/类/`{ apply }` 对象 + 元数据 `name?/Config?/inject?/provide?`（`cordis/src/registry.ts:92-133`）；`inject` 是**必需服务**列表——缺席时插件 PENDING 等待、不加载（印证 §6.3「approval 不进 inject」的理由）。官方对象形态实例 `dsh-tool-bash/src/index.ts:36-37`。

## 14. pnpm 行为实测（计划阶段 0 任务 3）

- **optionalDependencies 版本不存在（未发布）→ pnpm 跳过并继续安装**（本地实测：`pnpm install` 在 `@ocrx/ocrx-win32-x64@0.1.0` 不存在时成功完成，devDeps 正常装）；**npm 则直接 notarget 报错**（`--omit=optional` 也无法跳过解析）。**CI 插件作业统一用 pnpm**（与 dsh 生态前提一致）。
- pnpm 默认 `auto-install-peers=true` 会追 dsh 树的传递 peer（如 `@deepseek-ai/dsh-bash-env`，两 registry 均 404）→ 插件目录 `.npmrc` 固定 `auto-install-peers=false`。
- `minimumReleaseAge`（pnpm 11 默认 24h）：本地 pnpm 9 未默认启用；按 runbook 在发布流程留 24h 窗口（§6 阶段 8.5），不阻塞开发。

## 15. Node 工具链落地（阶段 0 任务 4）

- `plugins/ocrx-dsh/`：pnpm + TypeScript 5.9 → CommonJS `lib/`（tsconfig module node16 / moduleResolution node16，同时写 main 与 exports['.']）；测试用 node:test + `tsconfig.test.json` 编译到 `test-dist/`（跨 Node 版本可移植，不依赖原生 type-stripping）；`npm test` = build + build:test + `node --test "test-dist/test/*.test.js"`。装载冒烟（name/inject/apply/注册/Config 校验矩阵）5/5 绿。
- `@deepseek-ai/dsh` 不进 devDependencies：CLI 包不带插件面向的类型（仅 CLI 命令 .d.ts），手写 `src/dsh-seams.d.ts` 承载 seam 类型（D-2 允许的分支）；DSH 真宿主装载在 CI `dsh-loading`（阶段 9）以锁定版本实测。
- **Node 版本**：dsh engines 要求 `>=24.9`——CI（含 dsh-loading）用 Node 24；本地开发 Node 22.16 跑单测无碍。

## 16. 测试资产清点（阶段 0 任务 5）

- 小 PDF：`benchmarks/fixtures/pdf/normal-1-page.pdf`（600 B，已入库）；纯文本零模型路径：`benchmarks/fixtures/pdf/native-plus-empty.pdf`（885 B，native 路径）；输入错误映射：`not-a-pdf.txt`；批量/取消/加密场景 fixture 齐备（`encrypted-password-required.pdf`、`corrupt-truncated.pdf` 等）。
- CI 用桩下载（FakeTransport 已存在于 `src/ocrx-models` 测试）；真机门槛用真实下载（ovisocr3 quality ≈ 0.97 GB，`src/ocrx-models/src/builtin/ovisocr3.rs`）。

## 17. 对计划的修订记录

1. `dsh.bundle.patch`/`platforms` 嵌套在 `"dsh"` 键下（§2）；insert row `name` 用 `@ocrx/dsh`。
2. subprocess.spawn 为 spec 对象签名（§8）。
3. 插件自编译 ToolDefinition，零 dsh 包导入（§5）；Config 双层校验（§12）。
4. CI 插件作业用 pnpm + Node 24（§14/§15）。
5. 审批 outcome 词汇按 `'allowed-once'|'rejected'|'cancelled'|'unavailable'` 实现（§10）。
