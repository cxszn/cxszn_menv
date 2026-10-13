# MEnv 工程协作与安全规范

## 1. 产品边界

- 首发平台为 Windows 11 x64，首个完整运行时为 PHP 5.6–8.5；实际可下载版本以签名 catalog 中存在的官方 artifact 为准。
- Node（兼容 nvm）、MySQL 5.6–8.4、Python 和 macOS 只能通过 `RuntimeProvider` 扩展；未完成适配器不得伪造可用入口。
- 首版不做 WSL、Docker、项目级 profile、账号同步、机器级 PATH 接管、外部 Web Server 自动安装/重启、MySQL 服务生命周期或默认遥测。

## 2. 仓库结构与依赖方向

- `packages/core` 是唯一的领域与应用编排来源；GUI、CLI、MCP 只能调用它，不能复制启用、PATH、安装或回滚规则。
- `packages/cli` 和 `packages/mcp` 是协议薄适配器；`apps/desktop` 只负责展示、确认和终端承载。
- `crates/` 保存未来 Rust/Tauri 实现的 domain/application/Windows adapter 对应接口，不把 Windows 注册表、Tauri 或 UI 类型泄漏到 domain。
- `catalog/` 只保存可审计的示例和 schema，不提交真实私钥、Token 或未经验证的二进制。

## 3. 运行时不变量

- 状态必须分离为 availability、activation、health、operation；检测到不等于登记，登记不等于安装，安装不等于启用。
- 所有运行时默认 `disabled`。扫描是只读的，不自动接管外部安装。
- `managed`、`manual`、`detected-unmanaged` 的 ownership 必须保留；手动路径不得由应用复制、覆盖或删除。
- 受管版本使用不可变目录和原子 current pointer；启用/停用不杀已有进程，旧终端不热切换。
- 扫描保留同一 executable 的明确 activation，与 ownership 无关；自有 PHP shim
  不进入独立 inventory，也不能成为 enable/switch 目标。修改扫描或原生状态读取时，
  先读 `tasks/ADR-0014-scan-activation-native-read-boundary.md` 并运行其中的回归场景。

## 4. PATH、权限与回滚

- 默认使用当前进程环境注入和 app-owned shim；用户级 PATH 只维护应用自己的 lease，绝不修改机器 PATH、PowerShell Profile 或第三方条目。
- PATH 变更必须读取、规范化、合并、写回、读回校验，并检测外部并发修改；失败时保留原值。
- 安装和激活采用 journal、资源锁、幂等 key、快照和 last-known-good 回滚。
- 受保护目录或机器级操作只能通过一次性、类型化、最小权限 broker + UAC；禁止 shell 字符串、任意 exe、相对路径和常驻 SYSTEM 服务。

## 5. 下载与供应链

- 只允许固定来源的签名 catalog；必须校验签名、key id、有效期、撤销/最小安全版本、SHA-256、大小、架构和路径。
- SHA-256 只证明完整性，不替代发布者签名；下载失败、校验失败、解压路径异常时 fail-closed。
- 禁止任意 URL、未审计 redirect、安装脚本、Zip Slip、UNC、reparse/symlink escape、压缩炸弹和覆盖既有版本。
- PHP 5.6 等 EOL artifact 必须显式标记 legacy/archival；不得默认公网监听。

## 6. CLI、MCP 与 Skills

- CLI、MCP、GUI 使用相同的 `schema_version`、`request_id`、`ok/result/error` envelope。
- MCP 首版仅本机 stdio：stdout 只能输出 JSON-RPC，日志写 stderr；禁止任意 shell、PowerShell、URL、SQL、文件删除和隐式下载。
- 读操作可自动调用；下载、安装、登记、启用、停用、PATH 或配置写入必须 `plan -> 用户确认 token -> apply`。
- 写操作必须支持调用者生成且重试复用的 `idempotency_key`、操作锁、`operation_id`、取消、超时和审计。
- Skills 只描述安全调用流程，不包含 secrets、机器绝对路径或可直接执行的安装脚本；更新后必须验证实际 discovery。

## 7. 终端与日志

- 终端使用绝对 executable 和 argv，普通用户权限、受控 cwd、Job Object、超时和输出上限。
- PHP 健康探测默认使用最小配置，避免加载不受控扩展；项目 `php.ini` 必须显式选择并显示来源。
- 修改 `php.ini` 备份、确认绑定或幂等重放前，读取
  `tasks/ADR-0015-ini-backup-effect-and-recovery.md`。备份使用共享 application 的
  typed port 入口；失败后保留待恢复文件，并报告未完成，不按路径自动删除。
- 日志只记录 actor、request/operation id、runtime、版本、哈希、状态和错误码；不得记录 Token、完整环境变量、完整命令或未脱敏输出。

## 8. 验证与工作流

- 修改前先阅读本文件；保持增量，不使用 reset、clean、覆盖旧成果、远程部署、重启或无授权数据迁移。
- 每个新行为先写失败测试，再实现最小通过版本；每个切片都要运行聚焦测试、类型检查和构建检查。
- 报告必须区分静态、测试运行时、Windows 实机和用户可见证据；生成文件、注册 MCP 或下载完成不能单独代表功能完成。
- 涉及公共接口、权限、数据迁移、路径策略、跨平台、新依赖或安全模型时补充 ADR，并同步 schema/contract fixtures。

## 9. 当前切片状态与后续门禁

