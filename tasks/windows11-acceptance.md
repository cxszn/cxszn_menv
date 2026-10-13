# Windows 11 x64 实机验收矩阵

## 目的与当前结论

本文件定义 MEnv 0.0.2 之前必须取得的 Windows 11 x64 证据。它是验收协议，
不是“已经通过”的声明。当前仓库已经打开 Tauri NSIS bundle 并增加了 Windows
unsigned developer-artifact workflow，但 `crates/platform-windows` 的 PHP
provider 已具备只读 PATH/常见目录扫描和显式 php.exe 探测，catalog 签名下载、真实 PATH/shim、
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
| WIN-INSTALL-001 | 从有效 catalog 下载 PHP zip 到隔离 staging，再完成健康探测 | 解压路径检查、entrypoint 检查、`php -n -v`/`--ini` 通过后才原子 rename；完整目录不可变；安装不自动启用 | C + W + U | adapter W 通过；应用接线/U 未验证 |
| WIN-ROLLBACK-001 | 版本 A active 时，在 downloading/staged/verifying/registered/activated 各阶段注入可控失败 | journal 可恢复；staging 清理；A、shim、PATH lease 和 last-known-good 不变；不留下半目录 | C + W | 未验证 |
| WIN-ROLLBACK-002 | 重复相同 idempotency key，取消 running operation，应用崩溃后重启 | 重试复用同一 operation；取消有终态；重启能从 journal 恢复或安全回滚，不重复安装 | C + W | 未验证 |
| WIN-TAURI-001 | 在 Windows 11 x64 用 release 配置生成 NSIS/MSI 安装包 | `bundle.active=true`，版本与 tag 一致；安装包和可选签名元数据可复核；不把 dev server/mock UI 当 release | S + C + W | 未验证 |
| WIN-TAURI-002 | 普通用户安装、启动、关闭、再次启动、卸载 | 应用可见版本正确；首次确认路径写入前显示差异；不修改机器 PATH/Profile；卸载不删除外部 PHP；安装日志可关联 request/operation id | W + U | 未验证 |
| WIN-TAURI-003 | 对已安装 0.0.2 做同版本重装和后续升级/失败升级 | 用户数据和受管运行时按策略保留；失败升级回到 last-known-good；无重复快捷方式、lease 或 shim | W + U | 未验证 |

## 目录与 PATH 证据采集

实机报告中只保存哈希、计数和布尔结果，不保存完整环境内容。至少记录：

PHP shim 的新终端命令行检查可使用只读脚本：

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-PhpActivationSmoke.ps1 `
  -ExpectedVersion 8.5.11
```

脚本只在自身进程中合并 PATH，不写 HKCU、机器 PATH 或 runtime 文件；输出 JSON
可直接保存为验收证据。

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

## 本机已取得的分层证据（2026-10-13）

- 最新只读预检 `artifacts/acceptance/preflight-latest.json`：Windows 11 x64
  Build 26300、embedded-WebView2 包哈希、catalog revision 2/7 artifacts 和机器
  PATH 边界均通过；WebView2 Runtime 注册表入口仍为 `warn`，因此该文件仍是
  `preflight_only`，不是 GUI/U 验收通过。

- `cargo test --workspace` 全部通过；platform-windows 当前为 39 个测试通过、8 个
  会修改本机/联网或 console-host 验收的用例保持 ignored。ConPTY 输出上限、Job
  Object、wait/shutdown 和 GUI-subsystem child-I/O smoke 已通过；console-host
  child-I/O 用例因宿主输出竞态保持 ignored。
- Win11 x64 只读预检：Build 26300、x64、用户/机器 PATH 均未发现 MEnv lease。
- HKCU PATH/shim roundtrip ignored test：通过；临时 lease、hard-link shim 和
  用户 PATH 均恢复，机器 PATH 未变化。
- Tauri NSIS：本机重新生成 `cxszn_menv_0.0.2_x64-setup.exe`；当前测试包 SHA-256 为
  `2365C9C1E4834F1311BE1225BF7C92DF7A6CD13E043622CA2155AAC8E2CDBE4E`，大小
  `5,267,755` bytes，Authenticode 为 `NotSigned`，只能作为 unsigned developer artifact。
- Previous-to-current NSIS overlay smoke：旧 0.0.2 preview 安装后覆盖当前 preview，
  两阶段 `doctor --json` 成功，最终 uninstaller exit `0`，临时目录已移除。
  既有包的安装/启动/卸载 smoke 已通过；本轮新包尚未覆盖用户现有安装目录。
- 最新 unsigned 包已在隔离临时目录完成安装/启动/卸载 smoke：installer exit `0`、
  release `doctor --json` 返回 `ok=true`、uninstaller exit `0`、临时安装根已移除；
  未触碰现有安装目录。
- 最新包 SQLite migration smoke 已在隔离数据根通过：legacy `state-rust.json`
  可迁移为 `state-rust.sqlite3`，源文件保留，doctor、卸载和临时目录清理均成功。
