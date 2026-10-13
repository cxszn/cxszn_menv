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
3. The Rust/Tauri Windows composition root now uses a bundled SQLite/WAL state
   adapter under `%LOCALAPPDATA%\\cxszn-menv`; it migrates the legacy JSON state
   without deleting it. The TypeScript core keeps its JSON adapter and opt-in
   Node SQLite text store for protocol compatibility, while plans and install
   journals remain separate until the unified transaction model is complete.
4. MCP exposes only local stdio and the eight read/plan/apply/status tools. A
   write requires a plan, a short-lived confirmation token supplied by a
   trusted local confirmation surface, and an idempotency key. No arbitrary
   shell, URL download, SQL, or file deletion tool is part of the contract.

## Consequences

- Core, CLI, and MCP can be tested deterministically without a Windows registry
  or a running desktop process.
- The Rust/Tauri SQLite store now provides WAL/full-synchronous snapshot and
  operation-journal transactions; the remaining JSON plan/journal files still
  do not form one cross-file transaction.
- The desktop UI is currently a mock-backed prototype; connecting it to Tauri
  must preserve the same service boundary and add Windows user-visible tests.
- Adding a new runtime family or download/install action requires updates to
  the core, schema, adapters, Skill references, tests, and a new ADR review.

## Revisit when

Before shipping a production installer, complete the plan/journal migration to
the same SQLite transaction boundary and add crash-recovery, PATH conflict,
signed catalog, archive safety, ConPTY, and Windows 11 acceptance evidence.
