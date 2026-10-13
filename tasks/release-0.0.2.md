# Release 0.0.2 开发者预览发布记录

## 当前定位

按用户 2026-10-13 明确授权推送仓库并发布 EXE/ZIP，`v0.0.2` 以 GitHub prerelease
交付 Windows 原生 unsigned developer preview。该授权不把未完成验收标成完成；
正式发行仍受完整 GUI/终端/回滚、干净机、许可证及可信签名发行所需检查约束。

发布页：https://github.com/cxszn/cxszn_menv/releases/tag/v0.0.2

资产由最终提交后的源码重新构建，`scripts/windows/New-PreviewRelease.ps1` 仅打包
指定 EXE、使用说明、第三方声明和构建信息；不包含用户数据或诊断备份。
source commit、每个资产的大小与 SHA-256 记录在发布目录的 release-manifest.json
及 ZIP 内 build-info.json；版本页另提供 SHA256SUMS.txt。

## 2026-10-13 r4 最新接续记录

最新测试候选为 `artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-confirmation-cancel-20261013-r4/`。
本轮 138 项 Rust、83 项 TypeScript 测试通过；真实 EXE 的管道确认/伪造 token 拒绝，
以及隔离 GUI 取消重启保持通过。详细范围、校验值和限制见
`windows11-r4-acceptance-2026-10-13.md`。r4 未执行安装器、升级、卸载或正式发布。

原生 CLI 不再允许自动确认；旧 smoke 的自动 confirm 成功与清理计划描述是历史行为，
新脚本明确验证无控制台时拒绝签发，保留隔离证据。下面的旧安装/启用/备份结果
属于对应历史候选，不能当作 r4 新交互规则的正向验收。

## 历史已取得证据

- [x] Windows 11 x64 host preflight：Build 26300。
- [x] `cargo check --workspace`。
- [x] `cargo test --workspace`（16 application、1 core、3 domain、39
      platform-windows 实测通过、8 个联网/用户状态或 console-host 验收测试保持
      ignored、1 ports、9 tauri）。console-host ConPTY child-I/O 由 GUI-subsystem
      smoke 覆盖。
- [x] `cargo clippy --workspace --all-targets -- -D warnings`。
- [x] `pnpm test/typecheck/build/lint/validate:contracts`。
- [x] catalog Ed25519/staging/size/SHA-256 negative tests。
- [x] checked-in Ed25519 trust root and signed PHP official artifact manifest;
      `pnpm catalog:verify` prints fixed, non-writing plans.
- [x] Windows installer adapter contract for fixed-source HTTPS download,
      size/SHA verification, safe ZIP staging and atomic managed-directory
      publish，以及 Tauri catalog 列表/安装计划/确认应用命令。
- [x] Catalog live install：8.2.34、8.3.35、8.4.26、8.5.11、8.1.34、8.0.30、
      7.4.33 七个 x64 artifact 均完成真实下载、SHA-256、ZIP 安全解压、原子
      发布、`php -n -v` 探测和 `published` journal 记录。
- [x] Tauri 状态读取会恢复受管 PHP journal 中已发布但尚未登记的版本；journal
      路径限制在应用自己的 managed PHP 根目录内。
- [x] 跨磁盘 PHP shim：同盘 hard-link，跨盘复制受信任 `cxszn-menv.exe` launcher
      并通过 `php.target` 转发到绝对 PHP 路径；路径安全与跨卷判定测试通过。
- [x] 重新扫描同一路径时保留既有 managed/manual ownership、activation 和
      first-seen 状态，避免启用成功后被扫描重置为 detected/disabled。
- [x] 新增只读 Windows PHP activation smoke 脚本，能在当前用户/机器 PATH
      合并的模拟新终端中验证 `where php`、`php -v`、`php --ini` 和 `php.target`。
- [x] Tauri 面板支持确认绑定的手动 PHP 路径登记；登记不会复制、覆盖或启用外部文件。
- [x] Windows RuntimeProvider 已接入 PHP、Node.js、Python、MySQL/MariaDB client
      只读扫描；未完成启用适配的 family 在 UI 中显示为“仅检测”。
- [x] `runtime_discovery_smoke` 示例输出四个 provider 的只读 JSON 证据。
- [x] 手动登记和 catalog 安装均写入共享 operation 审计记录，并按 plan/idempotency
      key 处理重试。
