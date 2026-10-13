# MEnv Task List

## 0.0.2 Windows native (in progress)

- [x] 用户授权发布 `v0.0.2` 未签名 GitHub prerelease，提供 EXE 安装器与 ZIP 程序包；正式发行门禁仍保留，发布校验见 `release-0.0.2.md`。
- [x] 原生确认使用真实签发记录及摘要校验；CLI 交互门禁、完整登记载荷绑定、锁后过期拒绝和无副作用负面测试已补齐（ADR-0016）；真人控制台正向验收仍待完成。
- [x] PHP 待安装计划持久化取消、旧窗口执行拒绝、重复取消幂等和已开始计划拒绝取消已通过自动测试（ADR-0017）；r4 隔离原生 GUI 取消重启保持、注入 started 状态的取消拒绝与保留计划关闭验收通过；实际下载中断不在该能力内。
- [x] Windows 11 x64 toolchain preflight、Rust workspace compile/test
- [x] HKCU-only PATH lease、conflict detection、shim adapter and live roundtrip
- [x] Tauri current-user unsigned NSIS build/install/launch/uninstall smoke
- [x] Ed25519 catalog verifier、fixed-host staging plan、size/SHA-256 fail-closed tests
- [~] ConPTY native adapter and Job Object cleanup（GUI-subsystem child I/O smoke、输出上限、wait/shutdown、长驻 child-tree 回收和 shutdown cancel 已在 Win11 x64 通过；console-host child-I/O 用例因宿主输出竞态保持 ignored；完整嵌入终端 U 验收仍待补）
- [~] Real signed catalog/download/Zip-safe atomic PHP install and rollback（七个 catalog artifact、journal 阶段记录、受限 reparse-safe 恢复和 adapter 恢复已通过；GUI/W/U 回滚待完成）
- [~] 跨进程 Windows named mutex 已接入安装、登记、启用/停用和 CLI apply；双进程完整崩溃/回滚 W/U 仍待完成
- [~] Tauri core service persistence/scan/provider wiring and Win11 user-visible acceptance
      （扫描状态保留、PHP catalog/安装命令和手动 PHP 路径登记已接线；完整 GUI W/U 仍待执行）
- [x] Desktop 首次启动在无历史扫描记录时自动执行一次只读环境扫描；已有空扫描结果不重复扫描。
- [~] Tauri PHP install plan 已迁移到与环境状态同一 SQLite/WAL 文件，并支持旧 JSON
      迁移、重启恢复、幂等 key 和原生计划查询；安装 journal 统一事务和完整崩溃恢复仍待完成
- [x] Tauri manual PHP registration plan 已使用同等 JSON 持久化/过期过滤/幂等 key 边界
- [x] Windows JSON environment state 支持 backup rename、连续保存和主文件损坏恢复；最终 SQLite/WAL 仍待完成
- [x] Native CLI plan/confirm/apply 安全边界、plan 文件和 unsupported family fail-closed ADR
- [x] Native CLI operation get/cancel 查询、queued/running 取消和稳定错误码 ADR
- [x] TypeScript core apply persists running operation and preserves cancelled state with PATH rollback
- [ ] 全部 W/U 门禁与许可证决策完成后发布 `v0.0.2`；用户允许 unsigned 包，签名不作为其发布前置要求，但必须显式标记 NotSigned，不能宣称可信签名安装版。
- [x] PHP 启用状态 GUI 实测：r2 确认启用外部 8.5.11、重新扫描后仍 active；r3 重启读取和 CLI/shim 对照通过，8.3.35 保持 disabled。
- [x] 原生 GUI/CLI catalog 安装状态精确匹配版本、架构和 TS/NTS；3 个先红后绿回归及真实 GUI NTS 下载入口通过。
- [~] php.ini 备份的 source/hash 确认绑定、typed port 实际效果、legacy replay 拒绝与保守 orphan 保留已测试/复审通过；隔离 native CLI 流程通过，真实配置 GUI 和完整崩溃恢复仍待验收。

## 0.0.1 release

- [x] 发布 `0.0.1` source/developer preview（commit `e48e118`、tag `v0.0.1`、GitHub release 已完成）
- [x] 生成 `pnpm-lock.yaml` 并在 CI 使用 `--frozen-lockfile`

- [x] 建立 TypeScript workspace、共享 schema 和测试命令（直接 Node/Vitest/tsc 证据已取得）
- [x] 实现 RuntimeProvider/PHP 状态模型与持久化原型（当前为 JSON adapter；SQLite/WAL 未完成）
- [x] 实现 PHP 扫描、手动路径校验和用户级激活 lease 编排（真实 Windows PATH/注册表尚未接入）
- [x] 实现 CLI JSON 命令与 stdio MCP plan/apply 契约
- [x] 实现 Vue 桌面面板原型与可访问状态展示（当前使用 mock service）
- [~] 建立 Rust/Tauri 迁移骨架并在具备工具链后编译验证（骨架完成；Rust/MSVC 未安装）
- [~] 提供 SQLite WAL/迁移 text adapter（Rust/Tauri 环境状态已迁移到 bundled SQLite/WAL 并保留旧 JSON 源；plans/journal 统一事务和完整跨文件崩溃恢复仍未完成）
- [x] 实现签名 catalog 验证、下载 staging、Zip 安全和原子安装；journal/恢复接线已完成，跨进程事务待后续
- [~] 接入 Windows PATH lease、shim、WM_SETTINGCHANGE、ConPTY、Job Object 和 Tauri core service（GUI/W/U 验收待完成）
- [~] PHP ini 只读查看、结构校验、确认绑定备份与 loopback `php -S` 启停已接入；编辑/模板和外部 Web Server 只读诊断待完成
- [ ] 增加安全负面测试、打包和 Windows 11 x64 实机验收
- [~] Node.js/nvm-windows、Python、MySQL/MariaDB client 只读扫描已接入 Windows RuntimeProvider；安装、切换和 PATH 接管仍不在本切片范围；macOS 仍待实现
