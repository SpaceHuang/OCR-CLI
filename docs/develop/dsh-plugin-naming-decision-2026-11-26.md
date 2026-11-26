# DSH 插件 npm 包命名决策

日期：2026-11-26
状态：**结案**（决策人：产品 owner，会话内拍板）
性质：解除需求 `docs/requirement/dsh-plugin-v0.1.md` §16.1「插件与平台包命名——阻塞一切发布动作」

## 决策

| 包 | 名称 |
| --- | --- |
| 插件包 | **`@ocrx/dsh`** |
| Windows x64 平台包 | `@ocrx/ocrx-win32-x64`（沿用需求 §5.1 占位名） |
| macOS arm64 平台包 | `@ocrx/ocrx-darwin-arm64`（沿用需求 §5.1 占位名） |

仓内源码目录维持 `plugins/ocrx-dsh/`（目录名与包名解耦，包名只出现在 package.json 与 npm 模板中）。

## 占用核查（2026-11-26，registry + npm 站点差分）

- registry：`ocrx-dsh`、`ocrx`、`@ocrx/ocrx-win32-x64`、`@ocrx/ocrx-darwin-arm64`、`ocrx-win32-x64`、`ocrx-darwin-arm64`、`ocrx-dsh-plugin` 全部 404（无既有包）；
- npm 站点：`~ocrx`（用户）与 `org/ocrx`（组织）均 404，差分对照 `org/babel` = 200 → `@ocrx` scope 可创建。

## 理由（插件进 scope）

1. **发布自动化**：组织令牌（granular token 授权 scope `@ocrx` 读+写）从首次发布起覆盖全部三包，CI 全自动；无 scope 包首次发布需手动 `npm publish` 后再把包补进令牌范围。
2. **命名空间保护**：`@ocrx/*` 归组织所有，任何第三方永远无法在其中发包；无 scope 名在全局命名空间，存在被抢注面。
3. **npm 门面**：组织页将三个包列在同一页；主包无 scope 会造成「平台包在组织页内、主包在页外」的割裂。
4. **将来扩展**：后续新包（P2 Client 插件、meta 包等）自动有归属。

用户侧代价为零：安装命令多敲一个 `@`（无 Shell 转义问题）；装完后用户可见面（工具名 `ocrx_*`、`/ocrx` 命令、bundle id `ocrx`）由 `cordis.patch.yml` 决定，与 npm 包名无关。

## 考虑过且未采纳的备选

- **`ocrx-dsh`（无 scope，即需求占位名）**：README 首屏命令短 4 字符；但首发需手动步骤、无命名空间保护、npm 门面割裂——不值。
- **`@ocrx/win32-x64`（平台包去掉 scope 内冗余的 `ocrx-` 前缀，esbuild 风格）**：更简洁，但偏离需求 §5.1 已冻结的占位名，不为此重开需求评审；前缀冗余对检索与 npm 列表可读性无害。
- **裸名 `ocrx`**：原判「暂不占用」；**2026-11-26 同日改判采纳**——以 npm 启动器包形式占用（esbuild 模式：`npm i -g ocrx` 即得可用 CLI，复用 `@ocrx/ocrx-<plat>`），记为 TODO 收录于开发方案 §10 P1 backlog，落地前裸名保持空闲。

## 影响与生效

- `docs/plan/dsh-plugin-v0.1-development-plan.md` 已回填定名（§1/§3/D-1/阶段 0/阶段 3/runbook/风险表）。
- `docs/develop/dsh-plugin-npm-setup-guide-2026-11-26.md` 第 4/5 步与完成清单已按定名更新。
- **生效状态**：`@ocrx` 组织已于 2026-11-26 在 npmjs.com 创建（org 页可访问，Packages 0，独立核验通过），`@ocrx/*` scope 永久锁定，本决策完全生效。裸名 `ocrx` 的占用已同日决策：采纳 npm 启动器包形式，记 TODO（见下「考虑过且未采纳的备选」末条与开发方案 §10 P1）。
