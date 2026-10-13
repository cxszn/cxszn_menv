# ADR-0015: Source-bound php.ini backup effects and conservative recovery

## Status

Accepted for the 0.0.2 developer preview. Extends ADR-0013; not a release approval.

## Context

A backup must not be reported as successful unless a flushed, verified artifact
exists. The ordinary application `apply_change` cannot do this without a file
port. Deleting a failed backup by its filename after closing the handle can
delete a replacement file written by another process.

## Decision

- `Orchestrator::apply_change` rejects `BackupIni`, including completed retries.
  The dedicated application backup method takes a typed `PhpIniBackupStore`
  port and owns preflight, source hashing, artifact creation/verification and
  operation persistence. GUI and native CLI only supply the Windows adapter.
- `backupSource.path` and `backupSource.sha256` are part of the canonical plan
  hash. The confirmation dialog displays both. Before any write, the source
  must still match that digest. A backup plan without this binding is refused.
- All mutating plans require a bound token regardless of the client-supplied
  `requiresConfirmation` display field. Unknown fields are refused.
- PHP ini input is a regular local file, limited to 4 MiB. Existing reparse,
  symlink and local-path checks remain mandatory. The backup identifier is a
  UUID, never a caller-provided relative path or filename.
- Create-new semantics prevent overwriting an existing backup. The artifact is
  flushed and read back before a succeeded operation is persisted.
- Errors after a file is created retain it and return
  `BACKUP_RECOVERY_REQUIRED`, `retryable=false`. No filename-based deletion or
  blind overwrite is attempted. A retained file is not a succeeded operation.
  The user can inspect it and create a new plan; automatic orphan adoption or
  deletion is not implemented in this slice.
- Successful retries reverify the original artifact and return the original
  source/path, even if the runtime's current ini path has changed. A reused key
  must match both plan ID and `planHash`; legacy records without a hash remain
  readable but are not valid replay targets.

## Compatibility

- `backupSource` is omitted on ordinary plans, preserving their JSON/hash
  shape. Old unbound backup plans must be regenerated.
- `OperationRecord.planHash` is optional for reading old snapshots; new native
  change operations store the approved hash. No source snapshot is deleted.
- MCP actions are unchanged. Native backup support is not silently added to
  the TypeScript CLI or MCP contracts.
- The native GUI backup result retains `runtimeId`, `sourcePath`, `backupPath`.
  The recovery error adds an actionable code without exposing confirmation
  material or ini contents.

## Alternatives and consequences

Keeping a Windows handle and performing descriptor-identity-safe cleanup is a
possible later design. It introduces an additional Windows-specific lifetime
and recovery protocol; path/hash checks alone cannot provide that guarantee.
Retaining the file avoids destructive cleanup now, at the cost of possible
orphaned files after a write/persistence failure.

File creation and SQLite persistence are not one transaction. Process crashes
can still leave an unrecorded artifact. Full crash recovery, hardened directory
handle traversal, real configured-ini GUI acceptance and the other Windows
release gates remain separate work. This ADR must not be cited as proof of
complete rollback or formal-release readiness.
