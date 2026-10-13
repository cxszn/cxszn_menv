# ADR-0008: Durable manual PHP registration plans

## 状态

已接受，适用于 0.0.2 Windows manual PHP path registration flow。

## 决策

- Manual registration plans persist beside install plans in
  `%LOCALAPPDATA%\cxszn-menv\plans\php-register-plans.json`.
- The persisted payload contains the verified runtime metadata, plan hash,
  snapshot hash and idempotency key; confirmation tokens are never serialized.
- Plans are filtered by expiry on load and same-key requests reuse an unexpired
  plan. Writes use temporary-file plus backup rename semantics.
- Apply still requires a fresh confirmation token and state snapshot match.

## 未解决边界

- This remains a JSON adapter until the SQLite/WAL plan/operation store is complete.
- Full GUI restart and crash-injection user acceptance is still pending.