- [x] 安装后的 native executable 提供 JSON CLI（doctor、runtime scan/verify、runtime add、catalog list、
      change plan/confirm/apply），复用同一个 Rust application service，不启动 GUI、
      不开放任意 shell。
- [x] ConPTY PowerShell session 启动时显式注入当前 PATH lease 快照，旧会话不热切换。
- [x] HKCU PATH + hard-link shim roundtrip，恢复用户 PATH，机器 PATH 未变。
- [x] unsigned NSIS current-user installer build。
- [x] Windows bundle workflow 在打包前执行 workspace Rust test/clippy 和 desktop
      test/typecheck/build，避免上传未通过 Windows MSVC 自动检查的 bundle。
- [x] 最新 unsigned 包隔离临时目录安装/启动/卸载 smoke：installer、release doctor、
      uninstaller 均返回成功，临时安装根已移除。
- [x] Previous-to-current installer overlay smoke：旧 preview 安装后覆盖当前 preview，
      两阶段 `doctor --json` 均成功，最后卸载临时目录成功。
- [x] Windows bundle workflow now runs ConPTY GUI-subsystem smoke and installer
      install/launch/uninstall smoke after building the unsigned artifact.
- [x] Windows bundle workflow runs the opt-in fixed-source seven-artifact live
      install and hash-mismatch rollback acceptance tests.
- [x] Windows bundle manifest records installer size, SHA-256 and Authenticode
      status for artifact verification.
- [x] 将上述流程固化为 `scripts/windows/Test-InstallerSmoke.ps1`，后续 preview 可重复执行。
- [x] `scripts/windows/Test-NativeCliSmoke.ps1` 已验证 release CLI doctor/catalog/verify
      和 unsupported-family fail-closed，不执行 scan/apply、不写用户状态。
- [x] CLI smoke 同时验证 PHP `runtime add` plan/confirm，并在结束后清理 plan 文件。
- [x] 当前最终 preview 包再次通过 Installer smoke 和 Native CLI smoke，包内容与源码同步。
- [x] 既有 unsigned installer install/launch/uninstall smoke；本轮新包已构建，
      尚未覆盖用户现有安装目录。
- [x] ConPTY GUI-subsystem smoke、输出上限终止和 wait/shutdown：Win11 x64 实测通过；
      `conpty_smoke.exe` 隐藏窗口 smoke 返回 exit code `0`；新增 child-tree sentinel
      用例验证 Job Object 回收。console-host child-I/O 用例因宿主输出竞态保持
      ignored；完整嵌入终端用户验收仍待完成。
- [x] Windows named mutex 跨进程锁：竞争方 bounded wait 返回 `OPERATION_BUSY`，
      释放后可再次获取；Tauri、native CLI 和 PHP install/registration/apply 均接入。
- [x] Tauri PHP install plan JSON persistence：固定 artifact spec、plan hash、snapshot
      和 idempotency key 可 round-trip；确认 token 不落盘，过期 plan 启动时丢弃。
- [x] Native CLI operation get/cancel：查询持久化 operation，queued/running 可取消，
      已完成 operation 返回不可取消错误。
- [x] Catalog install failure envelope：SHA、大小、ZIP、entrypoint、网络和已存在版本
      返回稳定错误码，并保留 retryable/next_actions 信息。
- [x] Live hash-mismatch rollback：真实下载后 SHA 错误会清理 staging、不发布 managed
      目录，并在 journal 写入 `failed`。

远程 CI：

- TypeScript/contracts: https://github.com/cxszn/cxszn_menv/actions/runs/38159501967
- Windows x64 NSIS unsigned bundle: https://github.com/cxszn/cxszn_menv/actions/runs/38159501885

本机 unsigned installer 证据（最新构建）：

- `artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-php-ini-backup-cli/cxszn_menv_0.0.2_x64-setup.exe`
- SHA-256: `2365C9C1E4834F1311BE1225BF7C92DF7A6CD13E043622CA2155AAC8E2CDBE4E`
- Size: `5,267,755` bytes
- Authenticode: `NotSigned`
- Local current-user install/launch/uninstall smoke: installer exit `0`, MEnv
  process responsive, uninstaller exit `0`, default directory
  `%LOCALAPPDATA%\cxszn_menv` removed.
- Package catalog smoke: installed executable listed all seven signed PHP
  artifacts; cross-volume launcher smoke forwarded `php -n -v` to the R: drive
  PHP 8.5.11 without changing HKCU PATH.
- Cross-process runtime mutation protection: the Windows named mutex returns
  `OPERATION_BUSY` after a bounded wait and releases cleanly for the next apply.

