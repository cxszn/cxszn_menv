# MEnv 0.0.2 developer preview

MEnv 是 Windows 11 优先的多运行时开发环境管理器。`0.0.2` 当前是 unsigned developer preview，交付了 PHP 只读扫描/手动探测、Node/Python/MySQL client 只读扫描、HKCU PATH lease、app-owned shim、共享 Rust application service、Tauri NSIS current-user 安装包、ConPTY 结构化适配器、Ed25519 catalog 信任根/官方 PHP artifact 清单和安全安装器 adapter。Node/Python/MySQL 尚未接入安装、切换或 PATH 接管；macOS 仍只保留扩展契约。

安装后的 `cxszn-menv.exe` 提供 JSON CLI：`doctor`、`runtimes list`、`runtime scan/verify`、`catalog list --kind php`、`operation get/cancel`，以及 `runtime add/enable/disable` 的 `change plan → change confirm → change apply`；写操作仍必须经过共享确认边界。

版本变更见 [CHANGELOG.md](./CHANGELOG.md)，发布门禁见 [tasks/release-0.0.2.md](./tasks/release-0.0.2.md)。
当前仓库尚未声明开源许可证；本版本源码公开不等于授予生产、再分发或商用许可。
这是 `0.x` 预览 API，公共 JSON、CLI 和 MCP schema 仍可能在后续版本发生不兼容调整。
安全问题报告方式见 [SECURITY.md](./SECURITY.md)。

