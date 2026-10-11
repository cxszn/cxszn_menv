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
- 日志只记录 actor、request/operation id、runtime、版本、哈希、状态和错误码；不得记录 Token、完整环境变量、完整命令或未脱敏输出。

## 8. 验证与工作流

- 修改前先阅读本文件；保持增量，不使用 reset、clean、覆盖旧成果、远程部署、重启或无授权数据迁移。
- 每个新行为先写失败测试，再实现最小通过版本；每个切片都要运行聚焦测试、类型检查和构建检查。
- 报告必须区分静态、测试运行时、Windows 实机和用户可见证据；生成文件、注册 MCP 或下载完成不能单独代表功能完成。
- 涉及公共接口、权限、数据迁移、路径策略、跨平台、新依赖或安全模型时补充 ADR，并同步 schema/contract fixtures。

## 9. 当前切片状态与后续门禁

- 当前已验证的是 Node/TypeScript 核心、PHP 探测与状态编排、CLI、stdio MCP、catalog/Skill 校验、Vue 面板原型和 Rust/Tauri 源码骨架；不是 Windows 11 端到端发行版。
- 当前默认持久化是 JSON adapter，路径为 `%LOCALAPPDATA%\\cxszn-menv` 下的状态、计划和 lease 文件；另有 Node 22.5+/24 的 opt-in SQLite/WAL text adapter。三表事务、迁移/journal/锁/崩溃恢复完成前，不得称为生产持久化。
- 当前 MCP 公开变更动作只有 `runtime.add`、`register_path`（兼容别名）、`enable`、`disable`、`switch`；下载 artifact、安装、任意 URL 和旧的 `runtime.enable/runtime.disable` 不得重新暴露，除非同时补齐 core、schema、Skill、测试和 ADR。
- 当前桌面端仍使用 mock service；接入 Tauri 前必须保持 GUI/CLI/MCP 都调用同一个 application service，并补充 Windows 11 用户可见验收证据。
- 真实 Rust/Cargo/MSVC、Windows 注册表/HKCU PATH、shim、ConPTY、下载签名/解压安全和实机验收未验证时，交付说明必须逐项标记为未完成或未验证。

## 10. 0.0.2 Windows 原生切片状态

- 已取得当前 Windows 11 x64 主机上的 Rust workspace 编译/测试、HKCU PATH +
  hard-link shim roundtrip、Tauri NSIS current-user unsigned 安装/启动/卸载冒烟证据。
- ConPTY 当前只有结构化参数校验、同步管道、超时/输出上限和 fail-closed 代码；
  真实 child I/O/Job Object 证据仍未通过，不得称为完成。
- catalog 当前完成 Ed25519/schema/固定来源/大小/SHA-256/staging plan verifier；
  没有真实签名生产 catalog、下载器、Zip 安全解压和原子 runtime install。
- Tauri installer 当前为 unsigned developer artifact；没有签名证书时不得发布为
  可信 Windows 安装版。
- 0.0.2 不能在缺少真实 signed catalog、ConPTY W/U 证据、完整安装回滚和许可证
  决策时创建正式 release；参见 `tasks/plan-0.0.2-windows.md` 与
  `tasks/windows11-acceptance.md`。
