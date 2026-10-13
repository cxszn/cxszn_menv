# Windows 11 0.0.2 r3 acceptance — 2026-10-13

## Scope and authority

Continue the existing PHP activation/download repair and the php.ini backup
security slice. The user separately approved opening r2, confirming activation
of `R:\App\Env\php\php8.5.11\php.exe`, and closing r2/opening r3. No installer,
upgrade, uninstall, commit, push or formal release was performed.

Existing dirty changes were preserved. No production dependency was added.

## Actual native GUI and command behavior

- r2 process 113180 owned HWND 339350; r3 process 118840 owned HWND 273756.
  Both paths were checked against the requested artifact directories. Computer
  Use grouped their windows under the launcher's `OpenAI.Codex` App ID, but the
  actual executable/MainWindowHandle matched the returned MEnv window. This
  cross-check distinguished the native app from Codex UI; Codex UI was not
  inspected or controlled.
- Before confirmation, GUI and CLI both showed external 8.5.11 disabled with
  an existing lease. Native verification confirmed it is **TS/ZTS**, not NTS.
- The user-approved r2 confirmation completed. GUI showed current activation;
  a GUI rescan kept 8.5.11 active and 8.3.35 managed/disabled. No shim appeared
  as an extra PHP runtime. CLI read the same state.
- `Test-PhpActivationSmoke.ps1 -ExpectedVersion 8.5.11` returned `ok=true`.
  The explicit new-process environment found the app shim, forwarded to the
  R-drive PHP, returned 8.5.11/ZTS, and reported no loaded php.ini. This is not
  a manual user-terminal or full ConPTY acceptance result.
- r3 was opened only after separate user approval. Its overview read 8.5.11
  active after restart. CLI read 8.5.11 active and 8.3.35 disabled.
- r2 GUI exposed a remaining catalog bug: TS 8.5.11 hid the NTS artifact as
  already registered, while native CLI reported every artifact uninstalled.
  r3 uses one native matcher for family, version, architecture, TS/NTS and
  valid availability. GUI and CLI now agree: 8.3.35 NTS installed; 8.5.11 NTS
  not installed and independently downloadable.
- The GUI lists signed-catalog entries 7.4.33, 8.0.30, 8.1.34, 8.2.34,
  8.3.35, 8.4.26 and 8.5.11. Legacy/EOL warnings are visible. This does not
  claim downloadable packages exist for all 5.6–8.5 combinations.
- PHP 8.4.26 plan confirmation opened and was cancelled with Escape. No
  confirmation/download was executed; no managed 8.4.26 directory exists and
  the most recent operation is still the approved 8.5.11 enable. Closing the
  dialog does not delete the persisted short-lived plan; it expires normally
  and cannot auto-download. The native app was left on the overview.
- A transient console obstruction and user-input guard were observed during
  cancellation. No terminal UI was automated. Fresh MEnv observation and Escape
  closed the dialog; installer/GUI console-launch behavior remains a separate
  acceptance concern.

## Backup safety and isolated native CLI

ADR-0015 records the shared typed port and conservative recovery behavior.
Generic core apply cannot record BackupIni success. Source path/SHA-256 are
bound to the token/plan hash; source limits are 4 MiB; create-new artifacts are
flushed/read back before operation persistence. Failure retains the file and
returns `BACKUP_RECOVERY_REQUIRED`, never deleting an externally replaced
filename. Legacy no-hash operations remain readable but cannot be replayed.

The fixture under
`artifacts/acceptance/windows11/2026-10-13-ini-backup-cli-isolated/` used a test
ini, legacy snapshot and process-local LOCALAPPDATA override, restored in
finally. It did not back up a real user's configuration or mutate real PATH.

- Doctor -> plan -> missing-token refusal -> confirm -> apply -> doctor passed.
- Source and backup SHA-256 both:
  `61A7BB33F4B2B3ACAEA314542A73BDA40B83EB0ED879FED5547767F8A766F050`.
- Exactly one succeeded operation; its planHash matches the approved plan.
- Fixture runtime remains disabled, fixture PATH lease remains disabled.
- Seven catalog entries; the TS fixture does not hide the NTS download.
- Confirmation material remained in process variables and was not written to
  reports/logs. Fixture and generated backup were retained as evidence.

## Verification levels

| Level | Evidence |
| --- | --- |
| Static/build | cargo fmt check, clippy all targets with warnings denied, pnpm typecheck/lint/build, 8 MCP tool contracts, Windows x64 release/NSIS build passed |
| Tests | Rust workspace 110 passed / 8 existing ignored; TypeScript 76 passed; no newly skipped tests or lowered assertions |
| Reproduction | Generic/trait fake backup success, legacy replay, same-filename replacement deletion, recovery envelope and 3 catalog variant cases failed before their fixes |
| Schema | Draft 2020-12 validator: 2 valid instances accepted, 6 invalid instances rejected, including unknown fields, missing binding, wrong family/hash and UUID traversal; Rust serde/hash binding regression also passed |
| Independent review | Backup slice: 22 focused tests independently run, WATCH/APPROVE; catalog slice: 3 focused tests independently run, no blocker. Not whole-release approval |
| Current Win11 machine | r2 GUI enable/rescan, native shim process check, r3 restart/correct catalog and confirmation-only flow, isolated release-binary backup CLI |
| Not executed here | Installer/upgrade/uninstall, real configured-ini GUI backup/restore, complete terminal user interaction, clean machine, full cross-file crash recovery |

## User data preservation

Pre/post hashes are unchanged:

- User PATH: `48791EA043A8EF9FF21E582141A37370CF1C7E5ABC9AE9544B177E8813008404`.
- Machine PATH: `17ECF12C1EAAABC92465D72319EE9F0EF0CDCD9E804BB812D969EA4E13327C71`.
- External PHP: `AB48ED7BC789D966CBC2AB85C54669FFCD5DD66259AFE94F5B07DB0C75D68A28`.
- Original installed EXE: `6E47B7D417BE3B9F493616F460E053D9FFD134F46CECC3C0282534C145CDC593`.

The explicit enable/scan updated application state and its owned shim as
authorized. The original installation, source ini, external runtime files,
machine PATH, shell profiles and external services were not overwritten.

## Artifacts and screenshots

Candidate directory:
`artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-gui-state-fix-20261013-r3/`.

- NSIS: 5,282,355 bytes; SHA-256
  `0EF12B03C251522DFBA3806921842BD2CAD68D5CE0993EB6EA2FA708336B107C`.
- EXE: 13,958,144 bytes; SHA-256
  `5A967F76FB323EE7EF3686A91F753A5002C96E52B4E143C1F2272D919C1A9AB2`.
- Both `NotSigned`, manifest `formal_release=false`. Old candidate directories
  and the R-drive installation were preserved. The installer was not run.

Screenshots are in `artifacts/acceptance/windows11/2026-10-13-r2-live-retest/`
and `2026-10-13-r3-live-retest/`: r2 active overview, r3 active after restart,
r3 independent NTS download, and the unapplied PHP 8.4 confirmation.

## Remaining release gates

Full native install/upgrade/uninstall and failure rollback, clean Win11 x64,
user terminal snapshots and ConPTY, cross-process/cross-file crash recovery,
configured ini GUI flow, and license decision remain open. User approval of
unsigned packaging removes the certificate prerequisite, not these functional
or safety gates. The goal is still active; this report is not a formal release.