- 当前已验证的是 Node/TypeScript 核心、PHP/Node/Python/MySQL client 只读探测与状态编排、CLI、stdio MCP、catalog/Skill 校验、Vue 面板、Tauri Windows 运行时扫描、catalog 安装命令和固定 PowerShell 终端生命周期接线；仍不是完成 Windows 11 用户验收的正式发行版。
- Rust/Tauri 环境状态和 PHP install plans 当前默认使用 `%LOCALAPPDATA%\\cxszn-menv\\state-rust.sqlite3` 的 SQLite/WAL adapter；首次打开会从旧 `state-rust.json` 和 `plans/php-install-plans.json` 迁移且保留源文件。安装 journal 仍以独立 JSONL 作为恢复源，同时镜像到 SQLite 审计表；统一 journal 事务和完整跨文件崩溃恢复完成前，不得称为最终生产持久化。
- 当前 MCP 公开变更动作仍只有 `runtime.add`、`register_path`（兼容别名）、`enable`、`disable`、`switch`；下载 artifact/install 目前只接入 Tauri GUI 的 `plan -> confirmation -> apply`，不得直接重新暴露到 MCP，除非同时补齐 core、schema、Skill、测试和 ADR。
- Tauri 组合根已通过共享 application service 接入 Windows PHP 只读扫描；Windows catalog 列表、安装计划和确认后安装命令已注册，但 GUI/CLI/MCP 的完整端到端用户可见验收仍需继续补证据，不能用静态命令注册代替验收。
- 当前 Windows 主机上的 Rust/MSVC、静态 HKCU/PATH 适配器测试、签名 catalog 校验、安全 ZIP 规则、七个 PHP catalog artifact live 安装、journal 阶段记录/受限恢复和安装包构建已验证；跨磁盘 shim 的真实 GUI 启用、Tauri 安装回滚、ConPTY 用户会话、Job Object 子进程场景和干净机验收仍必须逐项标记为未完成或未验证。

## 10. 0.0.2 Windows 原生切片状态

- 已取得当前 Windows 11 x64 主机上的 Rust workspace 编译/测试、隔离状态全量测试、HKCU PATH +
  hard-link shim roundtrip、Tauri NSIS current-user unsigned 安装/启动/卸载冒烟证据，以及新的 unsigned 包构建证据。
  最新可测试包为 `artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-confirmation-cancel-20261013-r4/`，
  安装器 SHA-256 为 `8EF0B395B0D42602B46C31928CEE483C71E6C12248319C56F36BE3A5EA2EAD99`，仍为 `NotSigned`。
  r2 已通过真实 GUI 启用/扫描保持状态；r3 已通过重启读取、catalog TS/NTS 入口和
  隔离 native CLI 备份。r4 补齐签发确认/登记载荷绑定、交互门禁和待安装计划取消，
  已通过隔离原生 EXE 拒绝测试、GUI 取消重启保持与错误弹窗收起验收。继续前读取
  `tasks/windows11-r4-acceptance-2026-10-13.md`；修改确认或取消时先读 ADR-0016/0017。
  真人控制台正向确认、真实下载失败恢复及完整 GUI/系统验收仍待完成。
  电脑工具的 App 分组可能继承启动者，
  必须用实际 EXE 进程的 MainWindowHandle 与返回窗口编号交叉确认；不操作 Codex UI。
- 核对用户真实状态时，还必须验证数据库句柄实际路径。Codex 子进程的 AppData
  可能重定向到 Codex 包缓存，不能用逻辑路径、同一 EXE 或工具启动实例代替用户
  桌面场景。先读 `tasks/windows11-activation-user-context-2026-10-13.md`；只读诊断
  不调用会初始化/恢复状态的 doctor，不自动复制/合并不同实际数据根。
- PHP 安装、手动登记、启用/停用/切换和 native CLI apply 现在使用用户级 Windows named
  mutex `Local\\cxszn-menv-runtime-operation` 做跨进程串行化；等待超时返回 `OPERATION_BUSY`。
- ConPTY 当前已有原生 child I/O、同步管道、超时/输出上限和 Job Object 进程树回收代码；
  Win11 x64 的 GUI-subsystem child-I/O smoke、输出上限、wait/shutdown、长驻
  child-tree sentinel 回收和 shutdown cancel 实测通过。console-host child-I/O
  用例因宿主输出竞态保持 ignored，完整嵌入终端用户验收仍待完成。
- catalog 当前已提交 Ed25519 公钥信任根和 PHP 官方 artifact 清单，并通过
  固定来源、大小、SHA-256 和签名验证；Rust Windows adapter 已具备固定来源下载、
  Zip 安全解压和原子目录发布代码，Tauri 已注册 catalog 列表/安装计划/确认应用命令。
  七个 PHP x64 artifact 已完成真实下载、Content-Length/SHA-256、ZIP 安全解压、原子发布和健康探测；GUI 安装回滚和用户 W/U 证据仍待完成。
- 跨磁盘 PHP 启用已改为复制受信任的 `cxszn-menv.exe` 原生 launcher shim，目标 PHP
  仍通过 `php.target` 绝对路径转发；同盘仍使用 hard-link。当前已有路径安全、跨卷判定、
  回滚和 `scripts/windows/Test-PhpCrossVolumeLauncherSmoke.ps1` 实机 shell 级测试；完整 GUI/W/U 验收仍待完成。
- Tauri installer 当前为 unsigned developer artifact；没有签名证书时不得发布为
  可信 Windows 安装版。
- 0.0.2 不能在缺少完整下载/安装回滚证据、跨卷 shim GUI/W/U、ConPTY W/U、Job Object
  证据和许可证决策时创建正式 release；当前交付物仍是 unsigned developer preview。参见
  `tasks/plan-0.0.2-windows.md` 与 `tasks/windows11-acceptance.md`。
