# Tauri host

`src-tauri` is the Windows desktop composition root. The Vue panel lives in
`apps/desktop` and is deliberately kept as a separate frontend package so it
can be type-checked and previewed without a Rust toolchain.

Tauri uses the same panel in development and packaging:

- dev: `pnpm --dir ../desktop dev` on port `1420`
- build: `pnpm --dir ../desktop build`
- static frontend: `../../desktop/dist`

The Rust commands are thin adapters over the shared application service. They
cannot be claimed as compiled until Rust, Cargo, the MSVC toolchain, and
WebView2 are installed on a Windows 11 host.
