# ADR-0006: Durable Tauri PHP install plans

## 状态

已接受，适用于 0.0.2 Windows PHP catalog GUI 流程。

## 决策

- Tauri PHP install plans persist to `%LOCALAPPDATA%\cxszn-menv\plans\php-install-plans.json`.
- The payload contains schema version, plan, fixed artifact spec and cached
  environment result only; confirmation tokens are never serialized.
- Plans are sorted by plan id, written through a temporary file and recoverable
  backup rename. A malformed primary payload can fall back to the backup; expired
  plans are discarded on load.
- The same idempotency key returns the existing durable plan instead of creating
  another download operation.
- Apply still validates the fresh confirmation token, plan hash and environment
  snapshot; persistence does not weaken the confirmation boundary.

## 未解决边界

- This is a JSON durability slice, not the final SQLite/WAL transaction model.
- Cross-process writes are serialized by the runtime named mutex, but crash
  injection and full GUI restart/rollback W/U evidence remain pending.
