# ADR-0001: Shared application boundary for the first MEnv slice

- Status: accepted for the Phase 0/1 vertical slice
- Date: 2026-10-11

## Context

MEnv must expose the same runtime and activation behavior to a desktop panel,
CLI, and local stdio MCP server. The repository started empty, while the Rust
MSVC toolchain is not available on the current workstation. Implementing the
protocol and state model twice would create a security and compatibility risk.

## Decision

1. `packages/core` is the executable reference application service for the
   current slice. GUI, CLI, and MCP are adapters and may not duplicate
   scanning, confirmation, activation, PATH lease, or rollback rules.
2. `domain` concepts are represented as JSON-shaped TypeScript contracts and
   mirrored in the platform-neutral Rust crates. The Rust/Tauri host remains a
   migration skeleton until it can compile on Windows with the MSVC toolchain.
3. The current default persistence adapters are JSON files under
   `%LOCALAPPDATA%\\cxszn-menv`; this is an explicit temporary adapter. An
   opt-in `NodeSqliteJsonTextStore` provides WAL/full-synchronous storage and a
   migration/meta table on Node 22.5+/24, while the adapter boundary remains
   behind `StateStore`, `PlanStore`, and `PathLeaseStore`. It is not yet the
   final typed, cross-table journal/lock implementation.
4. MCP exposes only local stdio and the eight read/plan/apply/status tools. A
   write requires a plan, a short-lived confirmation token supplied by a
   trusted local confirmation surface, and an idempotency key. No arbitrary
   shell, URL download, SQL, or file deletion tool is part of the contract.

## Consequences

- Core, CLI, and MCP can be tested deterministically without a Windows registry
  or a running desktop process.
- JSON persistence is useful for the initial contract slice but does not yet
  provide SQLite WAL, transactional journal recovery, or cross-process locks.
- The desktop UI is currently a mock-backed prototype; connecting it to Tauri
  must preserve the same service boundary and add Windows user-visible tests.
- Adding a new runtime family or download/install action requires updates to
  the core, schema, adapters, Skill references, tests, and a new ADR review.

## Revisit when

Install the Rust/MSVC toolchain and implement the Windows adapters, or before
shipping a production installer. At that point replace the JSON defaults with
SQLite migrations/WAL and add crash-recovery, PATH conflict, signed catalog,
archive safety, ConPTY, and Windows 11 acceptance evidence.
