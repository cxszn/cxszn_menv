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

The installed executable also exposes a narrow JSON CLI without starting the
GUI:

```powershell
cxszn-menv.exe doctor --json
cxszn-menv.exe runtimes list --json
cxszn-menv.exe runtime scan --json
cxszn-menv.exe runtime verify --family php --path R:\\App\\Env\\php\\php8.5.11\\php.exe --json
cxszn-menv.exe runtime add --family php --path R:\\App\\Env\\php\\php8.5.11\\php.exe --json
cxszn-menv.exe catalog list --kind php --json
cxszn-menv.exe change plan --action enable --family php --runtime-id <id> --json
cxszn-menv.exe change plan --action backup-ini --family php --runtime-id <id> --json
cxszn-menv.exe change confirm --plan-id <id> --json
cxszn-menv.exe change apply --plan-id <id> --confirmation-token <token> --idempotency-key <key> --json
cxszn-menv.exe operation get --id <operation-id> --json
cxszn-menv.exe operation cancel --id <operation-id> --json
```

Write operations remain behind the shared confirmation-bound application
service and are not exposed as arbitrary shell commands or `--yes` bypasses.
`change confirm --json` now requires Windows console input and a visible console
stderr prompt. Inspect the displayed plan and enter `approve <full-plan-UUID>`.
Other input, EOF, pipes or file-redirection of input/prompt refuse issuance;
stdout remains a single JSON envelope. The nonce must actually have been issued
by the shared service: matching fields or an arbitrary nonempty string are not
sufficient. Windows stores only its SHA-256 and bound metadata in SQLite so
separate confirm/apply processes can validate it. Token debug formatting is
redacted. Old unrecorded tokens need fresh approval, without deleting old plans.
This console check is not proof of human identity or protection against software
controlling a real terminal. See `tasks/ADR-0016-native-issued-confirmation.md`.
Manual registration approval displays the full verified runtime, and its digest
binds that payload. Old unbound registration plans must be generated again.

The desktop can cancel a PHP install plan before installation starts. Cancellation
is durable and idempotent; stale callers cannot apply a cancelled plan. Started
plans return `OPERATION_NOT_CANCELLABLE`. On an error, “关闭（保留计划）” only
dismisses the dialog and preserves the plan. This does not interrupt an active
download. See `tasks/ADR-0017-pending-install-plan-cancellation.md`.

CLI environment snapshots redact individual PATH entries while retaining lease
state and hashes; the GUI/internal service keeps the full path list for PATH
operations.

Native PHP ini backup plans carry `backupSource.path` and `backupSource.sha256`
in the approved plan hash. Input is limited to 4 MiB. Generic core apply rejects
this action; the dedicated shared application method uses a typed backup store,
flushes and verifies a new artifact before recording success. It never edits
the source. A failure after creation returns `BACKUP_RECOVERY_REQUIRED` with a
retained path and `retryable=false`; inspect that artifact and create a new
plan instead of deleting or overwriting it automatically. Completed retries
verify the original artifact. Older unbound backup plans must be regenerated;
old operations without `planHash` remain readable but cannot be replayed.
See `tasks/ADR-0015-ini-backup-effect-and-recovery.md` and
`crates/application/schema/native-change-plan.schema.json`. This native action
is not automatically available in the TypeScript CLI or MCP tool contract.

## Windows installer

The Windows bundle is an NSIS installer configured for `currentUser` mode. It
installs under the user's local application data directory and does not request
machine-wide elevation. The default folder name is `cxszn_menv`; a user can
still choose another local installation directory such as `R:\App\Code_Tool\cxszn_menv`.
The bundle target is intentionally limited to `nsis`
until the Windows acceptance gate covers other targets.

The configuration reads its version from `apps/tauri/package.json`, so the
package version and installer version cannot silently diverge. The checked-in
icon source is `src-tauri/icons/menv.svg`; generated PNG/ICO variants are
required inputs to the bundler.

CI currently produces an explicitly **unsigned** NSIS artifact. No certificate,
private key, or signing command is stored in this repository. A signed release
must add a protected certificate workflow and a separate release approval gate;
the unsigned CI artifact must not be presented as a trusted Windows release.

The Vue panel uses the Rust application service when running inside Tauri. It
scans local PHP, Node.js (including nvm-windows version directories), Python,
and MySQL/MariaDB client paths, and after the confirmation flow enables the
app-owned HKCU shim/PATH lease for PHP only. Existing terminals must be
reopened after a change; the current process environment is intentionally not
hot-swapped. Node, Python, and MySQL discovery are read-only in this slice and
do not take over external managers or services.

Unexpired PHP download plans are restored from the user-scoped plan file after
desktop restart. Restoration only repopulates the confirmation dialog; the
download remains blocked until a new user confirmation token is issued.
