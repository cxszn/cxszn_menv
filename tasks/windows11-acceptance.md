# Windows 11 x64 实机验收矩阵

## 目的与当前结论

本文件定义 MEnv 0.0.2 之前必须取得的 Windows 11 x64 证据。它是验收协议，
不是“已经通过”的声明。当前仓库已经打开 Tauri NSIS bundle 并增加了 Windows
unsigned developer-artifact workflow，但 `crates/platform-windows` 的 PHP
provider 仍会返回 `UnsupportedPlatform`，catalog 签名下载、真实 PATH/shim、
ConPTY 和用户验收仍未完成。因此本矩阵在全部实现和证据取得前必须保持为
`未验证`，不能用静态构建、unsigned installer 或 mock UI 代替 Windows 实机证据。

## 证据等级

| 等级 | 含义 | 可以证明 | 不能证明 |
|---|---|---|---|
| S | 静态证据 | 配置、schema、源码约束、发布元数据存在 | Windows API、注册表、进程树或用户体验正确 |
| C | CI/自动化证据 | 可重复测试、类型检查、打包命令和负面用例通过 | 当前机器的 HKCU PATH、ConPTY、UAC 或安装器行为 |
| W | Windows 11 x64 实机证据 | 指定版本、指定账户和指定快照上的真实行为 | 其他 Windows 版本、升级路径或未知用户环境 |
| U | 用户可见验收 | 页面、终端、提示、回滚结果符合预期 | 未执行的后台步骤和未保存的证据 |

一项能力只有在要求的最低等级全部取得后才可标记为 `通过`。`生成安装包`、
`下载完成`、`注册 MCP`、`文件存在`或`静态测试通过`均不能单独完成 W/U 项。

## 干净机器基线

每次 W 验收都使用可恢复的 Windows 11 x64 VM 快照，记录但不上传以下信息：

1. Windows edition、build、架构、PowerShell 版本、WebView2 版本和本地账户类型。
2. MEnv 安装包文件名、SHA-256、版本、签名状态和 catalog revision/key id。
3. 快照前的用户 PATH 哈希、机器 PATH 哈希、`%LOCALAPPDATA%\\cxszn-menv`
   是否存在，以及 `HKCU\\Environment` 是否已有 MEnv lease。
4. 测试使用的 PHP artifact id、版本、x64/TS-NTS、SHA-256 和可用磁盘空间。

基线要求：普通用户运行；不预先添加 MEnv 目录到机器 PATH；不修改 PowerShell
Profile；不使用生产密钥；不把完整 PATH、完整环境变量、confirmation token 或
命令输出中的敏感值写入报告。需要联网的 catalog/下载用例必须记录固定来源和
redirect allowlist；离线负面用例优先使用本地副本。

可先运行只读预检：

```powershell
pwsh -NoProfile -File .\\scripts\\acceptance\\Test-Windows11AcceptancePreflight.ps1 `
  -RequireWindows11 -Json -OutputPath .\\artifacts\\acceptance\\preflight.json
