# Windows PATH lease and shim adapter

This crate is the only place allowed to touch Windows-specific environment
state. `path.rs` opens only `HKCU\\Environment\\PATH`, preserves unrelated
entries, detects an external change between read and write, verifies the value
after writing, and broadcasts `WM_SETTINGCHANGE`. It never writes HKLM,
machine PATH, PowerShell profiles, or shell scripts.

`shim.rs` keeps the fixed app-owned `bin` directory free of script launchers.
On the same volume the PHP shim is an NTFS hard link to a validated absolute
`php.exe` target. For a cross-volume target it copies the trusted
`cxszn-menv.exe` launcher and stores the absolute target in an app-owned
`php.target` record; the launcher forwards arguments without copying PHP or
changing the caller's working directory. UNC/device paths, reparse/symlink
targets, and unowned existing destinations fail closed. Activation stages the
new shim and record before replacing the previous app-owned pair and restores
that pair if publication fails.

The pure merge and validation functions are covered by unit tests. Registry
and hard-link behavior still needs compilation and user-visible acceptance on
a Windows 11 x64 host with the Rust MSVC toolchain installed.

`conpty.rs` follows the Windows pseudoconsole pipe contract: the ConPTY-facing
pipe ends are inheritable, while the MEnv reader/writer ends are explicitly
non-inheritable. The GUI-subsystem smoke test captures child output successfully;
running the same live test from an outer console/pseudoconsole can still mirror
`cmd.exe` output to that host, so the console-host case remains an acceptance
limitation rather than a release claim.

For read-only runtime discovery evidence, build and run:

```powershell
cargo run -p cxszn-menv-platform-windows --example runtime_discovery_smoke
```

It prints JSON for PHP, Node.js, Python, and MySQL/MariaDB client providers and
does not persist state or modify PATH/services. For explicit-path evidence,
optionally set `MENV_TEST_PHP_PATH`, `MENV_TEST_NODE_PATH`,
`MENV_TEST_PYTHON_PATH`, or `MENV_TEST_MYSQL_PATH`; those paths are verified
through the same provider but remain unregistered.
