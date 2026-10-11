# Changelog

All notable changes to MEnv are documented here.

## [0.0.2] - Unreleased

Windows native work in progress. This entry is not a release declaration.

### Added

- Rust application plan/apply boundary with SHA-256 plan/state binding, expiry,
  typed confirmation token, idempotency and rollback on PATH persistence failure.
- HKCU-only PATH lease with external-entry preservation, concurrent-change refusal,
  `WM_SETTINGCHANGE`, app-owned PHP hard-link shim and Windows 11 x64 roundtrip test.
- Windows ConPTY adapter contract with absolute executable/argv/cwd validation,
  explicit environment, synchronous I/O workers, timeout and output limits.
- Ed25519 catalog verifier, fixed official-host policy, staging path planning,
  artifact size/SHA-256 checks and negative tests.
- Tauri current-user NSIS configuration, Windows bundle workflow and unsigned
  install/launch/uninstall smoke evidence.

### Known limitations

- No production signed catalog, downloader, Zip-safe extractor, atomic PHP
  registration or journaled install rollback is shipped yet.
- ConPTY real child I/O and Job Object process-tree evidence is still pending.
- The local installer is unsigned and must not be treated as a trusted release.
- Tauri still uses an empty persistence/provider composition for runtime scan;
  the native PATH/shim adapter is not yet a complete PHP lifecycle.

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