```

预检只报告环境和证据缺口，不安装、下载、修改注册表、修改 PATH、创建 shim、
启动终端或删除文件。它的输出不是验收通过证明。

## 验收矩阵

| ID | 场景与最小步骤 | 预期结果/回滚条件 | 最低证据 | 当前状态 |
|---|---|---|---|---|
| WIN-SCAN-001 | 干净快照启动应用，执行首次 scan/doctor | 无 PHP 时返回空列表；不创建受管版本；PATH、Profile 和外部目录前后哈希不变 | S + C + W + U | 未验证 |
| WIN-SCAN-002 | 在快照外准备可运行的外部 PHP，放入用户 PATH 后扫描 | 显示 `detected-unmanaged`、`disabled`；不复制、不接管、不删除；版本、架构、TS/NTS、ini 来源可读 | C + W + U | 未验证 |
| WIN-SCAN-003 | 重复 PATH、大小写不同路径、无效目录、UNC、设备路径、父级 reparse path | 重复项去重；非法输入返回明确错误码；不访问或接管被拒绝路径 | S + C + W | 未验证 |
| WIN-MANUAL-001 | 对外部 `php.exe` 执行 `plan -> 用户确认 -> apply` 登记 | ownership 为 `manual`、activation 为 `disabled`；只保存规范化路径和探测结果；原文件哈希不变 | C + W + U | 未验证 |
| WIN-MANUAL-002 | 登记后重新扫描、重启应用，再尝试 disable/uninstall | 手动路径仍可发现；应用没有删除、移动、覆盖或改写外部 PHP | W + U | 未验证 |
| WIN-PATH-001 | 在无 MEnv lease 的快照启用受管 PHP | 只向 HKCU PATH 加入应用自有 lease 或 shim 目录；机器 PATH、第三方条目、Profile 不变；新终端可见 | S + W + U | 未验证 |
| WIN-PATH-002 | 在 plan 后、apply 前由另一进程向用户 PATH 加入 sentinel | apply 返回 `PATH_CONFLICT`；sentinel 和原 PATH 原样保留；不留下半个 lease/shim | C + W | 未验证 |
| WIN-PATH-003 | 已有外部 PHP 条目时启用和停用 MEnv PHP | 启用时保留外部条目；停用只移除 MEnv 自有 lease/shim；外部 PHP、机器 PATH 和 Profile 不变 | W + U | 未验证 |
| WIN-SHIM-001 | 启用版本 A，核对 `%LOCALAPPDATA%\\cxszn-menv\\bin\\php.exe`，再切到版本 B | shim 只指向已校验绝对路径；版本切换有 journal 和 last-known-good；失败时 A 和 PATH 保持 | S + C + W + U | 未验证 |
| WIN-SHELL-001 | 在切换前打开 PowerShell A；切换到 B；再打开 PowerShell B | 旧终端保持 A；新终端使用 B；不热修改旧进程环境；`where php`、`php -v`、`php --ini` 一致 | W + U | 未验证 |
| WIN-CONPTY-001 | 启动内嵌 PowerShell/命令提示符，执行版本诊断、输出上限、超时和 Ctrl-C | 会话绑定启动时 snapshot；使用绝对 executable/argv、受控 cwd、普通用户和 Job Object；超时/取消可回收进程树 | S + C + W + U | 未验证 |
| WIN-CONPTY-002 | 终端中启动一个长驻 PHP 子进程，再停用/切换运行时 | 停用或切换不杀已有 PHP 进程；新会话使用新状态；关闭会话后子进程按 Job Object 规则回收 | W + U | 未验证 |
| WIN-CAT-001 | 使用固定来源、有效期内、正确 key id 的签名 catalog 和匹配 artifact | 校验签名、有效期、最小 app 版本、撤销列表、大小、SHA-256、架构和来源后才进入 staging；安装完成仍为 `disabled` | S + C + W | 未验证 |
| WIN-CAT-002 | 逐项测试未知 key、无效签名、过期、撤销 artifact、降级和架构不符 | fail-closed；不下载/不解压/不登记；返回稳定错误码，不改受管目录 | C + W | 未验证 |
| WIN-CAT-003 | 测试 SHA 不符、大小不符、固定域名外 redirect、UNC/reparse、Zip Slip、压缩炸弹、磁盘不足 | staging 被清理；既有版本和 PATH 不变；不覆盖既有版本；operation 为 failed/cancelled | C + W | 未验证 |
| WIN-INSTALL-001 | 从有效 catalog 下载 PHP zip 到隔离 staging，再完成健康探测 | 解压路径检查、entrypoint 检查、`php -n -v`/`--ini` 通过后才原子 rename；完整目录不可变；安装不自动启用 | C + W + U | 未验证 |
| WIN-ROLLBACK-001 | 版本 A active 时，在 downloading/staged/verifying/registered/activated 各阶段注入可控失败 | journal 可恢复；staging 清理；A、shim、PATH lease 和 last-known-good 不变；不留下半目录 | C + W | 未验证 |
| WIN-ROLLBACK-002 | 重复相同 idempotency key，取消 running operation，应用崩溃后重启 | 重试复用同一 operation；取消有终态；重启能从 journal 恢复或安全回滚，不重复安装 | C + W | 未验证 |
| WIN-TAURI-001 | 在 Windows 11 x64 用 release 配置生成 NSIS/MSI 安装包 | `bundle.active=true`，版本与 tag 一致；安装包和可选签名元数据可复核；不把 dev server/mock UI 当 release | S + C + W | 未验证 |
| WIN-TAURI-002 | 普通用户安装、启动、关闭、再次启动、卸载 | 应用可见版本正确；首次确认路径写入前显示差异；不修改机器 PATH/Profile；卸载不删除外部 PHP；安装日志可关联 request/operation id | W + U | 未验证 |
| WIN-TAURI-003 | 对已安装 0.0.2 做同版本重装和后续升级/失败升级 | 用户数据和受管运行时按策略保留；失败升级回到 last-known-good；无重复快捷方式、lease 或 shim | W + U | 未验证 |

## 目录与 PATH 证据采集

实机报告中只保存哈希、计数和布尔结果，不保存完整环境内容。至少记录：

```powershell
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
[pscustomobject]@{
  userPathSha256 = (Get-FileHash -Algorithm SHA256 -InputStream ([IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes($userPath ?? '')))).Hash
  machinePathSha256 = (Get-FileHash -Algorithm SHA256 -InputStream ([IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes($machinePath ?? '')))).Hash
  userHasMenvLease = (($userPath -split ';') -contains "$env:LOCALAPPDATA\cxszn-menv\bin")
  machineHasMenvLease = (($machinePath -split ';') -contains "$env:LOCALAPPDATA\cxszn-menv\bin")
}
```

上面的片段用于证据采集示意；生产验收应使用脚本输出，避免复制完整 PATH。若
`machineHasMenvLease` 为 `true`，验收立即失败并恢复快照，不尝试覆盖机器 PATH。

## Catalog 负面用例要求

每个负面样本必须保存：原始样本的 SHA-256、变更点、期望错误码、实际错误码、
staging/managed 目录前后清单哈希和 PATH lease 前后哈希。样本不得包含私钥。至少
包括：

- 未知 key、签名值修改、签名算法不匹配。
- `expires_at` 已过期、`min_app_version` 高于当前版本、artifact 在撤销列表。
- SHA-256、文件大小、平台或架构不匹配。
- 非固定 host、未允许 redirect、任意 URL、UNC/reparse 路径。
- Zip Slip、绝对路径、符号链接/junction escape、压缩比异常和磁盘预算不足。

任何一个失败样本导致现有 active 版本、PATH 或已登记手动路径变化，均判为阻断
缺陷，不得进入 0.0.2 发布。

## 安装包与回滚证据包

每次 W 验收输出一个不含 secrets 的目录，建议结构如下：

```text
artifacts/acceptance/windows11/<run-id>/
  preflight.json
  installer.sha256.txt
  catalog-summary.json
  cases.jsonl
  path-before.json
  path-after.json
  journal-summary.json
  screenshots/
  logs-redacted/