`0.0.2` 以 GitHub prerelease 形式交付，仍是未签名开发者预览版。下载
[EXE 安装包与 ZIP 程序包](https://github.com/cxszn/cxszn_menv/releases/tag/v0.0.2)，
使用说明见 [Windows 预览版说明](./docs/windows-preview.zh-CN.md)。catalog/GUI 完整回滚、
终端、干净机和正式发行门禁仍待完成；第三方声明见 [THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md)。

## 已实现的边界

- PHP 探测使用绝对本地路径，解析版本、x64/x86、TS/NTS、`php --ini` 和健康诊断；支持 5.6–8.5 的版本模型，未验证的组合不会被标成可下载。
- PHP 详情页支持读取并本地只读验证 `php.ini` 的节、指令、重复键和截断状态；不会执行配置、修改文件或自动调参。
- Windows 扫描已增加 Node.js 只读 RuntimeProvider：检查 PATH、常见 nodejs 目录和 nvm-windows 版本目录，探测版本/架构但不接管 PATH 或 nvm。
- Windows 扫描也增加 Python 只读 RuntimeProvider：检查 PATH、`PYTHONHOME`、常见 Python 目录并区分 `python.exe`/`py.exe` launcher，探测版本/架构但不接管 `py.exe`、venv 或 PATH。
- Windows 扫描也增加 MySQL/MariaDB client 只读 RuntimeProvider：仅执行本地 `mysql.exe --version`，不连接服务、不启动服务、不访问数据目录。
- 扫描默认为只读，状态分离为 availability、ownership、activation、health 和 operation；手动路径只登记元数据，不复制、覆盖或删除原文件。
- 首次启动在没有历史扫描记录时自动执行一次只读环境扫描；之后保留空环境结果，不反复扫描或自动接管外部运行时。
- Tauri 面板已支持手动登记 PHP：绝对路径验证后必须确认 plan，登记保持 manual/disabled，不会修改 PATH 或外部文件。
- 手动登记会写入共享 operations 审计记录，使用 plan id 和幂等 key，重试不会重复登记。
- catalog PHP 安装成功后同样写入 `install` operation，活动日志和状态快照可追踪安装结果。
- catalog 安装失败会返回明确的 `HASH_MISMATCH`、`SIZE_MISMATCH`、`ARCHIVE_REJECTED`
  或 `DOWNLOAD_FAILED` 等错误码，不会统一伪装成平台错误。
- 变更统一走 `plan → host-issued confirmation token → apply`，有 plan hash、过期、actor、幂等 key 和操作状态；CLI/MCP 共享稳定 envelope。MCP 本身不从模型文字生成 token。
- PHP 下载计划在桌面重启后可恢复未过期的确认对话框；恢复过程只读，不会绕过用户确认自动下载。
- MCP 仅本机 stdio，stdout 只输出 JSON-RPC；当前工具不暴露任意 shell、URL 下载、artifact 安装或文件删除。Skill 随源码提供并通过静态校验，尚未证明已被 Codex runtime 自动发现。
- `catalog/trust/runtime-root.json` 和 `catalog/manifests/php-windows-official.json` 是可审计的静态签名清单；`pnpm catalog:verify` 只生成不写盘的下载计划。
- 当前 catalog 可下载 PHP 8.2.34、8.3.35、8.4.26、8.5.11，以及标记为 EOL/legacy 的 8.1.34、8.0.30、7.4.33；5.6 和更老的 7.x 因缺少可验证官方 x64 artifact 暂不显示为可下载。
- `crates/platform-windows/src/install.rs` 的 adapter 已实现固定来源、大小/SHA、Zip 安全检查和原子目录发布契约；Tauri GUI 已接入 PHP catalog 列表、确认计划和安装动作，安装完成仍保持 disabled，不附带 PHP、Node、MySQL 或 Python 二进制。
- PHP shim 同盘使用 hard-link，跨盘（例如 `R:` PHP 与 `C:` 用户数据根）使用受信任的 `cxszn-menv.exe` launcher 转发，不复制 PHP 本体；新终端才会读取更新后的用户 PATH。
- 内嵌 PowerShell 终端启动时会读取当前共享 PATH lease 快照，显式注入新会话环境；已有会话不会热切换。
- 桌面端已接入固定 Windows PowerShell 的终端生命周期命令：启动、输入、输出轮询和关闭；命令不接受任意 executable 或启动参数。GUI-subsystem ConPTY child I/O、输出上限和 wait/shutdown 已在 Win11 x64 实测通过；console-host child-I/O 用例因宿主输出竞态保持 ignored，完整嵌入终端用户验收仍待完成。
- Windows bundle workflow 故意产出 unsigned NSIS developer artifact；本机安装/启动/卸载 smoke 已通过，不能据此宣称 SmartScreen 或发行商信任。NSIS 内嵌 WebView2 bootstrapper，安装时仍需要网络；完全离线安装包尚未纳入。
- Windows NSIS 默认安装目录名称为 `cxszn_menv`；显示名称仍为 `MEnv`。用户可以选择其他本地目录，例如 `R:\App\Code_Tool\cxszn_menv`。

## 当前验证边界

本机已验证 Node/pnpm、Rust/Cargo、Visual Studio MSVC；`cargo check/test/clippy`、隔离状态全量测试、HKCU PATH/shim roundtrip 和 unsigned NSIS 构建/既有 smoke 均有证据。Rust/Tauri 环境状态和 PHP install plans 默认使用 `%LOCALAPPDATA%\\cxszn-menv\\state-rust.sqlite3` 的 SQLite/WAL adapter，并从旧 JSON 文件迁移；安装 journal 仍以 JSONL 作为恢复源并镜像到 SQLite 审计表，统一 journal 事务和完整崩溃恢复仍未完成。GUI-subsystem ConPTY child I/O、输出上限和 wait/shutdown 已在 Win11 x64 通过；console-host child-I/O 用例因宿主输出竞态保持 ignored。七个 PHP catalog artifact 的 adapter live 安装已通过，Tauri GUI 回滚和用户可见验收仍未通过。

## 开发命令

```powershell
pnpm install
pnpm test
pnpm typecheck
pnpm build
```

首选验证命令是 workspace 命令：

```powershell
pnpm test
pnpm typecheck
pnpm build
pnpm lint
pnpm validate:contracts
pnpm catalog:verify
```

如果本机 pnpm registry 或 workspace 链接异常，当前工作区还保留以下仅供本机
排障的直接验证命令；其中 `node_modules/.ignored` 是本机依赖布局，不是发布契约：

```powershell
node node_modules/.ignored/vitest/vitest.mjs run --reporter=dot
node node_modules/.ignored/typescript/bin/tsc --noEmit --typeRoots node_modules/.ignored/@types -p packages/core/tsconfig.json
node node_modules/.ignored/typescript/bin/tsc --noEmit --typeRoots node_modules/.ignored/@types -p packages/cli/tsconfig.json
node node_modules/.ignored/typescript/bin/tsc --noEmit --typeRoots node_modules/.ignored/@types -p packages/mcp/tsconfig.json
npm --prefix apps/desktop run build
.\\.agents\\skills\\cxszn-menv-operator\\scripts\\validate-contract.ps1 -RepoRoot .
node node_modules/.ignored/tsx/dist/cli.mjs packages/mcp/scripts/validate-contracts.ts
```

上述 workspace 验证已取得：TypeScript/Vitest 测试、Core/CLI/MCP 类型检查、桌面端和 Tauri 前端 Vite build、CLI lint、catalog/Skill validator、8 个 MCP 工具 schema，以及 Windows MSVC `cargo check/test/clippy`。Windows 11 HKCU PATH/shim、ConPTY child I/O 和 unsigned installer smoke 有实机证据；artifact 安装回滚和完整 GUI 用户验收仍未取得证据。

安全和协作规则见 [AGENTS.md](./AGENTS.md)。
