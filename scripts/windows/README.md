# Windows developer signing helpers

These scripts are optional local tooling for a developer preview. The normal
CI workflow intentionally produces an unsigned NSIS installer, and no private
key or certificate is checked in.

## Preview EXE and ZIP assets

Commit reviewed sources, build `pnpm --dir apps/tauri bundle:windows`, then run:

```powershell
pwsh -NoProfile -File scripts/windows/New-PreviewRelease.ps1 `
  -OutputDirectory artifacts/releases/v0.0.2
```

The output directory must not exist. The helper collects the version-matched
NSIS installer and a ZIP containing only the executable, usage guide, third-party
notices and source-commit metadata. It reopens the ZIP and checks every entry's
SHA-256. Publish the two assets plus SHA256SUMS.txt; never archive an entire local
artifacts directory or include user state. The ZIP requires an existing WebView2
Runtime and continues to use user AppData for state; it is not a portable data mode.

## PHP activation smoke

`Test-PhpActivationSmoke.ps1` is read-only. It combines the current HKCU and
machine PATH values in the script process, then checks `where php.exe`,
`php -n -v`, `php --ini`, and the app-owned `php.target` record. It does not
write PATH, the registry, or runtime files:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-PhpActivationSmoke.ps1 `
  -ExpectedVersion 8.5.11
```

The output is JSON suitable for the Windows acceptance evidence directory.

`Test-NativeCliSmoke.ps1` checks the release binary's CLI contract: doctor, PHP
catalog listing, PHP path verification, PHP registration planning, rejection of
headless confirmation, operation get/cancel routing for a missing id, and
fail-closed rejection of unsupported Node enablement. It requires PowerShell 7,
uses a unique temporary working directory and isolated `LOCALAPPDATA`, keeps
stdout and stderr separate, and restores the caller's process environment in
`finally`. It does not scan or apply a plan. The isolated state, plan and
token-free `result.json` remain under the reported `evidence_root` for review;
the caller's existing plan files are not removed:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-NativeCliSmoke.ps1 `
  -PhpPath R:\App\Env\php\php8.5.11\php.exe
```

`Test-NativeConfirmationRejectionSmoke.ps1` needs no installed PHP. It creates
an isolated runtime record and a small fake `php.ini`, then uses the supplied
real EXE to create a source-bound backup plan. Both a piped exact approval phrase
and piped EOF must return `CONFIRMATION_REQUIRED`; apply with a deliberately
unissued value must return `CONFIRMATION_INVALID`. Assertions verify unchanged
source/state/plan, no backup directory or operation, one stdout JSON envelope
per command, and unchanged HKCU PATH. It never executes the fixture's PHP path
or applies a PATH change. It retains the fixture and token-free result under a
unique evidence root and restores `LOCALAPPDATA` even on failure:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-NativeConfirmationRejectionSmoke.ps1 `
  -ExecutablePath .\target\x86_64-pc-windows-msvc\release\cxszn-menv.exe
```

These automated rejection checks do not prove successful human approval in a
real console. Use a freshly built candidate containing ADR-0016; the r3 package
predates issued-token validation. The scripts intentionally fail against a
binary that still permits headless approval or accepts an unissued value, and
do not persist raw confirmation responses.

`Test-PackagedPhpEnableSmoke.ps1` installs an explicitly supplied NSIS package
into a unique temporary directory, runs the packaged executable through the
real `runtime enable -> change confirm -> change apply` flow for a PHP path,
checks the fresh-process PATH/shim smoke, and uninstalls the temporary copy.
It does not overwrite the user's installed application directory. This script
does change the current user's PHP activation and PATH lease; run it only for
an explicitly authorized acceptance session in an interactive Windows console.
It stops before installation if input or stderr is redirected. Read the native
plan summary on stderr and manually enter `approve <full-plan-UUID>` when asked.
The native EXE checks both handles again; capture stdout only, never pipe input,
merge `2>&1`, redirect stderr, or supply an automatic approval phrase. An EOF or
any other input cancels. The confirmation value stays in script memory and is
not part of the script's result:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-PackagedPhpEnableSmoke.ps1 `
  -InstallerPath .\artifacts\cxszn_menv-windows-x64-nsis-unsigned-0.0.2-php-ini-backup-cli\cxszn_menv_0.0.2_x64-setup.exe `
  -PhpPath R:\App\Env\php\php8.5.11\php.exe
```

`Test-SqliteMigrationSmoke.ps1` verifies the packaged release's first-open
migration from an isolated legacy `state-rust.json` to `state-rust.sqlite3`,
checks that the source JSON is preserved, and removes its temporary install/data
roots:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-SqliteMigrationSmoke.ps1 `
  -InstallerPath .\artifacts\cxszn_menv-windows-x64-nsis-unsigned-0.0.2-sqlite-plan-unified\cxszn_menv_0.0.2_x64-setup.exe
```

## Installer smoke

`Test-InstallerSmoke.ps1` installs an explicitly supplied NSIS package into a
new temporary directory, runs the installed release `doctor --json`, then uses
the package's own uninstaller. It leaves the temporary directory in place if
the uninstaller fails so the failure can be inspected:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-InstallerSmoke.ps1 `
  -InstallerPath .\artifacts\cxszn_menv-windows-x64-nsis-unsigned-0.0.2-final\cxszn_menv_0.0.2_x64-setup.exe
```

`Test-InstallerUpgradeSmoke.ps1` installs a previous preview, overlays the
current preview into the same temporary directory, runs `doctor --json` after
both stages, and uninstalls the result:

```powershell
pwsh -NoProfile -File .\scripts\windows\Test-InstallerUpgradeSmoke.ps1 `
  -PreviousInstallerPath .\artifacts\cxszn_menv-windows-x64-nsis-unsigned-0.0.2-final\cxszn_menv_0.0.2_x64-setup.exe `
  -CurrentInstallerPath .\artifacts\cxszn_menv-windows-x64-nsis-unsigned-0.0.2-cross-volume-catalog-fix\cxszn_menv_0.0.2_x64-setup.exe
```

1. Run `New-MenvDevSigningCertificate.ps1` to create a short-lived, local
   CurrentUser code-signing certificate and an ignored PFX under `artifacts`.
2. Run `Sign-MenvInstaller.ps1` with that certificate thumbprint or PFX to sign
   one `.exe` installer.
3. Treat the result as locally trusted only. A self-signed certificate is not a
   public release certificate and must not be used to claim Windows SmartScreen
   or publisher trust.

The signing helper never accepts a shell command, arbitrary executable, or
remote URL. It signs only the explicitly supplied `.exe` path and reports the
Authenticode status.
