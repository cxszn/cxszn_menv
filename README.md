# MEnv 0.0.1

MEnv 是 Windows 11 优先的多运行时开发环境管理器。`0.0.1` 是首个公开开发预览版本，交付了 PHP 版本发现、手动路径登记、状态分离、用户级 lease 编排、CLI、本地 stdio MCP、Canonical Skill、catalog schema/fixture、Vue 面板原型和 Rust/Tauri 宿主骨架。Node/nvm、MySQL、Python 和 macOS 只保留扩展契约，未伪造可用实现。

版本变更见 [CHANGELOG.md](./CHANGELOG.md)，发布门禁见 [tasks/release-0.0.1.md](./tasks/release-0.0.1.md)。
当前仓库尚未声明开源许可证；本版本源码公开不等于授予生产、再分发或商用许可。
这是 `0.x` 预览 API，公共 JSON、CLI 和 MCP schema 仍可能在后续版本发生不兼容调整。
安全问题报告方式见 [SECURITY.md](./SECURITY.md)。

`0.0.2` Windows 原生工作目前仍是 unreleased candidate；真实 signed catalog、
ConPTY child I/O、PHP 生命周期接线和签名安装包通过前，不会创建正式 release。

## 已实现的边界

- PHP 探测使用绝对本地路径，解析版本、x64/x86、TS/NTS、`php --ini` 和健康诊断；支持 5.6–8.5 的版本模型，未验证的组合不会被标成可下载。
- 扫描默认为只读，状态分离为 availability、ownership、activation、health 和 operation；手动路径只登记元数据，不复制、覆盖或删除原文件。
- 变更统一走 `plan → host-issued confirmation token → apply`，有 plan hash、过期、actor、幂等 key 和操作状态；CLI/MCP 共享稳定 envelope。MCP 本身不从模型文字生成 token。
- MCP 仅本机 stdio，stdout 只输出 JSON-RPC；当前工具不暴露任意 shell、URL 下载、artifact 安装或文件删除。Skill 随源码提供并通过静态校验，尚未证明已被 Codex runtime 自动发现。
- catalog fixture 明确标记为不可下载；生产签名 catalog、真实下载和安装器尚未接入。
- 本版本没有预编译 Tauri installer/updater，也不附带 PHP、Node、MySQL 或 Python 二进制。

## 当前验证边界

本机 Node/pnpm 可用；Rust/Cargo 和 Windows MSVC 工具链尚未安装，因此 Rust/Tauri 只能确认源码结构和 JSON 配置，不能声称已编译。核心默认使用 `%LOCALAPPDATA%\\cxszn-menv` 下的 JSON 状态/计划/lease 文件作为阶段性 adapter；另提供 Node 22.5+/24 的 opt-in SQLite/WAL text adapter，但完整 schema 迁移、跨表事务、journal/锁和崩溃恢复仍未完成。真实 Windows 注册表/HKCU PATH、shim、WM_SETTINGCHANGE、ConPTY、下载签名验证、Zip 安全、PHP `php.ini` 写入和 `php -S` 尚未接入。

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

上述 workspace 验证已取得：49 个 TypeScript/Vitest 测试通过、Core/CLI/MCP 类型检查通过、桌面端和 Tauri 前端 Vite build 通过、CLI lint（当前映射为 TypeScript 检查）通过、catalog/Skill validator 通过、8 个 MCP 工具 schema 通过。Rust 原生编译和 Windows 11 实机验收尚未取得证据。

安全和协作规则见 [AGENTS.md](./AGENTS.md)。
