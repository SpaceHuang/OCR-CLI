# DSH 插件 npm 发布前置：账号 / 组织 / 令牌 / GitHub 环境 注册指南

日期：2026-11-26
服务：`docs/plan/dsh-plugin-v0.1-development-plan.md` 阶段 8.5「npm 侧仓外前置」
预计用时：**约 15–20 分钟**（全程浏览器内完成，最后一可选步用命令行验证）
前提：一个常用邮箱、一部手机（认证器 App 或短信）、OCR-CLI GitHub 仓库的管理员权限。

> 背景速览：npm 自 2025-11-05 起禁用了 classic token 创建，发布凭据只能用 **Granular Access Token**（细粒度令牌）；组织对公有包免费。本指南按此现状编写。

---

## ☑ 第 1 步：注册 npm 账号（2 分钟）

1. 打开 <https://www.npmjs.com/signup>；
2. 填写：邮箱（常用邮箱，后续 org/发布通知都走它）、用户名（您的个人 ID，与 `@ocrx` 组织名无关，随便取）、密码；
3. 过人机验证，提交；
4. **先别关页面**，进入第 2 步。

## ☑ 第 2 步：验证邮箱（1 分钟，后续所有步骤的前置）

1. 收件箱找 npm 的验证邮件（没收到就查垃圾箱 / 在网站头像菜单 → Account 里重发）；
2. 点邮件里的 **Verify Email**；
3. 未验证邮箱无法创建组织和令牌——这一步没完成，后面会到处碰 403。

## ☑ 第 3 步：开启 2FA（3 分钟）

1. 头像菜单（右上角）→ **Account**（或直达 <https://www.npmjs.com/settings/~/profile>）；
2. 找到 **Two-Factor Authentication** → **Enable 2FA**；
3. 方式二选一：
   - **Authenticator App**（推荐）：Microsoft Authenticator / Google Authenticator / 1Password 扫码；
   - **Security Key**：WebAuthn（YubiKey、Touch ID、Windows Hello 均可）；
4. 输入一次验证码确认；
5. **把恢复码（recovery codes）抄下来存进密码管理器**——手机丢了这是唯一救生索；
6. 2FA 档位保持默认（登录 + 敏感操作）即可；**不影响 CI**——细粒度令牌发布时不需要 OTP。

## ☑ 第 4 步：创建 `@ocrx` 组织（2 分钟）

1. 头像菜单 → **Add Organization**（或直达 <https://www.npmjs.com/org/create>）；
2. 组织名填 `ocrx`（2026-11-26 已核查未被占用，见开发方案阶段 0 记录；若此刻提示被占用，回到方案 D-1 换备选 scope）；
3. 计划选 **Unlimited public packages — $0**（免费档，公有包无限制；私有包才要付费）；
4. 创建完成。组织页 <https://www.npmjs.com/org/ocrx> 现在应该能打开了；
5. （可选）Organization Settings → 确认允许 granular access tokens（默认允许）。

**命名决策（已定，2026-11-26）**：插件包名取 **`@ocrx/dsh`**（进 scope，决策记录见 `docs/develop/dsh-plugin-naming-decision-2026-11-26.md`）。组织令牌从首次发布起即覆盖全部三个包，CI 首发全自动，无任何手动首发步骤。

## ☑ 第 5 步：创建发布令牌（Granular Access Token，3 分钟）

1. 头像菜单 → **Access Tokens**（或直达 <https://www.npmjs.com/settings/~/tokens>）；
2. **Generate New Token** → **Granular Access Token**（2025-11 后唯一选项）；
3. 表单逐项：
   - **Token name**：`ocrx-dsh-ci-publish`（自明即可）；
   - **Expiration**：90 天或 365 天（到期记得轮换：重新生成 → 更新 GitHub secret）；
   - **Packages and scopes**：**Select packages and scopes** → 勾选 **scope `@ocrx`**，权限 **Read and write**——授权到 scope 而不是逐包，`@ocrx/*` 下未来新增的包（含插件 `@ocrx/dsh`）自动被覆盖；
   - **Organizations** 一栏**留空**（那是管组织设置的权限，发布不需要，还少踩一个已知 UI 坑）；
4. **Generate Token** → 立刻复制令牌（`npm_` 开头，**只显示这一次**），暂存到密码管理器；
5. 令牌只出现在这一行页面与您的剪贴板——任何聊天、issue、日志里都不要再贴它。

## ☑ 第 6 步：GitHub 环境 + 密钥（3 分钟）

在 `OCR-CLI` 仓库网页上：

1. **Settings** → 左栏 **Environments** → **New environment**；
2. 名称填 `npm` → **Configure environment**；
3. **Required reviewers** → Add yourself（这就是将来 CI 发布前的人工审批门：发布作业会停在这里等您点批准）；
4. 同页 **Environment secrets** → **Add secret**：
   - Name：`NPM_TOKEN`
   - Secret：粘贴第 5 步的令牌
5. 保存。发布工作流（方案阶段 8 的 `publish-npm` job）声明 `environment: npm` 后就会受这个门保护。

## ☑ 第 7 步（可选，建议做）：验证令牌真的能通

本机任意终端：

```bash
curl -s -H "Authorization: Bearer npm_xxxxxxxx" https://registry.npmjs.org/-/whoami
```

- 返回 `{"username":"<您的用户名>"}` → 令牌有效；
- 返回 401/403 → 回第 5 步检查范围与有效期。

（令牌对 `@ocrx` 的发布权限要到第一个包真的发布时才完全验证——CI 首发的 `npm publish --dry-run` 与发布门就是最终验证。）

---

## 常见问题

| 症状 | 原因与解法 |
| --- | --- |
| 建组织 / 建令牌时 403 | 邮箱未验证（回第 2 步） |
| 组织名提示被占用 | 2026-11-26 核查后被抢注了——回方案 D-1 启用备选 scope（`@ocrx-cli` 等），改动面只有 package.json 与模板 |
| CI 发布报 `ENEEDAUTH` / 403 | secret 名不对（必须是 `NPM_TOKEN`）、令牌过期、或令牌范围没覆盖目标包 |
| 令牌疑似泄漏 | 立即到 Access Tokens 页 **Revoke** → 重新生成 → 更新 GitHub secret（旧令牌即刻作废） |
| 手机丢了进不了账号 | 用第 3 步存的恢复码；也没有就找 npm 支持（所以恢复码务必存好） |

## 完成清单（全勾完即可回方案阶段 8.5 画勾）

- [ ] npm 账号已注册、邮箱已验证
- [ ] 2FA 已开启、恢复码已妥善保存
- [x] `@ocrx` 组织已创建（2026-11-26 完成，org 页 Packages 0 已独立验证）
- [x] 插件 scope 取舍已定：`@ocrx/dsh`（2026-11-26，见 `docs/develop/dsh-plugin-naming-decision-2026-11-26.md`）
- [ ] Granular Access Token 已创建（scope `@ocrx` 读+写、有效期已知）
- [ ] GitHub environment `npm`（含 required reviewer）与 secret `NPM_TOKEN` 已配置
- [ ] `curl whoami` 验证通过（或明确跳过）

做完这些，npm 侧的仓外前置就绪；剩余动作全部回到仓库内（阶段 8 的流水线实现）。
