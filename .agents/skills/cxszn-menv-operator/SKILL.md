# cxszn-menv-operator

Use this skill when an AI agent needs to inspect or request a change to the
MEnv development-runtime manager. The skill is deliberately a safe-flow
description; it is not an installer, a shell wrapper, or a source of runtime
credentials.

## Scope

- MEnv currently targets Windows 11 x64 and PHP 5.6–8.5.
- Other runtime families are exposed only when their provider is implemented
  and the catalog says that the artifact is available.
- The local MCP server uses stdio. The CLI and MCP adapters call the same core
  application service and must return the same envelope and state model.

## Required call order

1. Call `menv_get_environment_state` or `menv_scan_installed_runtimes` before
   making assumptions about installed runtimes, ownership, or active state.
2. For a path supplied by a user, call `menv_verify_runtime_path`. Treat the
   response as read-only evidence; never copy, overwrite, or delete that path.
3. For registration, activation, deactivation, or PATH change, call
   `menv_plan_environment_change` first. (Catalog installation is not exposed
   until its signed downloader is implemented.) Present the plan summary,
   affected runtime, scope, and rollback information to the user.
4. Continue only after the user interface returns a short-lived confirmation
   token bound to that exact plan. A natural-language approval, a guessed
   token, or a `confirm=true` argument is not sufficient.
5. Call `menv_apply_environment_change` with the plan identifier, the
   confirmation token, and a caller-generated `idempotency_key`. Reuse the
   same key when retrying an interrupted request.
6. Poll `menv_get_operation_status` until the operation reaches a terminal
   state, or call `menv_cancel_operation` when the user asks to cancel and the
   operation is cancellable.
7. Re-read environment state after a successful operation. Report the
   resulting availability, activation, health, and operation states
   separately; installation is not activation and activation is not proof that
   an existing terminal changed.

## Read-only operations

These operations do not modify the machine and may be used without a
confirmation token:

- `menv_get_environment_state`
- `menv_scan_installed_runtimes`
- `menv_list_runtime_catalog`
- `menv_verify_runtime_path`
- `menv_get_operation_status`

Catalog entries marked `fixture`, `archival`, `eol`, `revoked`, or
`unavailable` must not be presented as downloadable supported artifacts.

## Write-operation rules

- Never expose or request an arbitrary shell, PowerShell script, SQL statement,
  URL, installer script, or file-deletion tool through MCP.
- Never infer a path, version, architecture, host, or scope that changes the
  user's requested target. Ask the user through the host UI when a required
  value is missing.
- Keep user-level app-owned PATH leases separate from machine PATH and third-
  party entries. Existing terminals do not hot-switch.
- Do not activate a version as a side effect of scanning, catalog listing,
  path verification, or downloading.
- Do not claim success from an operation acknowledgement alone. Confirm the
  terminal operation state and then perform a read-back.
- Do not include secrets, full environment variables, raw command output,
  private paths, or confirmation tokens in the response.

## Error handling

Treat `CONFIRMATION_REQUIRED`, `STALE_PLAN`, `IDEMPOTENCY_CONFLICT`,
`RESOURCE_BUSY`, `CATALOG_REJECTED`, `PATH_CONFLICT`, and `UNAVAILABLE` as
actionable states, not as permission to retry with broader privileges. Use the
`retryable` flag and `next_actions` from the response. Never bypass a failed
signature, expiry, revocation, path, archive, or hash check.

The full wire contract, CLI mapping, and security rules are in:

- `references/mcp-tool-contract.md`
- `references/cli-contract.md`
- `references/security-policy.md`

Run the repository's contract validator after changing this skill or its
references. The validator checks the fixture, the schema's required safety
markers, and the absence of machine-specific secrets or absolute paths; it
does not grant permission to perform an operation.
