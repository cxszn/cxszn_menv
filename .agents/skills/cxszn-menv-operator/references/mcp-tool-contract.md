# MEnv MCP contract (v1)

## Transport and lifecycle

- Transport is local stdio only. The server writes JSON-RPC responses to
  stdout and diagnostics to stderr; stdout must never contain a log line.
- Clients must complete MCP `initialize` negotiation before calling a tool.
- The implementation advertises tools through `tools/list` and rejects unknown
  tool names, unknown input fields, and invalid enum values.
- This contract is transport-neutral at the core: the same application service
  is used by the GUI and CLI.

## Response envelope

Every tool result has a structured response with this logical shape:

```json
{
  "schema_version": "1",
  "request_id": "request-generated-id",
  "ok": true,
  "result": {},
  "operation_id": null
}
```

Errors use the same envelope and replace `result` with `error`:

```json
{
  "schema_version": "1",
  "request_id": "request-generated-id",
  "ok": false,
  "error": {
    "code": "CONFIRMATION_REQUIRED",
    "message": "A user-confirmed plan is required.",
    "retryable": false,
    "next_actions": ["create_or_show_plan"]
  }
}
```

`request_id` is an opaque correlation value. `operation_id` is returned for a
long-running write and must be used with the operation-status tool.

## Tools

### `menv_get_environment_state`

Read the persisted state and current user-level app lease. Input is an object
with no required fields. The result includes `runtimes`, `active`, `path_lease`,
and `updated_at`. Each runtime separates `availability`, `activation`,
`health`, and `operation`.

### `menv_scan_installed_runtimes`

Perform a read-only scan of PATH and approved discovery locations. Input:

```json
{ "runtime_family": "php" }
```

`runtime_family` is optional; when present it is one of `php`, `node`,
`mysql`, or `python`. Scanning never registers, installs, enables, or deletes.

### `menv_list_runtime_catalog`

List catalog metadata loaded by the core. Production signed-catalog
verification and the PHP installer are implemented by the Windows desktop host;
MCP remains metadata-only and does not expose arbitrary download/install tools.
Fixture entries are visible for tests only and are not downloadable. Input may contain
`runtime_family` and `include_archival` (boolean). The result explicitly marks
legacy/EOL/archival entries and never returns a download URL.

### `menv_verify_runtime_path`

Read-only verification of a user-provided executable or directory. Input:

```json
{ "runtime_family": "php", "path": "<user-selected-path>" }
```

The implementation canonicalizes and validates the path without modifying it.
It returns detected version, architecture, thread-safety (when applicable),
health checks, and a reason when the path is not usable.

### `menv_plan_environment_change`

Create an immutable plan; this does not download, install, enable, disable, or
write PATH. The current TypeScript vertical slice supports registration of an
already-present PHP executable and activation changes. Download/install from a
signed catalog is a later phase and is deliberately not exposed as an action
until its verifier and installer are implemented. Input is strict and contains
only the requested action:

```json
{
  "action": "runtime.add",
  "path": "<user-selected-php-executable>",
  "scope": "user"
}
```

`action` is one of `runtime.add`, `register_path` (deprecated alias for
`runtime.add`), `enable`, `disable`, or `switch`. `path` is required for
`runtime.add`/`register_path`; `runtime_id` is required for enable/disable and
the current target is required for switch. `install` and `artifact_id` are not
accepted by the MCP adapter; catalog installation remains a Tauri desktop host
operation with a signed plan and confirmation dialog. `scope` is currently
`user` for PATH changes.
The result includes `plan_id`, `plan_hash`, `expires_at`, `path_before`,
`path_after`, and `requires_confirmation: true`.

### `menv_apply_environment_change`

Apply exactly one unexpired plan after the host UI has obtained its
confirmation token. Input:

```json
{
  "plan_id": "<plan-id>",
  "confirmation_token": "<host-issued-token>",
  "idempotency_key": "<caller-generated-stable-key>"
}
```

The host UI or another trusted local confirmation surface issues the short-lived
token; the MCP adapter does not provide a `confirm=true` shortcut or mint a
token from model text. The token is bound to the plan hash, caller, target, and expiry. Reusing an
idempotency key for a different plan is rejected with `IDEMPOTENCY_CONFLICT`.
The result returns `operation_id` and an operation state. The current local
slice may complete a small change synchronously; callers must still read the
operation and environment state back.

### `menv_get_operation_status`

Read an operation by `operation_id`. Terminal states are `succeeded`,
`failed`, and `cancelled`; non-terminal states are `queued` and `running`.
Results contain safe error codes and summarized effects, not
tokens or raw process output.

### `menv_cancel_operation`

Request cancellation of a cancellable operation. Input is
`{ "operation_id": "<operation-id>" }`; callers may also provide an
idempotency key when their host retries cancellation. Cancellation does not
roll back a completed step automatically; the status response states whether
the journal restored the last-known-good state.

## Error codes

Stable codes include `INVALID_PARAMS`, `UNKNOWN_TOOL`,
`CONFIRMATION_REQUIRED`, `PLAN_STALE`, `IDEMPOTENCY_CONFLICT`, `RESOURCE_BUSY`,
`OPERATION_BUSY`, `CATALOG_REJECTED`, `HASH_MISMATCH`, `SIZE_MISMATCH`,
`ARCHIVE_REJECTED`, `DOWNLOAD_FAILED`, `PATH_CONFLICT`, `PATH_NOT_FOUND`,
`UNSUPPORTED_RUNTIME`, `UNAVAILABLE`, `OPERATION_NOT_FOUND`, `OPERATION_NOT_CANCELLABLE`,
`TIMEOUT`, and `INTERNAL_ERROR`. Clients must use `retryable` and
`next_actions`; they must not broaden privileges or bypass verification after
an error.
