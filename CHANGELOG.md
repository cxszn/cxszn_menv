# Changelog

All notable changes to MEnv are documented here.

## [0.0.2] - 2026-10-13 (Developer Preview)

Windows x64 unsigned developer preview, distributed as an EXE installer and a
ZIP application bundle. This prerelease does not represent completed production
acceptance; see the release record for remaining gates.

### Fixed

- Explicitly enabled external PHP keeps its activation and first-seen state
  after a rescan without changing its ownership.
- App-owned PHP shims are excluded from runtime inventory and rejected as
  enable/switch targets, including canonical self-reference aliases.
- Native GUI and CLI snapshot reads share recovery; already registered install
  journals no longer trigger a rescan or invalidate a pending confirmation.
- Native GUI and CLI PHP catalog installation status both match the exact
  version, architecture and TS/NTS build, so a TS installation does not hide
  the NTS download; CLI ini discovery accepts `Loaded Configuration File:`.

### Security

- Native confirmations now require an actual issued SHA-256 record, bind the
  displayed plan and full manual-registration payload, and recheck expiry
  after waiting for operation locks. Legacy unrecorded approvals are refused.
- Native `change confirm` requires console stdin/stderr and an explicit
  `approve <full-plan-UUID>` response; headless/piped approval is refused.

- PHP ini backups bind the displayed source path and SHA-256 to confirmation,
  verify a create-new artifact before recording success, and refuse generic
  apply without a backup port. Failures retain the artifact for recovery rather
  than deleting a possibly replaced file by name.
- Native writes always require a bound token even when confirmation metadata
  is false. Historical operations without a plan hash remain readable but
  cannot be used as idempotent replay targets.

### Added

- Pending PHP installation plans can be cancelled durably. Cancelled plans
  stay cancelled across restart and stale-window retries; installations that
  have started cannot be cancelled through this pending-plan action.

- Desktop startup now performs one read-only runtime scan when no prior scan
  record exists, while preserving intentionally empty post-scan state on later
  launches.
- Persisted, unexpired PHP install plans can now be recovered after a desktop
  restart; recovery only reopens the confirmation dialog and never downloads
  without a fresh user confirmation.
- PHP details now provide a local, read-only `php.ini` structure check for
  malformed sections, directives, duplicate keys and truncated content.
- PHP details can now create a confirmation-bound, create-new backup of the
  selected `php.ini` under the app-owned data root without editing the source.
- Rust/Tauri environment state and PHP install plans now use bundled SQLite WAL
  with FULL synchronous writes and first-open migration from legacy JSON files
  without deleting those sources; append-only install journals remain separate.
- Windows NSIS now embeds the WebView2 bootstrapper instead of downloading the
  bootstrapper at install time; the installer still requires network access to
  retrieve the WebView2 runtime itself.
- Rust application plan/apply boundary with SHA-256 plan/state binding, expiry,
  typed confirmation token, idempotency and rollback on PATH persistence failure.
- HKCU-only PATH lease with external-entry preservation, concurrent-change refusal,
  `WM_SETTINGCHANGE`, same-volume app-owned PHP hard-link shim, cross-volume native
  launcher shim, and Windows 11 x64 roundtrip tests.
- Windows ConPTY adapter with absolute executable/argv/cwd validation, explicit
  environment, synchronous I/O workers, timeout/output limits, and per-session
  Job Object process-tree cleanup.
- Ed25519 catalog verifier, fixed official-host policy, staging path planning,
  artifact size/SHA-256 checks and negative tests.
- Tauri PHP catalog listing plus plan/confirmation/apply download commands for
  PHP 8.2.34, 8.3.35, 8.4.26, 8.5.11 and EOL/legacy 8.1.34, 8.0.30, 7.4.33;
  successful installs remain disabled.
- Rescans preserve ownership, activation and first-seen state for an already
  managed/manual executable instead of resetting it to detected/disabled.
- ConPTY reader failures and output-budget violations terminate the session
  child; published PHP directories are rolled back when the durable `published`
  journal record cannot be written.
- PHP install recovery now discovers both legacy root-level journals and the
  current per-version `operations\\php\\<version>\\install.jsonl` layout while
  rejecting symlink/reparse journal entries.
- Windows runtime writes now share a user-scoped named mutex across Tauri/CLI
  processes and return `OPERATION_BUSY` after the bounded wait expires.
- ConPTY acceptance now includes a Win11 child-tree sentinel test proving a
  spawned child is terminated with the parent Job Object.
- Added a Win11 shutdown-cancel child-tree test covering explicit terminal close
  rather than only natural parent exit.
- Tauri PHP catalog install plans now persist fixed artifact metadata and plan
  hashes across app restarts without storing confirmation tokens.
- Manual PHP registration plans now persist verified runtime metadata across
  restarts without storing confirmation tokens.
- Windows JSON state writes now use synced temporary/backup replacement and can
  recover a valid backup when the primary state file is malformed.
- JSON state staging filenames now include an atomic per-process suffix to avoid
  collisions during concurrent saves.
- Windows runtime scans now share the operation mutex before persisting
  discovered state, preventing scan/install/PATH write races.
- TypeScript core apply now persists `running` before mutation and preserves a
  concurrent `cancelled` state with PATH rollback.
- Core tests now exercise cancellation during a blocked PATH write and verify
  the lease returns to its last-known-good state.
- JSON state loading now rejects unsupported schema versions explicitly.
- Native CLI now exposes persisted `operation get/cancel` with explicit
  `OPERATION_NOT_FOUND` and `OPERATION_NOT_CANCELLABLE` semantics.
