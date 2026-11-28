# OCRX DSH 插件发布 runbook（v0.1）

依据：`docs/plan/dsh-plugin-v0.1-development-plan.md` §6 阶段 8 / §8；命名决策 `docs/develop/dsh-plugin-naming-decision-2026-11-26.md`；npm 前置操作指南 `docs/develop/dsh-plugin-npm-setup-guide-2026-11-26.md`。

## 0. 仓外前置确认（阶段 8.5 清单，全部打勾才进步骤 1）

- [ ] D-1 命名已结案：`@ocrx/dsh` + `@ocrx/ocrx-win32-x64` + `@ocrx/ocrx-darwin-arm64`（2026-11-26 结案，`@ocrx` org 已创建并验证）。
- [ ] npm 账号：邮箱已验证、2FA 已开启。
- [ ] **Granular Access Token**：仅授权三个目标包的 publish（不用全权限 classic token），已设到期时间与轮换节奏（granular token 的 publish 不需要 OTP，CI 不被 2FA 阻断）。
- [ ] GitHub 仓库：已创建 environment `npm`（required reviewers 作为手动门），`NPM_TOKEN` secret 挂在该 environment（最小权限，不挂 repo 级）。
- [ ] dist-tag 策略确认：三包均发 `latest`（平台包只被精确版本消费，`latest` 仅保证 npm 页面正常）。
- [ ] 可选增强（不阻塞）：npm provenance（`--provenance`，CI OIDC 签源）或 trusted publishing。

## 1. 代码合流

Rust 泳道（C2/C3）与插件泳道（plugins/）合入 `main`，CI 全绿（含 `plugin-test` / `dsh-loading` / `engines-window` 三作业）。

## 2. 版本准备提交（tag 前必做）

当前 workspace 与 `Cargo.lock` 均为 `0.1.0` 且已存在 `v0.1.0` tag——直接打 tag 会冲突。选择**未使用**的 workspace 版本（含 C2/C3 的首发建议 `0.2.0`），同一次提交内更新：

1. workspace 版本（根 `Cargo.toml` 的 `[workspace.package] version`；`Cargo.lock` 随构建刷新）；
2. 插件版本与 README 兼容矩阵（`plugins/ocrx-dsh/package.json` 的 `version` + `README.md` 表）;
3. 插件 `optionalDependencies` 两个**精确**平台版本 = 新版本（D-3：无 `^`）；
4. `plugins/npm/package-*.json` 模板版本由打包器注入（无需手改）。

经 CI 合入 `main`。

## 3. 打 tag（预检失败即停）

```sh
# 「tag 不存在」检查在 push 之前（release.yml 的 publish-npm 只断言 tag 存在且指向本次运行）
git ls-remote --tags origin | grep "refs/tags/v<ver>" && { echo "tag 已存在，换版本"; exit 1; }
git tag -a "v<ver>" -m "ocrx v<ver> (+ @ocrx/dsh <ver>)" && git push origin "v<ver>"
```

CI：tag ancestry guard → 版本一致性 → 双平台出归档 + **本平台** npm tgz（artifact：`npm-ocrx-win32-x64` / `npm-ocrx-darwin-arm64`）→ GitHub Release。

## 4. 手动 promotion：`publish-npm` 作业（environment `npm` 门）

作业按顺序执行（结构断言见 `scripts/test-release-scripts.sh` 的 workflow 段）：

1. 认证与权限：`npm whoami`；`@ocrx` scope 发布权限（granular token）。
2. 版本预检：tag 存在且 == workspace 版本 == 平台包版本 == 插件 `optionalDependencies` 钉的版本；tag 指向本次 `github.sha`。
3. **首跑/重跑分类**：`npm view <pkg>@<ver>` 三包判 404——首跑全量发布；重跑（部分已发布，如中断恢复）→ 已发布者经 tarball 一致性核验后跳过、仅补发缺失者（npm 版本不可变，这是唯一恢复路径）。**不得用「均未发布」阻断合法断点恢复。**
4. 发布顺序：**平台包先于插件包**（插件 `optionalDependencies` 精确钉的版本必须已在 registry）。
5. 插件 tarball 的 packed `package.json` 断言**无 postinstall/preinstall/prepare**（pnpm `allowBuilds` 政策）。
6. 失败处理：某包发布失败 → 修复后重跑本 job；预检识别已发布者并跳过。在本节记录实际发版清单与差异原因（版本-发布物对照表）。

**记录表**（每次发版填一行）：`版本准备提交 SHA ↔ tag ↔ 发版清单（包@版本 × 实际发布时间）↔ 重跑/首跑`。

## 5. GitHub 收尾

- Release 页确认归档齐（win zip + macos tar.gz）；
- 仓库打 `dsh-plugin` topic；
- 主 README / 官网加 DSH 安装入口（30 秒上手块）；
- CUDA 路线说明（README 单独注明：本地工具链构建、非 CI 同源）。

## 6. 24h 窗口后复验

pnpm 11 `minimumReleaseAge` 默认 24h：公告发布后**预留该窗口**；窗口外用干净 profile 复验：

```sh
dsh plugin --profile smoke add @ocrx/dsh
# 漏斗冒烟：/ocrx → 扫描 → 同意安装 → 重试 → dsh plugin --profile smoke remove @ocrx/dsh
# 卸载后目检：插件自身无残留（模型缓存按范围限定保留）
```

## 7. 真机双平台漏斗 + 零网络门槛（阶段 9.2）

- Windows x64（CPU）与 macOS arm64 各完成一次真实漏斗（干净环境、真实下载、同意流、产物与直连 CLI diff），记录到 `docs/test/dsh-plugin-v0.1-release-gates-<date>.md`；
- **零网络请求回归门槛**：随包 CLI 在插件全部工具 argv 下扫描典型输入，断网/进程级监控验证零请求（P1 暴露 `layoutRegions` 前必须重跑，前置 §13-C9）。

## 回滚

- npm `unpublish` 72h 窗口内可用；插件与平台包同退；
- GitHub Release 附件可独立删除；
- 兼容矩阵（README）随每次发版更新插件 ↔ ocrx ↔ `engines.dsh` 三列；
- 发版前核对「三处版本口径」（插件 package.json ↔ README 矩阵 ↔ assets SKILL.md 与插件 SKILL.md 的对齐，C6 结案后）。
