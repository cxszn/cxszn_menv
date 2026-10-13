# ADR-0010: Core operation running/cancelled lifecycle

## 状态

已接受，适用于 TypeScript CLI/MCP core adapter。

## 决策

- `applyEnvironmentChange` persists a `running` operation before performing
  PATH/runtime work, then updates that same record to `succeeded` or `failed`.
- If a concurrent cancellation marks the operation `cancelled`, apply rolls the
  user PATH lease back and returns `OPERATION_CANCELLED` instead of overwriting
  the cancelled state with success.
- Idempotency and persisted operation lookup continue to use the same operation
  record; cancellation never rolls back an already terminal operation.

## 未解决边界

- The synchronous native Rust service still uses its own application-store
  lifecycle; full cross-host async operation unification remains a later slice.

## 验证

- Core service test uses a blocking PATH adapter to cancel a running operation,
  verify `OPERATION_CANCELLED`, and assert the lease is restored.
