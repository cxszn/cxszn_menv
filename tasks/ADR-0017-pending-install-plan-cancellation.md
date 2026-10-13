# ADR-0017: 持久化 PHP 待安装计划取消

## 状态

已接受，用于下一个 0.0.2 developer preview；补充 ADR-0007 和 ADR-0011。

## 决策

- `cancel_php_install { planId }` 只取消从未开始执行的安装计划。成功使用现有
  `schema_version / request_id / ok / result` envelope，`result` 为 `null`。
- 计划内部增加兼容字段 `status`：`pending / started / cancelled`。旧记录缺少该字段
  时按 `pending` 读取；已有 `result` 的旧计划仍视为完成，不能取消。
- 取消保留 SQLite 原记录作为 tombstone，重复取消成功。恢复列表排除取消记录；
  持有旧 plan/token 的调用方 apply 返回 `PLAN_CANCELLED`。相同 idempotency key
  不能创建新计划来恢复已取消操作；主动重建需要新 key。
- SQLite 记录包括过期和终态记录都保留，避免删完表后兼容迁移重新导入旧 JSON
  并复活计划。此切片不执行历史清理，原 JSON 文件继续保留。
- 创建、取消、执行和兼容迁移使用现有用户级 operation mutex。每次计划写入和
  apply 都在锁内重新读取 SQLite 权威记录，不依赖进程启动时的内存缓存。
- apply 在 operation mutex 和 install I/O lock 都取得之后，重新验证计划、取消
  状态、已签发 token、过期时间和快照；在任何下载开始之前持久化 `started`。
  持久化失败则不下载。
- `started`（包括一次失败但可能留下恢复状态的安装）和完成计划不能取消，返回
  `OPERATION_NOT_CANCELLABLE`；找不到计划返回 `PLAN_REQUIRED`。并发安装仍在持锁
  时，取消可能先返回 `OPERATION_BUSY`，不会声称已中断下载。
- `started` 不阻止原计划通过有效确认重试，仍保留既有 managed-directory
  reconciliation 逻辑。它不承诺延长五分钟确认期，也不扩展已有崩溃恢复保证。
- GUI 等待后端取消成功后才关闭确认页；失败保留计划并在弹窗显示错误，允许重试。
  取消等待时禁用确认和取消按钮，避免前端竞态。
- 错误状态另提供“关闭（保留计划）”，只收起弹窗，不调用取消或删除记录；进行中
  不可操作。它不表示安装已取消，避免已开始但失败的计划把用户困在弹窗内。

## 边界与验证

这次变更不实现下载/解压中的协作式中断，也不把 artifact install 暴露到 MCP。
ADR-0007 的 operation cancel 仍是已有 operation 记录的状态变更。

隔离 SQLite 测试覆盖：持久化和重复取消、重启读取过滤、旧进程计划拒绝 apply、
取消 key 拒绝复用、开始和完成拒绝取消、锁后审批过期检查、旧字段兼容、started
重试，以及错误 envelope/状态 fixture。前端服务测试覆盖取消失败保留本地计划。
实际 Windows GUI 取消与重启验收另行记录，单元测试不代替该证据。
