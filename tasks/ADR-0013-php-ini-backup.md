# ADR-0013: Confirmation-bound php.ini backup

## Status

Accepted for the 0.0.2 developer preview.

## Context

The desktop panel could read and validate `php.ini`, but the planned safety
workflow also needs a recoverable backup before future editing support. A direct
copy button would be a write operation without the common plan/confirmation
boundary.

## Decision

- Add `BackupIni` to the shared Rust change action model.
- The panel creates a confirmation-bound plan for the selected PHP runtime.
- Apply copies the verified regular local `php.ini` into the app-owned
  `%LOCALAPPDATA%\cxszn-menv\backups\php-ini\<plan-id>.php.ini.bak` path using
  create-new semantics and a flushed file handle.
- The source file is never modified, overwritten, deleted or opened in an
  external editor by this action.
- The operation is recorded through the shared application service and remains
  disabled with respect to runtime activation.
- Native CLI `change plan --action backup-ini` and `change apply` reuse the same
  plan/token/application-service path; the CLI never gets a direct copy bypass.

## Consequences

- Users can create a durable backup before a future configuration editor is
  added, with an explicit confirmation step and an auditable operation record.
- Backup files are app-owned user data and are not removed when the external
  PHP installation is stopped or disabled.
- Editing, template generation and external-editor launch remain separate future
  operations and require their own validation and confirmation design.
