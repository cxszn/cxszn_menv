# Windows PATH lease and shim adapter

This crate is the only place allowed to touch Windows-specific environment
state. `path.rs` opens only `HKCU\\Environment\\PATH`, preserves unrelated
entries, detects an external change between read and write, verifies the value
after writing, and broadcasts `WM_SETTINGCHANGE`. It never writes HKLM,
machine PATH, PowerShell profiles, or shell scripts.

`shim.rs` keeps the fixed app-owned `bin` directory free of script launchers.
The PHP shim is an NTFS hard link to a validated absolute `php.exe` target,
with a small app-owned target record used to prove ownership before replacing
or removing it. Cross-volume targets, UNC/device paths, reparse/symlink
targets, and unowned existing destinations fail closed.

The pure merge and validation functions are covered by unit tests. Registry
and hard-link behavior still needs compilation and user-visible acceptance on
a Windows 11 x64 host with the Rust MSVC toolchain installed.