- Catalog install failures now preserve stable error codes such as
  `HASH_MISMATCH`, `SIZE_MISMATCH`, `ARCHIVE_REJECTED`, and `DOWNLOAD_FAILED`
  instead of collapsing into a generic platform error.
- Added a live hash-mismatch rollback acceptance test covering staging cleanup,
  no managed-directory publish, and `failed` journal recording.
- Added a repeatable previous-to-current NSIS overlay smoke for upgrade/rollback
  preparation without touching the user's installed directory.
- Windows bundle CI now executes ConPTY and installer smoke after building the
  unsigned NSIS artifact.
- Windows bundle CI now executes the fixed-source PHP live install and hash
  mismatch rollback acceptance tests with an explicit network opt-in.
- Desktop UI now translates stable native catalog and operation error codes into
  actionable Chinese recovery messages.
- Added a read-only `Test-PhpActivationSmoke.ps1` acceptance helper that checks
  a fresh-process PATH view, `where php`, `php -v`, `php --ini`, and `php.target`.
- Added confirmation-bound manual PHP path registration in the Tauri panel;
  registration remains manual/disabled and never writes the external executable.
- Catalog PHP installs now append a shared `install` operation record with the
  plan id and idempotency key for activity/audit consistency.
- Added read-only Windows Node.js discovery for PATH, common nodejs locations,
  and nvm-windows version directories; no Node PATH ownership is performed.
- Added read-only Windows Python discovery for PATH, `PYTHONHOME`, and common
  Python directories; `py.exe`, venv, and PATH ownership remain out of scope.
- Added read-only MySQL/MariaDB client discovery using `mysql.exe --version`;
  service, port, data-directory, and PATH management remain out of scope.
- Added a narrow JSON CLI mode to the installed executable for doctor, runtime
  scan/list, PHP catalog listing, and confirmation-bound enable/disable plans;
  arbitrary shell and `--yes` bypasses remain unavailable.
- Native CLI environment snapshots redact individual PATH entries while keeping
  lease metadata and hashes.
- Native CLI also supports confirmation-bound `runtime add` for PHP paths and
  rejects enablement plans for unsupported Node/Python/MySQL families.
- Embedded PowerShell sessions now inject the current shared PATH lease snapshot
  at session start instead of inheriting a stale application PATH.
- Fixed PowerShell terminal lifecycle commands for the desktop shell: start,
  bounded input, output polling and close through the ConPTY adapter.
- Tauri current-user NSIS configuration, Windows bundle workflow and unsigned
  install/launch/uninstall smoke evidence.

### Known limitations

- A checked-in Ed25519 trust root, signed PHP artifact manifest, fixed-source Rust
  installer adapter, and Tauri plan/confirm/apply wiring now exist; complete
  user-visible GUI install rollback evidence is still pending.
- ConPTY real child output binding, output-budget termination, and wait/shutdown
  behavior pass on the current Win11 x64 host; embedded terminal and long-running
  child-tree user acceptance is still pending.
- The local installer is unsigned and must not be treated as a trusted release.
- Tauri invokes the shared Rust service for Windows PHP scanning and PATH/shim
  activation; cross-volume activation and catalog installation still require
  user-visible Windows acceptance with the rebuilt package.

## [0.0.1] - 2026-10-11

首个开发预览版本。此版本发布的是可审查的核心契约和开发骨架，不是可在
Windows 11 干净机器上直接安装的生产发行版。

### Added

- Windows 11 优先的 MEnv workspace、Rust/Tauri 迁移骨架和 Vue 3 面板原型。
- PHP 5.6–8.5 版本模型、`php -n -v` / `php --ini` 探测、TS/NTS 与架构解析。
- 只读扫描、手动路径登记、availability/ownership/activation/health/operation
  状态分离。
- `plan -> confirmation token -> apply` 变更流程、用户级 app-owned lease、
  plan hash、幂等 key、操作状态和失败回滚原型。
- `menv` CLI JSON 契约和本机 stdio MCP（8 个严格 schema 工具）。
- Canonical operator Skill、catalog schema/fixture、契约校验脚本和 ADR；Skill 的实际 Codex discovery 未在本机验收。
- JSON 文件持久化，以及 Node 22.5+/24 opt-in SQLite/WAL text adapter。

### Changed

- 所有 workspace/package/Tauri/Cargo 版本统一为 `0.0.1`。
- GUI、CLI、MCP 均通过共享 `packages/core` application service 访问状态和变更逻辑。

### Known limitations

- 当前仓库未声明开源许可证；此公开预览不授予生产使用、再分发或商用许可。
- 安全问题处理方式见 [SECURITY.md](./SECURITY.md)。
- 本版本没有预编译 Tauri installer/updater，也不附带 PHP、Node、MySQL 或 Python
  二进制；`0.x` 的 CLI/MCP/JSON schema 允许在后续版本发生不兼容调整。
- 尚未实现真实 Windows 注册表/HKCU PATH、shim、`WM_SETTINGCHANGE`、ConPTY、
  Job Object、下载/签名校验、Zip 安全、原子安装、`php.ini` 写入和 `php -S`。
- SQLite 仍是 opt-in text adapter，完整 typed schema、跨表事务、journal、锁和
  崩溃恢复尚未完成。
- Rust/MSVC/WebView2 未在本机编译验证；桌面端仍使用 mock service。
- Node/nvm、MySQL、Python 和 macOS 适配器不属于此版本。

### Verification

- 7 个测试文件、49 个 TypeScript/Vitest 测试通过。
- Core、CLI、MCP TypeScript 类型检查通过。
- Vue 类型检查与 Vite production build 通过。
- catalog/Skill validator 通过，MCP schema validator 通过 8 个工具。
- Rust 编译和 Windows 11 实机验收未完成。
