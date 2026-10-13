# ADR-0007: Native operation query and cancellation semantics

## 状态

已接受，适用于 0.0.2 native CLI/application service operation contract。

## 决策

- `operation get --id <operation-id>` returns the persisted operation record
  without mutating runtime state.
- `operation cancel --id <operation-id>` may transition only `queued` or
  `running` records to `cancelled`; it uses the same user-scoped runtime mutex.
- `succeeded`, `failed`, and already `cancelled` operations return
  `OPERATION_NOT_CANCELLABLE`; missing ids return `OPERATION_NOT_FOUND`.
- Cancellation is a state transition boundary. Synchronous download/apply code
  must still check the operation state before starting destructive work; it does
  not pretend to kill a process that has already completed.

## 验证

- Application tests cover queued cancellation, status lookup, and completed
  operation refusal.
- Native CLI uses the same JSON envelope and stable error codes as Tauri.
