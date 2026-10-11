# MEnv Task List

## 0.0.1 release

- [~] 发布 `0.0.1` source/developer preview（本地版本和 CHANGELOG 已统一；Git commit/tag/远程 release 待最终门禁后创建）
- [x] 生成 `pnpm-lock.yaml` 并在 CI 使用 `--frozen-lockfile`

- [x] 建立 TypeScript workspace、共享 schema 和测试命令（直接 Node/Vitest/tsc 证据已取得）
- [x] 实现 RuntimeProvider/PHP 状态模型与持久化原型（当前为 JSON adapter；SQLite/WAL 未完成）
- [x] 实现 PHP 扫描、手动路径校验和用户级激活 lease 编排（真实 Windows PATH/注册表尚未接入）
- [x] 实现 CLI JSON 命令与 stdio MCP plan/apply 契约
- [x] 实现 Vue 桌面面板原型与可访问状态展示（当前使用 mock service）
- [~] 建立 Rust/Tauri 迁移骨架并在具备工具链后编译验证（骨架完成；Rust/MSVC 未安装）
- [~] 提供 SQLite WAL/迁移 text adapter（已支持 opt-in JSON text storage；三表事务、journal、锁和崩溃恢复仍未完成）
- [ ] 实现签名 catalog 验证、下载 staging、Zip 安全和原子安装
- [ ] 接入 Windows PATH lease、shim、WM_SETTINGCHANGE、ConPTY、Job Object 和 Tauri core service
- [ ] 完成 PHP ini 编辑/备份、`php -S` 和外部 Web Server 只读诊断
- [ ] 增加安全负面测试、打包和 Windows 11 x64 实机验收
- [ ] 增加 Node/nvm、MySQL、Python 适配器及 macOS 平台实现