- Shim 已覆盖跨磁盘场景：同盘使用 hard-link，跨盘复制受信任 `cxszn-menv.exe`
  launcher 并使用 `php.target` 转发；版本切换的 owned-shim 替换测试通过。
- 当前用户安装目录已取得跨盘 shell 级实机证据：`php.target` 指向
  `R:\App\Env\php\php8.5.11\php.exe`，注入最新用户 PATH 后新进程的
  `where.exe php`、`php -v` 和 `php --ini` 均成功；旧终端仍需重开。
- 本轮新增扫描状态回归保护：同一 executable 重新扫描时保留既有 managed/manual
  ownership、activation 和 first-seen 状态；应用测试已通过，尚未替代完整 GUI W/U 验收。
- Tauri catalog 列表、安装计划和确认应用命令已完成接线；安装包内 catalog
  smoke 列出七个版本，七个固定 PHP x64 来源 HEAD 均为 HTTP 200，Content-Length
  与 catalog 一致，完整用户级 GUI 安装回滚仍待验收。
- 七个 catalog artifact 已完成隔离目录 live 安装验收：8.2.34、8.3.35、8.4.26、
  8.5.11、8.1.34、8.0.30、7.4.33 的下载大小、SHA-256、ZIP 安全解压、managed
  发布和 `php -n -v` 均通过；每个版本的 append-only journal 均记录到 `published`；
  Tauri GUI plan/apply、`registered` 后续阶段和用户可见回滚仍待验收。
- Tauri 读取环境状态时会安全扫描受限 journal 根及一层版本子目录，并恢复已发布
  受管版本；进程崩溃后完整 journal 重放、跨进程锁和用户可见回滚仍需 W/U 验收。
- 本轮新增 journal recovery 安全门：managed 根外路径、缺失/非普通 `php.exe` 和
  symlink/reparse 组件均 fail-closed；Windows Tauri 单元测试通过。
- Windows runtime write operations now use a user-scoped named mutex; contention returns
  `OPERATION_BUSY` after a bounded wait and does not alter PATH, shim or state.
- Node provider 已在 Win11 x64 实机验证 `R:\App\Env\nodejs\node.exe`：通过扩展盘符
  前缀规范化探测到 Node 24.15.0、x64，保持 manual/disabled，不写 PATH。
- Python provider 已在 Win11 x64 实机验证 `C:\Users\17276\AppData\Local\Python\bin\python.exe`：
  探测到 Python 3.14.5、x64，保持 manual/disabled，不接管 `py.exe`、venv 或 PATH。
- Python provider 另以 `C:\Windows\py.exe` 验证 launcher 识别：探测到 Python 3.14.5、
  x64，并标记为 `py.exe launcher; selected interpreter may vary.`，仍保持只读。
- MySQL provider 已加入只读 `mysql.exe --version` 探测契约；真实客户端 W/U 样本待有
  MySQL/MariaDB 客户端安装后再执行，不连接服务、不启动服务、不读取数据目录。
- `runtime_discovery_smoke` 示例已在本机运行并输出四个 provider 的 JSON 结果；本机
  发现 Node 24.15.0 多个 nvm 版本、Python 3.14.5/3.11.15，PHP/MySQL 因当前
  provider 搜索根/客户端状态未发现，未伪造可用入口。
- 该 smoke 支持 `MENV_TEST_*_PATH` 显式路径证据；使用 `MENV_TEST_PHP_PATH` 后已
  验证 PHP 8.5.11 的 provider 直接探测，仍不登记、不修改 PATH。
- 当前 release binary 已实测 `doctor --json` 和 `catalog list --kind php --json`：
  schema_version 为 `1`，catalog 返回 7 个 artifact。
- release binary 的 `runtime verify --family php` 已验证 PHP 8.5.11；对 Node enable
  写操作返回 `UNSUPPORTED_ACTION`，未生成或执行无效 shim 计划。
- 同一 release binary 已通过 `runtime verify` 路由实测 Node 24.15.0/x64 和 Python
  3.14.5/x64；两者均保持只读，不生成启用计划。
- release CLI `doctor --json` 已验证 PATH entry 脱敏：包含 `<redacted>`，不泄露完整
  用户 PATH，同时保留 lease/hash 元数据；catalog/verify 结果仍正常。
- native CLI plan/confirm 流程已用当前 active PHP 生成并确认过一次，未执行 apply；
  随后已清理 plan 文件，用户 PATH 和 PHP active 状态保持不变。
- native CLI `runtime add --path` 已验证 PHP 8.5.11 registration plan 和 confirmation
  token 生成；未执行 apply，测试 plan 文件已清理。
- 最新 release binary 已再次通过同一 `runtime add` plan/confirm 实测，确认交付包
  包含该 CLI 能力；测试文件已清理。
- 固定官方 PHP 8.5.11 NTS x64 下载/安装 adapter：显式运行
  `MENV_RUN_LIVE_WINDOWS_INSTALL=1 cargo test ... --ignored` 后，36,048,897
  bytes、SHA-256 `0ea96e0d2b9b737a6036f05cf4e95c49313faa6d0f27bd97edb2742503f0c043`、
  Zip 安全解压、`php -n -v` 健康探测和原子 managed-directory 发布通过；专用
  `acceptance-install-test` 目录随后已清理。该证据不等于 catalog/GUI/journal/U 验收。
