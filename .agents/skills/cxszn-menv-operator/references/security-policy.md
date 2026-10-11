# MEnv operator security policy

This policy is the minimum boundary for GUI, CLI, MCP, and future Rust
adapters. A provider may add stricter checks but may not weaken these rules.

## Trust and catalog

- Only a catalog accepted by the configured trust root can describe a
  downloadable artifact.
- Validate key identifier, signature algorithm, signature, validity window,
  minimum application version, revocation list, artifact size, SHA-256,
  platform, architecture, and fixed source host before staging.
- Resolve source hosts through an allowlist. The catalog does not grant
  permission to follow arbitrary redirects or execute an installer script.
- A fixture, EOL, archival, revoked, or unavailable record is never a
  downloadable supported artifact. The PHP 5.6 family is explicitly legacy.
- A hash is an integrity check, not proof of publisher identity. A missing or
  invalid signature fails closed.

## Filesystem and archive

- Stage downloads outside the immutable managed-version directory.
- Reject absolute paths, UNC paths, `..` traversal, reparse-point/symlink
  escapes, duplicate entries, oversized entries, and compression bombs.
- Verify the extracted root and expected entrypoint before an atomic rename.
- Never overwrite an existing managed version. A manual path remains owned by
  the user and is never copied, edited, or deleted by MEnv.

## Activation and permissions

- Default state is disabled. Scanning, catalog listing, verification, and
  download do not activate a runtime.
- Use process-level environment injection and the app-owned shim first. A
  user-level PATH lease may contain only the fixed application shim directory.
- Never alter machine PATH, shell profiles, third-party PATH entries, or
  existing terminal sessions.
- Run ordinary operations as the user. Protected-directory or machine-level
  changes require a narrow, one-shot, typed broker and UAC; the broker accepts
  neither shell strings nor arbitrary executables.

## Process and terminal

- Spawn only an absolute, verified executable with an argv array and a
  controlled working directory.
- Apply ordinary-user permissions, a process/job boundary, timeout, output
  limits, and cancellation. Do not expose arbitrary shell execution through
  MCP or CLI.
- PHP health probes use a minimal configuration and make the selected
  `php.ini` source visible. Do not load an untrusted project configuration by
  default.

## Confirmation, concurrency, and rollback

- Every write follows `plan -> host confirmation token -> apply`.
- Bind the token to the plan hash, caller, target, and short expiry; never
  accept natural-language approval or a guessed flag as confirmation.
- Require a caller-generated idempotency key, a per-runtime lock, an operation
  identifier, cancellation support, and an operation journal.
- On failure, preserve the last-known-good state and remove only the staging
  transaction. Detect external PATH edits and refuse to overwrite them.

## Data and logs

- Do not persist secrets, confirmation tokens, full environment variables,
  complete command lines, or unredacted process output.
- Audit actor, request/operation identifiers, runtime family, version, artifact
  hash, state transition, and safe error code.
- Keep audit records local to the user's data directory and make retention a
  separate, explicit product decision.

## Prohibited operations

The public CLI/MCP surface must not expose arbitrary shell or PowerShell,
arbitrary URL fetch, SQL, machine PATH mutation, service installation or
restart, silent elevation, recursive deletion, or execution of an unverified
user-provided binary.
