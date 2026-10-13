# Windows 11 0.0.2 r4 接续验收 — 2026-10-13

## 来源与范围

从会话 `01a12ab4-faf0-7431-be51-1be24d019a47` 的中断点接续。
原会话最后两次请求返回 `invalid_encrypted_content`，最近实际执行的切片是
原生签发确认校验与 CLI 交互确认；测试包构建后停在 schema/README 更新。
本会话恢复了原记录，并核对工作树，没有重置或回退既有改动。

沿用用户已确认的选择：保留 `change confirm`，要求可交互终端与明确的计划确认。
本轮同时完成原切片已列出的“取消尚未开始的 PHP 安装计划”。
这不代表授权真实下载安装、切换 PHP、覆盖原安装或创建正式 release。

## 本轮检查与修复

- 确认校验继续使用共享签发记录和 SHA-256 摘要；明文 token 不写入证据。
- 确认前核对可见计划与其摘要，登记计划绑定完整运行时记录。
- 等待操作锁后重新检查批准是否仍有效。
- 取消尚未开始的安装计划要持久化，重启后不可恢复或通过旧计划执行。
- 修订旧冒烟脚本对自动确认的错误预期，使用独立测试目录。
- 原生确认 schema 新增可重复运行的正负 fixture 校验。

## 验证记录

本轮证据目录：
`artifacts/acceptance/windows11/2026-10-13-r4-ec00440644114f8381c4e7bc5eb89dc3/`。

| 层次 | 实际结果 |
| --- | --- |
| Rust 测试 | 138 passed / 0 failed / 8 既有 ignored；application 30、core 1、domain 3、Windows adapter 50、ports 1、Tauri 53 |
| TypeScript 测试 | 83 passed：desktop 29、core 29、MCP 11、CLI 14 |
| 静态与构建 | cargo fmt、clippy all-targets warnings denied、pnpm typecheck/lint/build、Windows x64 NSIS bundle 通过 |
| 契约 | 8 个 MCP 工具、catalog/Skill 契约及原生 schema 的 2 个合法形状/16 个非法形状通过 |
| 独立审查 | 确认与取消链路复审后无未解决代码阻塞；审查期间发现的登记实际路径展示和失败弹窗退出已修复 |
| Windows EXE | 最终候选 doctor/catalog/verify、无控制台确认拒绝、伪造 token apply 拒绝通过 |
| 用户可见 | 隔离原生 GUI 创建 PHP 8.4.26 计划、取消、重启不恢复；注入 started 记录后取消拒绝，独立关闭保留计划通过 |

`rust-tests-final.log`、`rust-clippy.log` 和 `windows-bundle-final.log` 保留命令输出。
本轮新增回归取得先红后绿证据，包括未实现取消接口、显示内容/摘要不一致、登记
runtime 被替换、旧登记计划、锁后到期、真实路径展示及错误状态关闭按钮。

### 最终 EXE 的拒绝行为

`native-confirmation-smoke.json` 使用假的运行时和 php.ini，运行实际 r4 EXE：

- stdin 管道即使输入精确批准短语，或传入 EOF，均返回 `CONFIRMATION_REQUIRED`。
- 未签发值执行 apply 返回 `CONFIRMATION_INVALID`。
- 状态、源文件、旧 JSON 与计划文件不变；没有备份目录、没有 operation。
- stdout 只有一条 JSON envelope，stderr 独立，未保存明文 token。

`native-cli-smoke.json` 使用另一个隔离数据根，只读探测已有 PHP 8.5.11，
确认 7 个 catalog 入口、登记计划、无控制台拒绝与 operation 错误码。
它没有登记或启用真实 PHP。

### 原生 GUI 的实际操作

最终候选进程/窗口已按 EXE 绝对路径与 HWND 交叉确认：首次 PID 111676 /
HWND 5637848，重启 PID 194700 / HWND 404164，错误状态实例 PID 135624 /
HWND 5701992。工具把它们分组在启动者 Codex 名下，但实际窗口属于 MEnv EXE。
重启时一次截图被其他窗口遮挡，未对该截图输入；聚焦已核实的 MEnv 窗口后重新观察。

- `gui-localappdata` 内只有一个人工测试 runtime；不代表真实 PHP 安装。
- 通过 GUI 选择 PHP 8.4.26，确认页出现后点击取消；SQLite 记录从 pending 变为
  cancelled，确认记录数为 0。没有点击“确认下载”。
- 关闭该测试实例并用同一隔离数据根重启；在五分钟计划有效期内，总览正常显示，
  已取消计划没有恢复弹窗。SQLite cancelled 记录仍存在。
- 另建 `gui-error-localappdata`，从测试库复制并仅把测试记录改为 started，模拟
  已开始后不能取消的状态。GUI 点击取消显示 `OPERATION_NOT_CANCELLABLE`；
  点击“关闭（保留计划）”返回总览，数据库中的 started 记录保留，确认记录为 0。
  这是注入状态下的真实 GUI 验证，不是实际网络下载失败或安装回滚验收。
- 两个测试实例均已关闭，测试状态与证据保留。

`gui-before-cancel.json`、`gui-after-cancel.json`、`gui-after-restart.json` 和
`gui-error-dismiss.json` 保留相应只读状态核对结果。

### 用户数据对照

后续诊断确认，以下 AppData 文件对照属于 Codex 包重定向上下文，不能等同于用户
从桌面启动的实际数据。新的物理路径核验与用户现场恢复见
`windows11-activation-user-context-2026-10-13.md`。R 盘 EXE 的对照不受此限制。

`user-baseline.json` 与 `user-baseline-after.json` 对照通过：用户 PATH 摘要、真实
MEnv SQLite/旧 JSON 文件、`R:\App\Code_Tool\cxszn_menv\cxszn-menv.exe` 及
`R:\App\Env\php\php8.5.11\php.exe` 的哈希未变化。未运行安装器、切换真实 PHP、
下载 PHP、执行提交/推送或发布。

## 本轮候选包

`artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-confirmation-cancel-20261013-r4/`

- 安装器 5,297,307 bytes，SHA-256：
  `8EF0B395B0D42602B46C31928CEE483C71E6C12248319C56F36BE3A5EA2EAD99`。
- EXE 14,048,768 bytes，SHA-256：
  `4CC4A9880A5C6609771FE3A94B0211EE00C4FC6DDD4C9DB8CFE970B6D3C8DE67`。
- 两者 Authenticode 均为 `NotSigned`。安装器构建成功，不代表 r4 安装/升级/卸载已验收。
- 原安装与 r3 候选保留。旧未记录的 token、旧未绑定 runtime 的登记计划需要重新
  生成和确认；已有环境状态仍保留。

原 r3 的历史 GUI/备份证据见 `windows11-r3-acceptance-2026-10-13.md`，不能将其
当作 r4 新确认机制的实测。

## 仍独立于本切片的门禁

- 真人终端输入确认的正向验收；控制台句柄不是人的身份认证。
- 正在进行的下载的主动中断、完整安装 journal/SQLite 崩溃事务恢复。
- 完整内嵌终端、跨卷 GUI/W/U、真实配置备份、干净机与升级/卸载验收。
- 许可证决策与正式发布；当前仍是未签名开发者测试候选。
