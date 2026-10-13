# ADR-0016: Issued native confirmation tokens and interactive CLI approval

## Status

Accepted for the next 0.0.2 test candidate, following the user's 2026-10-13
choice to retain `change confirm` with explicit interactive approval. Extends
ADR-0004 and ADR-0015; it is not formal-release approval.

## Context

Native validators checked a token's nonempty value and plan metadata, but did
not verify issuance. A never-issued string with matching fields could therefore
authorize core changes, backup, manual registration or installation. The old
CLI confirm command also minted a token in a noninteractive pipeline.

The TypeScript core already stores and verifies confirmation-token digests;
the Rust migration must provide the same security property. CLI and GUI share
native state, so approvals must survive the separate confirm/apply processes.

## Decision

- The shared Rust application service issues cryptographically random UUID
  nonce values after the host's explicit approval, and validates both metadata
  and an actual issued record. A `ConfirmationStore` port stores only SHA-256,
  plan ID/hash, actor and expiry. It never stores raw token material.
- Windows injects the same SQLite/WAL adapter used for environment state. Its
  additive `menv_confirmations` table uses a unique digest primary key and
  parameterized inserts/lookups. A duplicate digest cannot replace a binding.
  Non-Windows/isolated composition uses instance-local memory records; it does
  not pretend to provide cross-process durability.
- New records expire within the existing five-minute plan lifetime. Validation
  is performed even for cached successful results. A nonce cannot be rebound
  to another plan hash, actor or expiry. Retries of the same approved plan may
  reuse it while valid, under the existing operation mutex/idempotency checks.
- GUI enable/backup, manual registration and catalog installation all validate
  through this shared authority. Artifact-bound install-plan hash checks stay
  in the existing adapter; the issuer binds that approved digest rather than
  incorrectly substituting the generic core hash.
- The displayed plan must match its canonical digest before native CLI approval
  is requested. Generic application issuance also rejects a mismatched digest.
  GUI issuance validates install requests against the current SQLite plan/spec
  under the operation lock and refuses cancelled plans; registration issuance
  checks the stored plan/runtime record. A caller-supplied digest alone is not
  an authority for either artifact-bound action.
- Registration uses SHA-256 of `{canonicalPlanHash, runtime}` with the complete
  verified runtime record. GUI and CLI creation, approval and application use
  this same binding, so changing a saved executable path or runtime metadata
  cannot borrow a previously issued approval. The JSON plan shape is unchanged;
  legacy unbound registration plans are preserved but must be regenerated.
- Registration and installation recheck issued-token validity after obtaining
  their operation locks and before effects. Approval that expires while waiting
  does not authorize a delayed write.
- `change confirm --json` requires genuine Windows console handles for stdin
  and stderr. It displays the action, runtime identifiers, plan hash, source
  binding and expiry on stderr. Only `approve <full-plan-UUID>` permits issuance.
  Raw input is capped at 256 bytes before trimming. EOF, other inputs, pipes,
  file redirection of input/prompt and unavailable handles fail closed.
- Stdout still contains exactly one JSON envelope; prompts go to stderr.
  There is no `--yes`, `confirm=true`, environment override or headless bypass.
- Windows checks `GetStdHandle` + `GetConsoleMode`, rather than accepting
  named-pipe PTY-name heuristics. It does not allocate/attach a new console,
  replace redirected handles, close borrowed standard handles or alter modes.
- Token debug formatting redacts the opaque value. Legacy, unrecorded tokens
  are refused; old plan/snapshot files are preserved and require new approval.

## Sources

- Windows crate 0.61.3 generated API signatures, verified in the installed
  package; Rust 1.99.0 bundled `std::io::IsTerminal` documentation notes the
  MSYS/Cygwin named-PTY heuristics.
- Microsoft Learn, viewed in the user's external Chrome:
  https://learn.microsoft.com/en-us/windows/console/getconsolemode#parameters
  and https://learn.microsoft.com/en-us/windows/console/getstdhandle#remarks.
  These document console-buffer access, redirected/missing handles and
  process-owned handle lifetime.

## Compatibility and trust limits

Token JSON fields and application envelope remain unchanged. CLI confirm's
interactive requirement is an intentional security tightening: unattended
clients may plan/read/apply an already approved token, but cannot auto-confirm
through an ordinary pipeline. Automated tests use isolated authorities and
test data, not a production override.

A console proves an I/O capability, not that a human supplied the keystrokes.
Software controlling a real console, or a same-user process able to alter the
database/binary, remains outside this local protocol's protection. The UI/CLI
trust boundary and AI permission policy must not be described as cryptographic
proof of human identity. No new account authentication or remote authority is
introduced.

New table creation is additive and does not change the environment snapshot
schema. Full cross-file install crash recovery, live interactive CLI acceptance,
pending-plan cancellation, clean-machine and installer lifecycle gates remain
independent work. r3 does not contain this change and must not be presented as
having the issued-token protection.
