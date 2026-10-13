# ADR-0009: Windows JSON state backup and replacement

## 状态

已接受，适用于 0.0.2 transitional `JsonEnvironmentStore`。

## 决策

- State writes use a synced temporary file, move the previous state to
  `state-rust.json.bak`, publish the new file, and restore the backup if publish
  fails.
- Temporary names include the process id and an atomic in-process suffix so two
  saves from one process cannot share a staging file.
- Windows state loads fall back to the backup when the primary file is missing
  or malformed.
- State schema versions other than `1` fail closed instead of being silently
  interpreted as the current model.
- The adapter remains transitional JSON persistence; this does not replace the
  planned SQLite/WAL transactional store.

## 验证

- Tests cover two consecutive saves on the same Windows path and malformed
  primary state recovery from a valid backup.
