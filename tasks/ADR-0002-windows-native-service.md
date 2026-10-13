# ADR-0002: Windows native application service for 0.0.2

## Status

Accepted for implementation; native source and static contracts are validated,
but clean-machine Windows 11 acceptance is still pending.

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
7. Use `reqwest` with HTTPS-only/no-redirect policy (Windows Schannel via
   `native-tls`) and `zip` for the Windows installer adapter. These dependencies
   are scoped to fixed-source artifact installation because the standard library
   has no HTTP or ZIP reader; the adapter still enforces catalog-provided
   size/SHA, path, reparse, and atomic publish checks before accepting a version.

## Consequences

- Tauri no longer returns a fake empty snapshot once the Windows service is wired.
- A temporary compatibility period exists where CLI/MCP TypeScript and Tauri Rust
  are separate hosts; write actions must not be exposed by one host only.
- Windows-specific tests require a Windows runner; Linux CI remains useful for
  schema and pure logic tests but cannot prove registry, ConPTY, or installer work.
- The installer adapter is not a public URL downloader: its source validator
  accepts only `https://downloads.php.net/~windows/releases/`, and its staging
  function is not wired to a GUI write command until plan/confirm/apply and
  journal recovery evidence are complete.

## Evidence required before v0.0.2

- Rust `cargo check/test` and Tauri `tauri build` on `windows-latest`.
- Win11 x64 clean-machine install, launch, PATH lease/shim, ConPTY, rollback and
  uninstall evidence.
- Negative tests for registry concurrency, path traversal/reparse, invalid
  signatures, redirect/size/hash mismatch, pipe deadlock, timeout and output cap.
