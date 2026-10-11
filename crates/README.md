# Rust host contracts

The Rust workspace is a migration-safe host boundary for the TypeScript core.
`domain` contains platform-neutral state and transition types; `ports` defines
read-only discovery, process probing, persistence, and user PATH leases;
`application` orchestrates plan/confirmation/apply requests; and
`platform-windows` is the only place allowed to gain Windows-specific adapters.
`menv-core` is the composition facade consumed by Tauri, CLI, and MCP hosts.

No crate in this workspace exposes an arbitrary shell command. Discovery starts
disabled and platform adapters must preserve external PATH entries and manual
runtime ownership.

The current machine has no `rustc`, `cargo`, or `rustfmt`, so this workspace has
only static review evidence. Compile and test evidence must be collected on a
Windows 11 host after the Rust MSVC toolchain and WebView2 are installed.
