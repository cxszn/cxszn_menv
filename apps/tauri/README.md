# Tauri host

`src-tauri` is the Windows desktop composition root. The Vue panel lives in
`apps/desktop` and is deliberately kept as a separate frontend package so it
can be type-checked and previewed without a Rust toolchain.

Tauri uses the same panel in development and packaging:

- dev: `pnpm --dir ../desktop dev` on port `1420`
- build: `pnpm --dir ../desktop build`
- static frontend: `../../desktop/dist`

On a Windows host with Rust/MSVC installed, build the installer with
`pnpm --dir apps/tauri bundle:windows` from the repository root.

## Windows installer

The Windows bundle is an NSIS installer configured for `currentUser` mode. It
installs under the user's local application data directory and does not request
machine-wide elevation. The bundle target is intentionally limited to `nsis`
until the Windows acceptance gate covers other targets.

The configuration reads its version from `apps/tauri/package.json`, so the
package version and installer version cannot silently diverge. The checked-in
icon source is `src-tauri/icons/menv.svg`; generated PNG/ICO variants are
required inputs to the bundler.

CI currently produces an explicitly **unsigned** NSIS artifact. No certificate,
private key, or signing command is stored in this repository. A signed release
must add a protected certificate workflow and a separate release approval gate;
the unsigned CI artifact must not be presented as a trusted Windows release.

The Rust commands are thin adapters over the shared application service. They
cannot be claimed as compiled until Rust, Cargo, the MSVC toolchain, and
WebView2 are installed on a Windows 11 host.
