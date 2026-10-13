# ADR-0012: Use SQLite WAL for the Rust Windows state store

## Status

Accepted for the 0.0.2 Windows developer preview.

## Context

The Rust/Tauri composition root still used an atomic JSON file for the
environment snapshot while install plans and journals were stored separately.
That protected individual files, but it could not commit the snapshot and
operation journal as one durable transaction after a crash.

## Decision

- Use `rusqlite` with the `bundled` feature in the Windows adapter. The bundled
  SQLite library avoids depending on a system DLL and keeps the installer
  self-contained.
- Open the database with WAL mode, `synchronous=FULL`, and a bounded busy
  timeout for every operation. The store owns no long-lived connection, so the
  existing application service remains `Send + Sync`.
- Keep an `environment_state` singleton row containing the versioned snapshot
  payload and an `operation_journal` table containing the snapshot's operation
  records. Save replaces both inside one `BEGIN IMMEDIATE` transaction.
- On first open, migrate the existing `state-rust.json` through the same store;
  leave the legacy JSON file intact as a recoverable migration source.
- Reject unsupported schema versions and malformed payloads fail-closed. Do
  not fall back from a corrupt SQLite database to a potentially stale JSON file.
- Move PHP install-plan rows into the same SQLite file and migrate the legacy
  `plans/php-install-plans.json` on first access without deleting that source.
  Install journals remain JSONL because their append-only recovery semantics are
  intentionally independent in this slice; each production journal line is also
  mirrored into `menv_install_journal` for structured audit queries.

## Alternatives considered

- **Keep JSON:** no dependency cost, but no cross-record transaction or WAL
  crash semantics; rejected for the formal persistence boundary.
- **Use a system SQLite DLL:** smaller binary, but deployment depends on the
  user's machine and can load an incompatible or untrusted DLL; rejected.
- **Add a database ORM:** unnecessary abstraction for two small tables and adds
  more dependencies; rejected in favor of parameterized `rusqlite` statements.

## Consequences

- The Windows bundle grows because SQLite is statically bundled.
- Existing users migrate on first state access without deleting their JSON
  state; rollback can remove the new database and retain the source file.
- A unified install-journal transaction, normalized runtime rows and final
  operation schema remain future work; this decision does not claim those
  boundaries are done.

## Verification

- Legacy JSON migration preserves the source file and round-trips state/WAL.
- Unsupported SQLite schema versions fail closed without falling back to the
  legacy JSON source.
- PHP plan rows and install-journal mirror rows round-trip transactionally.
