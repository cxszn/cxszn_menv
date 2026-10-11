# ADR-0002: Windows native application service for 0.0.2

## Status

Accepted for implementation; not yet validated on a Windows 11 clean machine.

## Context

The `0.0.1` Tauri shell uses `EmptyEnvironmentStore` and the TypeScript core owns
the only tested behavior. That is safe for a source preview, but it cannot mutate
HKCU PATH, host ConPTY, or install a runtime. Adding independent logic to Tauri
would create a third implementation beside CLI/MCP and make confirmation,
rollback, and audit behavior diverge.

## Decision

1. Move Windows-native mutations into the Rust `crates/application` service and
   inject `crates/platform-windows` ports from the Tauri composition root.
2. Keep `packages/core` as the compatibility reference for CLI/MCP until the
   Rust service implements the same request/envelope and has integration evidence.
3. Make HKCU PATH and shim operations typed ports. No adapter accepts shell text,
   arbitrary executables, machine PATH, profile files, or UNC targets.
4. Make ConPTY a typed session port with absolute executable, argv, cwd, bounded
   environment, synchronous I/O workers, timeout, output cap, and Job Object
   ownership.
5. Make catalog install a separate typed port. Signature and archive validation
   must finish before an immutable version directory is registered.
6. Enable Tauri current-user NSIS only after Windows CI can build the native host;
   unsigned artifacts remain explicitly unsigned.

## Consequences

- Tauri no longer returns a fake empty snapshot once the Windows service is wired.
- A temporary compatibility period exists where CLI/MCP TypeScript and Tauri Rust
  are separate hosts; write actions must not be exposed by one host only.
- Windows-specific tests require a Windows runner; Linux CI remains useful for
  schema and pure logic tests but cannot prove registry, ConPTY, or installer work.

## Evidence required before v0.0.2

- Rust `cargo check/test` and Tauri `tauri build` on `windows-latest`.
- Win11 x64 clean-machine install, launch, PATH lease/shim, ConPTY, rollback and
  uninstall evidence.
- Negative tests for registry concurrency, path traversal/reparse, invalid
  signatures, redirect/size/hash mismatch, pipe deadlock, timeout and output cap.
