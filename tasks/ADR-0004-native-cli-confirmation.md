# ADR-0004: Native CLI plan/confirm/apply boundary

## 状态

已接受，适用于 Windows 0.0.2 preview。

## 背景

安装后的 `cxszn-menv.exe` 既是 Tauri GUI 入口，也是用户可能直接调用的命令行
入口。CLI 必须复用 Rust application service，不能重新实现 PATH、shim、扫描或
回滚规则，也不能把 `--yes`、任意 shell 或模型文本当作确认。

## 决策

- 只读命令可以直接执行：`doctor`、`runtimes list`、`runtime scan`、
  `runtime verify`、`catalog list --kind php`。
- PHP enable/disable/switch 和 PHP manual registration 使用：
  `plan -> change confirm -> change apply`。
- plan 存在 `%LOCALAPPDATA%\\cxszn-menv\\cli-change-plan.json`；manual registration
  使用同根下独立的 `cli-register-plan.json`，避免两个 plan 类型互相覆盖。
- plan 文件只保存 ChangePlan 和已验证的 runtime metadata，不保存 confirmation token。
- confirmation token 由显式 `change confirm` 命令生成，短期绑定 plan id、plan hash、
  actor 和 expiry；apply 必须再次提供 token 和匹配的 idempotency key。
- apply 成功后删除对应 plan 文件；失败时保留 plan 供用户重新处理或让其过期。
- Node、Python、MySQL/MariaDB 当前只有只读 RuntimeProvider；CLI 对这些 family 的
  enable/disable 直接返回 `UNSUPPORTED_ACTION`，不生成无效 PHP shim 计划。
- CLI 输出使用稳定 `schema_version/request_id/ok/result/error` envelope；doctor、list
  和 scan 的 PATH entries 脱敏为 `<redacted>`，保留 lease/hash 元数据。

## 安全与回滚

- CLI 不开放任意 shell、PowerShell、URL 下载、SQL、文件删除或 `--yes` 绕过。
- PATH/shim 和安装回滚仍只由共享 application service/Windows adapter 执行。
- plan 文件是本机用户数据，不是跨进程数据库；跨进程计划锁和完整 SQLite 事务仍是
  后续持久化切片。

## 验证

- release binary 已实测只读 CLI、PHP runtime verify、PHP runtime add plan/confirm 和
  unsupported Node enablement fail-closed。
- Rust workspace、clippy、Desktop tests/typecheck/build 和 installer smoke 通过。
