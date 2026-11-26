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
