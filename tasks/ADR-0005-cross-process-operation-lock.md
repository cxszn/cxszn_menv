# ADR-0005: Windows user-scoped cross-process operation lock

## 状态

已接受，适用于 0.0.2 Windows runtime install、registration 和 activation apply。

## 背景

Tauri 原先只用进程内 `Mutex` 串行化 PHP 安装。同一用户启动两个 MEnv 进程时，
它们仍可能同时写 JSON 状态、PATH lease、shim 或同一 catalog managed directory，
导致 journal、快照和 last-known-good 语义不一致。

## 决策

- 使用 Windows `Local\\cxszn-menv-runtime-operation` named mutex 作为用户会话范围的
  运行时写操作锁。
- PHP catalog 安装、手动 PHP 登记、扫描状态写回、enable/disable/switch 以及 native CLI apply
  在执行真正写入前获取该锁；锁等待超过 30 秒返回 `OPERATION_BUSY`。
- 同进程 Mutex 仍保留，用于减少同一 Tauri 进程内的重复竞争；named mutex 负责跨进程边界。
- 只处理用户级 MEnv 操作，不创建机器级对象、不提升权限、不接受调用方提供的 mutex 名称。
- 锁释放依靠 RAII；进程异常退出时由 Windows 关闭句柄并释放 mutex，后续操作可恢复。

## 未解决边界

- named mutex 不是持久化事务；JSON 状态、journal 和跨进程崩溃恢复仍需后续 SQLite/WAL
  与故障注入验收。
- 用户可见 GUI 回滚和两个独立安装包进程的完整 W/U 测试仍待补充。

## 验证

- Windows 单元测试验证持有者与竞争线程的超时、释放后重新获取。
- Tauri、platform-windows focused tests、clippy、workspace tests 和 unsigned bundle
  smoke 必须通过。
