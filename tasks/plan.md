# MEnv Implementation Plan

当前执行切片已交付，并作为 `0.0.1` 开发预览候选：Node/TypeScript 核心、CLI、stdio MCP、PHP 扫描/手动路径登记/用户级 lease 编排、签名 catalog schema/fixture、Canonical Skill 和 Vue 桌面 UI 原型；Rust/Tauri 已建立同契约宿主骨架。

这不是“Windows 端到端完成”声明。Rust/Tauri 环境状态已切换到 bundled SQLite/WAL，并保留旧 JSON 状态作为首次迁移源；TypeScript core 仍提供 JSON/Node SQLite text adapter。完整的 plans/journal 统一事务、跨文件崩溃恢复、Windows 注册表/HKCU PATH、shim、ConPTY、下载/签名验证/原子安装和 Tauri 用户验收仍属于后续切片，见 `tasks/todo.md`。

## 验收门槛

- 通过可复现的直接 Node/Vitest 与 TypeScript 命令验证核心切片；pnpm workspace 命令仍需在依赖和 registry 环境稳定后复验。
- CLI 与 MCP 使用相同 envelope 和状态模型。
- 扫描只读；安装/启用默认 disabled；PATH 只允许 app-owned lease。
- MCP 写操作必须 plan/confirmation/apply，不能暴露任意 shell。
- Rust/Tauri 编译状态如工具链缺失必须明确记录，不得冒充运行时通过。

## 已取得的证据

- `node node_modules/.ignored/vitest/vitest.mjs run --reporter=dot`：7 个测试文件、49 个测试通过。
- Core、CLI、MCP 使用 TypeScript 5.8 编译器执行 `tsc --noEmit`：通过。
- `apps/desktop` 的 `vue-tsc --noEmit` 和 Vite production build：通过。
- `.agents/skills/cxszn-menv-operator/scripts/validate-contract.ps1 -RepoRoot .`：通过。
- MCP schema validator：8 个工具通过。
- Rust/Tauri 未编译：当前工作机缺少 Rust/Cargo/MSVC 工具链。

## 0.0.1 发布边界

`0.0.1` 只发布源码、契约和开发预览文档。它不代表可安装的 Windows 11
发行版；Windows adapter、Tauri installer、签名 artifact、真实 PATH/shim、
ConPTY 和实机验收必须在后续版本完成并单独取得证据。