本轮用户问题回归证据：

- [x] `R:\App\Env\php\php8.5.11\php.exe` 与用户级 `C:` shim 的跨磁盘
      launcher 可执行，`php -n -v` 返回 PHP 8.5.11；新进程 PATH smoke 中
      `where php`、`php -v`、`php --ini` 均通过。
- [x] 真实下载并安装 catalog 中全部七个 PHP x64 artifact（8.2.34、8.3.35、
      8.4.26、8.5.11、8.1.34、8.0.30、7.4.33），每个均通过固定大小、SHA-256、
      安全 ZIP 解压、原子发布和 `php -n -v` 健康探测。
- [x] 新包 installer/doctor/uninstaller smoke 通过，临时安装目录已清理。
- [x] 直接使用最新 NSIS 包执行真实 `runtime enable -> confirm -> apply`，跨磁盘
      PHP 8.5.11 启用返回 `succeeded`；包内 executable 的新进程 PATH smoke 通过，
      临时安装目录已卸载并清理。
- [x] 最新包包含首次启动只读扫描策略；桌面端 startup-scan 回归测试、typecheck、
      build 以及包内启用 smoke 均通过。
- [x] 最新包包含安装计划重启恢复：持久化未过期计划可被原生查询，桌面 service
      重新绑定计划后只显示确认对话框；Tauri service 回归测试和包内 smoke 通过。
- [x] 最新包包含 php.ini 本地只读结构校验；桌面端校验回归测试、typecheck、build
      以及包内安装/启用 smoke 均通过。
- [x] 最新包切换 PHP 版本时会清空旧版本的 php.ini 内容和校验结果，避免详情页
      显示错误版本的配置诊断；桌面构建和包内 smoke 通过。
- [x] 最新包使用 bundled SQLite/WAL 保存 Rust/Tauri 环境状态；隔离迁移测试、
      workspace test/clippy、安装 smoke 和跨磁盘 PHP 启用 smoke 通过。
- [x] 最新包将 PHP install plans 写入同一 SQLite/WAL 文件；旧 plans JSON 可迁移且保留，
      plan rows 事务回读测试、workspace check 和包内 smoke 通过。
- [x] `Test-SqliteMigrationSmoke.ps1` 使用隔离数据根验证最新包的 legacy
      `state-rust.json` → `state-rust.sqlite3` 首次迁移、源文件保留和卸载清理。
- [x] 安装 journal 每个生产阶段会继续写入 JSONL，同时镜像到 SQLite
      `menv_install_journal` 审计表；隔离 journal 行顺序测试通过。
- [x] NSIS 改为内嵌 WebView2 bootstrapper；最新包安装/卸载和 PHP 启用 smoke 通过，
      文档明确仍需网络获取 WebView2 Runtime，不宣称完全离线安装。
- [x] 最新包加入 confirmation-bound php.ini backup；备份只写入应用数据根，
      不修改外部配置，Tauri check、桌面构建和包内 smoke 通过。
- [x] native CLI `change plan --action backup-ini` 与 `change apply` 复用同一
      plan/token/application-service 边界；Tauri check、测试和包内 smoke 通过。

## 阻断项

- [x] ConPTY GUI-subsystem child I/O、输出上限终止、基础 Job Object wait/shutdown、
      长驻 child-tree sentinel 回收和 shutdown cancel 在 Win11 主机通过；
      console-host child-I/O 用例保持 ignored。
- [ ] 完整嵌入终端用户验收仍待补充。
- [x] Fixed-source PHP 8.5.11 download, exact size/SHA, safe ZIP extraction and
      atomic adapter publish passed the opt-in Win11 test; this is adapter-level
      evidence only.
- [ ] Catalog download/install 的完整用户级 Windows 下载、解压、回滚和崩溃恢复证据。
- [ ] 跨磁盘 shim 的新安装包 GUI 启用以及新终端 `where php` / `php -v` / `php --ini`
      用户验收。
- [x] Windows PHP provider scan/manual path 与 Tauri runtime state 的基础接线；
      完整用户级生命周期验收仍待完成。
- [ ] installer Authenticode signing、证书链和签名验收。
- [ ] 普通用户完整 GUI/U 级验收和升级/回滚验收。

上述门禁继续约束正式发行。本次用户授权允许创建 `v0.0.2` tag 与明确标记为
prerelease 的未签名预览版本，不宣称这些门禁已经关闭。