```

`cases.jsonl` 的每条记录至少包含 `case_id`、`started_at`、`finished_at`、
`evidence_level`、`result`、`error_code`、`runtime_id`、`operation_id`（如有）、
`artifact_sha256`（如有）和 `notes`。不要写入 token、完整命令、完整环境变量或
未脱敏 stdout/stderr。

## 0.0.2 发布门禁

在所有 W/U 用例取得证据前，版本只能标记为开发预览，不能发布为 Windows 安装版。
发布前必须同时满足：

- Windows adapter 不再返回 `UnsupportedPlatform`，并有真实 Win11 x64 scan/manual
  path 证据。
- HKCU PATH lease、shim、并发冲突、旧/新终端和 ConPTY 用例全部通过。
- 有真实签名 catalog、有效和负面下载/解压/回滚证据；artifact 安装后保持 disabled。
- Tauri release bundle 已构建、哈希/签名可核对，普通用户安装/卸载/升级可重复。
- `pnpm test/typecheck/build/lint/validate:contracts`、Windows 自动化测试和本矩阵
  的 W/U 证据都通过；任何未执行项显式列为 release blocker。
- 发布 notes 明确 OS、架构、artifact 来源、已知限制、回滚方式和许可证状态。

## 本机已取得的分层证据（2026-10-12）

- `cargo test -p cxszn-menv-platform-windows`：12 个纯契约测试通过，2 个会修改
  本机/启动真实子进程的验收用例保持 ignored。
- Win11 x64 只读预检：Build 26300、x64、用户/机器 PATH 均未发现 MEnv lease。
- HKCU PATH/shim roundtrip ignored test：通过；临时 lease、hard-link shim 和
  用户 PATH 均恢复，机器 PATH 未变化。
- Tauri NSIS：本机生成 `MEnv_0.0.2_x64-setup.exe`，安装后 `MEnv` 窗口可响应，
  卸载后安装目录和 HKCU uninstall entry 均移除。该包 Authenticode 为 `NotSigned`，
  只能作为 unsigned developer artifact。
- ConPTY 真实 child I/O、签名 catalog 下载/解压、PHP artifact 安装和完整
  Tauri 业务接线仍未取得 W/U 通过证据，不能据此发布稳定 `v0.0.2`。

## 当前缺口索引

- Windows provider 占位：`crates/platform-windows/src/lib.rs:18-26`。
- ConPTY adapter已经具备结构化输入、超时和输出上限校验，但当前实现尚未取得
  真实 child I/O 通过证据，且 Job Object 进程树隔离仍待补齐；不得把 `cargo test`
  的纯契约测试当作 ConPTY 实机验收。
- 当前 Tauri 只生成 unsigned developer bundle；release signing/实机安装验收仍未完成：
  `apps/tauri/src-tauri/tauri.conf.json:28-48`、`.github/workflows/windows-bundle.yml:1-89`。
- TypeScript core 当前 lease 仍是内存/JSON 抽象；Rust 侧虽已增加 HKCU-only
  `WindowsUserPathStore`，但尚未取得 Rust/MSVC 编译、Tauri 接线和 Win11 read-back
  证据：`packages/core/src/path-lease.ts:64-90`、`crates/platform-windows/src/path.rs:1-320`。
- 主 CI 仍没有把 Windows runner 的实机验收作为 release gate；独立 workflow 只构建
  unsigned developer bundle，不执行安装/卸载/回滚验收：`.github/workflows/ci.yml:13-47`、
  `.github/workflows/windows-bundle.yml:1-89`。
- 当前 catalog fixture 明确不是签名下载清单：`catalog/schema/runtime-catalog.schema.json:68-103`。

这些索引只用于定位待实现工作；它们不会降低本矩阵的验收等级。
