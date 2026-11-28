# OCRX DSH 插件 v0.1 发布门槛执行记录

日期：2026-11-27
环境：Windows x64 开发机（worktree `feat/dsh-plugin-v0.1` @ 自动化合流点）；CI 作业见 `.github/workflows/ci.yml`（plugin-test / dsh-loading / engines-window）。
对应需求：`docs/requirement/dsh-plugin-v0.1.md` §14.2/§14.3；计划 §6 阶段 9。

## 1. 自动化门槛（本机已执行，CI 持续覆盖）

| 门槛 | 结果 | 证据 |
| --- | --- | --- |
| Rust workspace 门禁（fmt / clippy -D warnings / 全测） | ✅ 全绿 | stage 1-2 合流点提交；`cargo test --workspace` 0 failed |
| C3：`ocrx version` == workspace 版本 == doctor JSON `version` | ✅ | `cli_subprocess::cli_version_and_doctor_report_the_same_workspace_version` |
| C2：recommend 计划面 / install 摘要与失败对象 / 退出码不变 | ✅ | `ocrx-models` 65 测 + `cli_subprocess` 10 测；离线失败对象实测（stdout 单对象、exit 5） |
| C2：传输加固旗标 / 尝试上限 3 / 残留清扫双 scope / 锁 PID / 信号清理函数 | ✅ | `install.rs` 新增 8 测（含 SlowTransport ticker、CountingFailTransport、篡改清扫负例、purge_paths） |
| C4：macOS 归档许可齐备 / 平台包文件集 / SHA256SUMS 同源 / ≤64MiB / 无 postinstall | ✅ | `test-release-scripts.sh`（fixture 自测含篡改负例）；打包器对 `\\?\` 路径与 npm-cli 直调已固化 |
| release.yml 结构（artifact 命名 / needs / 认证 / 预检先于 publish / 首跑与重跑分支） | ✅ | 同上 workflow 结构断言 |
| 插件单测（argv/形态判别/错误表/冻结配对/readiness 四组合/同意门三分支/失败映射/stats/残留/命令/Skill/Config 校验/装载冒烟） | ✅ 88 项 | `plugins/ocrx-dsh` `npm test`（89 项，含 1 汇总） |
| D-10：engines 窗口接受 `0.2.2-rc.5`、拒绝 `0.2.3-alpha.1`（含 rc.10/rc.1 与 includePrerelease 语义） | ✅ | `engines-window.test.ts` |
| D-11：旧二进制降级（recommend 去旗标重试一次 / 版本未知标注 / install 文本映射） | ✅ | `scan.test.ts` / `install.test.ts` / doctor `cli_version_note` |
| 真实 CLI 集成（零模型端到端+产物 diff / 单文档与批量 model_missing / resume 重试合法性 / doctor 版本面 / model 目录透传+C2 计划 / consent_denied 与 offline install_blocked） | ✅ 7 项 | `integration.test.ts` @ `target/release/ocrx.exe`（worktree 构建）；CI 以 `OCRX_TEST_BINARY` 注入 |
| 恒 `--offline` 构造性封死 + 互斥拒绝 + device 静态预判 | ✅ | `scan.test.ts` argv 钉死断言 |
| 无审批服务部署：插件正常加载、install → `consent_denied` | ✅ | 装载冒烟（FakeCtx 无 approval）+ `install.test.ts` unavailable 用例 |
| PTC（run_code）路径 approval 表现（§14.1.3 未核实项） | ✅ 已核实 | spike §11：子调用重走完整管线、denial 为 binding rejection、审批等待计入 timeout——三分支均有定义，无静默下载路径 |

CI 无法本地执行的作业（`dsh-loading`、双平台 `plugin-test` 矩阵）随 push 在 GitHub Actions 上执行；其可脚本化装载面（CJS 互操作 unwrapExports → name/inject/apply、Config 装载期校验）已在本机以 ESM `import()` 复刻验证通过。

## 2. 真机门槛（未执行——需真机窗口，runbook §7 清单）

| 门槛 | 状态 | 执行要点 |
| --- | --- | --- |
| Windows x64（CPU）真实漏斗（干净环境、真实下载、同意流、产物与直连 CLI diff） | ⬜ 未执行 | 干净 profile；记录模型下载体积/耗时与 `.part` 清理行为 |
| macOS arm64（Metal）真实漏斗 | ⬜ 未执行 | 同上；另记录 Gatekeeper 下未签名二进制首启行为（§16.3，缓解措施如实文档化） |
| 干净 profile `npm pack → dsh plugin add <tarball>` 冒烟 | ⬜ 未执行 | 发布后经 npm 名复验（24h `minimumReleaseAge` 窗口外） |
| **零网络请求回归门槛**（随包 CLI 全工具 argv 断网/进程级监控） | ⬜ 未执行 | P1 暴露 `layoutRegions` 前必须重跑（前置 §13-C9） |
| 卸载残留目检（插件自身无残留；模型缓存按范围限定保留） | ⬜ 未执行 | CI `dsh-loading` 的 `--verify-removed` 覆盖可脚本化部分 |

## 3. 已知偏差与说明

1. **扫描缓存隔离**：`scan` 不读 `OCRX_MODEL_CACHE`（C5 为 P1），集成测试以 `LOCALAPPDATA`/`HOME` 覆盖隔离缓存根——已记入 spike §16 与测试注释；C5 落地后应切回环境变量隔离。
2. **视觉 fixture 的路由**：本地构建中 `benchmarks/fixtures/pdf` 的全部非加密 fixture 在零模型下走 native/empty 路径（auto 路由不触发 OCR 下载路径），model_missing 集成面以 `pdfMode: "ocr"` 强制 OCR 路由触发——这是 CLI 实际行为，非测试桩。
3. **npm 发布**：`publish-npm` 作业与三包发布未执行（依赖仓外前置：environment `npm` + `NPM_TOKEN`，见 runbook §0 清单）。

## 4. 结论

自动化可覆盖的门槛全部通过；真机双平台漏斗、真实下载自举与零网络门槛等待真机窗口按 runbook §7 执行后归档补充。

## 5. 代码评审修复记录（2026-11-27，review BLOCK×9 全部闭环）

对照 `docs/review/dsh-plugin-v0.1-code-review-2026-11-27.md`（评审基线 `3526472`）：

| BLOCK | 修复 | 回归测试 |
| --- | --- | --- |
| 1 | workflow 改用 npm 真实 scoped tarball 名（无 `@`：`ocrx-ocrx-<plat>-<v>.tgz`）；上传 glob/预检 ls/发布路径全部对齐；pack.json 路径废弃（tarball 路径显式传递） | `test-release-scripts.sh`：真实 `npm pack --dry-run --json` 名契约探针 + workflow glob 断言（禁止 `dist/npm/@ocrx-` 拼写） |
| 2 | `publish-npm` 增 pnpm frozen-lockfile 安装 + `pack-plugin.mjs`（编译、拷根 LICENSE、tar 内容断言 lib/index.js + LICENSE + patch + skill、无构建脚本）后才发布 | pack-plugin 本机行为自测（tar 内容断言）+ workflow 结构断言 |
| 3 | `install.rs` 传输成功路径 `last_error = None`（重试真正恢复） | `FlakyTransport`（首败后成）断言提交成功、total_bytes=23 |
| 4 | `resolveBootstrapTarget` 双显式分支改用归一化布尔（`auto` = 未显式） | `model+auto ≡ 仅 model` 逐字段一致测试（含 recommend 查询参数） |
| 5 | 残留 scope 从 plan 派生（revision + `<rev>/<profile>` 两目录，去重） | 安装工具级取消测试：100B+50B 两文件各计一次、总量 150B、model 不再漏报 |
| 6 | install 同意后检测 code 2/unsupported flag → 同一 execute 去旗标重试一次（不再同意）；`ocrx_model` recommend 走 `fetchRecommend` 降级 | 三进程序列测试（recommend±旗标、install±旗标、单次同意、stats 如实缺省） |
| 7 | `model recommend --report json <profile>` 接受位置档位（计划按目标 manifest）；插件 `fetchRecommend`/`fetchPlan` 携带 profile；同意体积来自目标 manifest，不可得时如实标注 unconfirmed（不再统一 1.0 GB） | `plan.rs` explicit_profile 测试（尺寸=该 manifest 之和）+ 插件双显式/仅 profile 组合体积断言 + 同意文案 540.0 MB 断言 |
| 8 | stderr collect 启用 `spill`；`readFrom().truncated` 透传；截断（前缀丢失）时禁止 FIFO 配对（全部 code 默认 + attribution_degraded） | run spec spill 断言 + 截断批量回归测试（同 code 双文档不误挂） |
| 9 | 发布逻辑抽为 `plugins/npm/publish-npm.sh`：重跑时经 fake registry 下载已发布 tarball 与本地字节比对，不一致即 FAIL；`npm view` 网络/认证错误不再视为 missing | fake npm 行为测试四场景：首跑全发（平台先于插件）、同字节重跑幂等跳过、字节不一致 FAIL、registry 错误 FAIL |

修复后门禁：Rust fmt/clippy -D warnings/workspace tests 全绿；插件 `npm test` 102 项（95 过 + 7 集成跳过；真实二进制集成 7/7 单独复跑绿）；`test-release-scripts.sh` 全绿（新增 BLOCK-1 名契约探针、BLOCK-2 pack-plugin 行为自测、BLOCK-9 fake-npm 四场景行为测试——不再仅 grep workflow 文本）。

## 6. 重审修复记录（2026-11-27，re-review 剩余 BLOCK-8 闭环）

对照 `docs/review/dsh-plugin-v0.1-code-review-2026-11-27-re-review.md`（基线 `6ab66b1`）：锁定宿主 `@deepseek-ai/dsh-subprocess@0.2.2-rc.5` 的 `readFrom()` 真实契约为 `{ text, nextOffset, lossy, spillPath? }`（已重新下载发布包核对 `src/types.ts:124-133`），此前插件读取不存在的 `truncated` 字段且忽略 `spillPath`，导致真实宿主上截断保护不可达、spill 全流未消费。

修复与测试：

1. `dsh-seams.d.ts` 类型按 rc.5 修正为 `CollectedRead { text, nextOffset, lossy, spillPath? }`。
2. `run.ts`：`lossy` 且 spillPath 可读 → `readFileSync(spillPath)` 恢复全流（stderr 与 stdout 两侧，stdout 聚合 JSON 大于内存窗口同样受益）；`lossy` 无可用 spill → `stderrTruncated: true`。该标志进入 `normalizeAggregate` 的"截断禁止 FIFO 配对"保护分支。
3. fake harness 的 readFrom 改为真实字段（`stderrLossy`/`stderrTail`/`stderrSpillPath`/`stdoutSpillPath` 脚本面），不再用自定义 `truncated` 模拟。
4. 行为测试三项（重审建议清单）：①内存 tail + 有效 spill → 从 spill 恢复全部失败对象并正确 FIFO 配对（run.test + scan.test 组合测试，正是重审的同 code/不同 cause 复现场景——每个文档拿到自己的 message，无 attribution_degraded）；②`lossy` 无 spill → 全部禁配对 + attribution_degraded；③stdout lossy + spill → 聚合 JSON 恢复。

修复后：插件 `npm test` 106 项全绿（99 过 + 7 集成跳过）；真实二进制集成 7/7；Rust workspace 全测 0 failed；`test-release-scripts.sh` EXIT=0。

## 7. 三审修复记录（2026-11-28，BLOCK-10 闭环）

对照 `docs/review/dsh-plugin-v0.1-code-review-2026-11-28-re-review-2.md`（基线 `dc7e445`）：stdout collect 此前未请求 spill，真实宿主不会为聚合 JSON 提供 spillPath，恢复逻辑不可达；exit 0 前缀丢失被误报为 launcher_failed。

修复与测试：

1. `run.ts`：stdout collect 规格启用 `spill`（新增 `stdoutSpillMaxBytes`，缺省 = maxBytes × 8）；`stdoutTruncated` 推导与 stderr 对称（lossy + 可读 spill → 全流恢复；lossy 无 spill → truncated）。
2. `parse/result.ts`：批量 stdout 前缀丢失（lossy 无可用 spill）且无法解析聚合时——**不再落入 code 默认兜底**（exit 0 曾被误报为 launcher_failed）——返回专门的 collection-limit `cli_error`：如实指出是插件采集上限、exit code 为多少、指引拆小批次重跑；单文档 face 的 unparseable 分支同样区分截断（collection limit）与契约异常（retry/report）。
3. fake harness 守卫：脚本化 spillPath 而对应 collect 规格未请求 spill 时**直接抛错**——测试无法再注入真实宿主不会产生的能力（正是上轮全绿漏掉该问题的根因）。
4. 行为测试四项：stdout collect 规格断言（必须请求 spill 且 cap ≥ maxBytes）；harness 守卫负例；lossy 无 spill 批量 → collection 错误（非 launcher_failed，指引含 exit code 与拆批）；单文档 face 同场景 → cli_error + collection limit 说明。

修复后：插件 `npm test` 110 项全绿（103 过 + 7 集成跳过）；真实二进制集成 7/7；Rust workspace 0 failed；fmt 干净；`test-release-scripts.sh` EXIT=0。

## 8. 本机 DSH 真实装载轮（2026-11-28）

环境：本机 DeepSeek Harness 桌面版自带 CLI（`resources/runtime/cli/bin/dsh.cmd`，版本 **0.2.2-rc.5**，与冻结窗口一致；Electron 运行时满足 node ≥ 24.9）。

执行序列与结果：

1. `pack-plugin.mjs` 打插件包 `ocrx-dsh-0.1.0.tgz`（4 个关键 entry 齐）。
2. 本地组装 win32 平台 stage（target/release 真实 ocrx.exe + vendor pdfium + 系统 CRT DLL + 许可集）→ `build-platform-packages.mjs` 打 `ocrx-ocrx-win32-x64-0.1.0.tgz`（7.5 MB ≤ 64 MiB）。
3. `dsh plugin --profile ocrx-test add <插件 tgz>` → pnpm v11.28.5 安装成功，`dsh.profile.bundles` 激活 `@ocrx/dsh`（pnpm 对未发布的 optionalDependencies 静默跳过，符合预期）。
4. `dsh plugin --profile ocrx-test add <平台包 tgz>` → 作为普通依赖落位（dsh 正确提示"declares no dsh.bundle — installed as a plain dependency"）。
5. `--dump-config`：组合树出现 `# == @ocrx/dsh` 层（`id: ocrx, name: '@ocrx/dsh'`）。
6. **真实宿主 web 启动装载**：初次启动暴露真实 bug——宿主 schema 校验器拒绝 output schema `type: 'json'`（JsonSchemaError，注册失败）。修复为 `type: 'object'` 并新增宿主子集回归测试（所有编译后 schema type ∈ object/array/string/number/integer/boolean/null）后重新打包安装，启动无任何 ocrx 相关错误输出（bundle 挂载与 apply() 注册通过，失败会打印并跳过 bundle）。
7. **bundled 定位**：插件在 profile 内 `locateBinary()` → `source: 'bundled'`，指向 `node_modules/@ocrx/ocrx-win32-x64/bin/ocrx.exe`；该二进制 `ocrx version` → `ocrx 0.1.0`，真实扫描（native-plus-empty.pdf，pdf-mode text，恒 offline 旗标）→ success 2/2 页 + markdown 产出。
8. `dsh-loading-smoke.mjs` 对真实 profile：注册/Config 校验/binary 面全部通过。
9. `dsh plugin remove @ocrx/dsh` + `remove @ocrx/ocrx-win32-x64` → bundle 退回 dsh-base，无插件文件残留（仅 pnpm 自身留下的空 scope 目录，已澄清非插件残留）；测试 profile 已删除还原。

发现并修复：output schema `type: 'json'` 超出 rc.5 宿主允许子集（静态测试全绿未能发现——该 face 只有真实宿主能暴露）。回归测试已钉死。

注：headless 会话面需 LLM 凭据（新 profile 未配置），未执行；headless 漏斗属真机门槛（runbook §7）。
