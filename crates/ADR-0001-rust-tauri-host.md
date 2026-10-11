# ADR-0001: Rust/Tauri host boundary

- Status: accepted for the migration skeleton
- Scope: `crates/` and `apps/tauri/src-tauri`

## Decision

Use a Cargo workspace with platform-neutral `domain`, `ports`, and
`application` crates, a Windows adapter crate, and a small `menv-core` facade.
Tauri commands are thin composition adapters and call the same application
service used by future CLI/MCP hosts. The Tauri capability file grants only
`core:default`; process, shell, filesystem, and URL permissions are not
implicitly enabled.

## Consequences

- Runtime state and transition contracts can be shared without leaking Windows
  registry or Tauri types into domain code.
- Windows discovery and PATH/ConPTY implementations remain isolated and can be
  tested on a real Windows 11 host.
- The initial Rust provider deliberately returns `UnsupportedPlatform` until
  those adapters have integration tests; it must not claim discovery works.
- Cargo and Tauri builds are pending because this development host does not have
  the Rust MSVC toolchain or WebView2.

## Revisit when

Replace the placeholder Windows provider only after adding tests for path
concurrency, reparse-point rejection, process bounds, and rollback journal
recovery. Any new privileged Tauri command requires a corresponding application
service method and capability review.