- GUI-subsystem ConPTY smoke：构建 `conpty_smoke.exe` 后由
  `scripts/windows/Test-ConptyGuiSmoke.ps1` 使用隐藏窗口启动，child 输出读取和
  `wait/shutdown` 返回 exit code `0`；cargo test 的输出上限和长驻 child-tree
  sentinel 回收用例也在当前 Win11 x64 主机通过。普通 cargo test 的
  console-host child-I/O 用例因宿主输出竞态保持 ignored。完整嵌入终端用户级
  业务验收仍未完成。

## 当前缺口索引

- Windows provider 的只读扫描/探测：`crates/platform-windows/src/php.rs`；仍需 Win11
  实机 PHP 安装样本和用户可见验收。
- ConPTY adapter 已具备结构化输入、原生 child I/O、超时、输出上限和 Job Object
  进程树回收代码；GUI-subsystem smoke、输出上限、长驻 child-tree sentinel 和
  shutdown cancel 测试已通过，console-host child-I/O 用例因宿主输出竞态保持
  ignored；完整嵌入终端用户验收仍待补充。
- Live hash-mismatch 用例已验证下载后 staging 清理、managed 目录不发布和 `failed`
  journal；GUI 级失败回滚仍待补充。
- 当前 Tauri 只生成 unsigned developer bundle；release signing/实机安装验收仍未完成：
  `apps/tauri/src-tauri/tauri.conf.json:28-48`、`.github/workflows/windows-bundle.yml:1-89`。
- TypeScript core 当前 lease 仍是内存/JSON 抽象；Rust 侧已增加 HKCU-only
  `WindowsUserPathStore` 并完成 Rust/MSVC 编译和静态接线，但跨磁盘 shim 的新包
  GUI/read-back 用户验收仍待补证据：`packages/core/src/path-lease.ts:64-90`、
  `crates/platform-windows/src/path.rs:1-320`。
- 主 CI 仍没有把 Windows runner 的实机验收作为 release gate；独立 workflow 只构建
  unsigned developer bundle，不执行安装/卸载/回滚验收：`.github/workflows/ci.yml:13-47`、
  `.github/workflows/windows-bundle.yml:1-89`。
- 当前 catalog 是使用仓库内开发信任根校验的静态签名清单；清单和信任根分别位于
  `catalog/manifests/php-windows-official.json` 与 `catalog/trust/runtime-root.json`，
  但仍不能替代真实下载、解压、安装和回滚证据。

这些索引只用于定位待实现工作；它们不会降低本矩阵的验收等级。

## 2026-10-13 原生 GUI 本轮实测（部分通过，整体不通过）

- 已通过 Computer Use 操作用户打开的实际 MEnv 窗口，并在安装前取得 PHP 8.3.35
  NTS x64 的明确许可；GUI 正常路径安装、managed/disabled 展示及独立产物探测通过。
- 外部 PHP 8.5.11 的 GUI 确认启用日志为 succeeded，但随后页面仍无活动版本；
  app-owned shim 被重复发现且可进入启用计划，TS/NTS catalog 变体被错误合并。
- 同一安装路径的 CLI 与 GUI 实际状态不一致，原因待定位。用户/机器 PATH 和外部
  PHP executable 的前后哈希相同，shim 的独立进程检查仍为 8.5.11。
- 内嵌终端只观察页面，没有通过 UI 自动输入终端命令；不据此完成 ConPTY U 验收。
- 详见 `tasks/windows11-gui-acceptance-2026-10-13.md` 和
  `artifacts/acceptance/windows11/2026-10-13-native-gui/` 的四张截图。
  这些实测失败阻断正式发布；本轮没有修改生产源码或替换用户安装包。

## 2026-10-13 后续修复候选 r2（W/U 待续）

- 已修复上述扫描 activation、own-shim/launcher alias、TS/NTS 和 ini 解析问题；
  所有新增行为先红后绿，Rust 84 passed / 8 原有 ignored，TypeScript 75 passed。
- 共用 native GUI/CLI 状态恢复入口，已登记 journal 不再重扫。r2 CLI 的 doctor/list
  快照一致、真实 PHP 探测和自有 shim 拒绝通过；尚不证明新 GUI/CLI U 一致性。
- 新 NSIS 包为 `artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-gui-state-fix-20261013-r2/`，
  installer SHA-256 `3567E9305BF5C165073BAA6051D54AC0CF3428A399D68A2C97D455068E92D951`。
  未执行此安装器、未覆盖原安装；独立审查批准代码切片，完整发行门禁保持未完成。
- 用户批准了独立 EXE 启动，但工具尚未取得该 EXE 的可控原生窗口；Codex 的同名
  窗口不允许自动化，也不作为 MEnv 验收。完整记录与下一步见
  `tasks/windows11-gui-state-fix-2026-10-13.md`。
