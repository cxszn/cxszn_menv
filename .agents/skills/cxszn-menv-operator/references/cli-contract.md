# MEnv CLI contract (v1)

The `menv` executable is a thin adapter over the same application service used
by the desktop and MCP surfaces. Human-readable output is for interactive use;
automation must pass `--json` and consume the shared envelope.

## Commands

| Command | Default behavior | Writes state? |
| --- | --- | --- |
| `menv doctor --json` | Report provider, catalog, PATH lease, and storage health | No |
| `menv runtimes list --json` | List known runtime records | No |
| `menv runtime scan [--family <family>] --json` | Read-only discovery | No |
| `menv runtime verify --family <family> --path <path> --json` | Read-only path verification | No |
| `menv catalog list [--family <family>] --json` | List verified catalog entries | No |
| `menv change plan --action <action> ... --json` | Produce a short-lived plan | No |
| `menv change apply --plan <id> --confirmation-token <token> --idempotency-key <key> --json` | Apply a confirmed plan | Yes |
| `menv operation get --id <operation-id> --json` | Read operation state | No |
| `menv operation cancel --id <operation-id> --json` | Request cancellation | Yes |

The command names are intentionally explicit. There is no `run-shell`,
`--url`, `--script`, `--force`, or `--yes` escape hatch.

## Shared envelope

JSON output uses the same shape as MCP:

```json
{
  "schema_version": "1",
  "request_id": "request-generated-id",
  "ok": true,
  "result": {},
  "operation_id": null
}
```

Errors retain `error.code`, `error.message`, `error.retryable`, and
`error.next_actions`. Do not parse human-readable messages as protocol fields.

## Confirmation and retry

`change plan` never mutates the machine. A host UI must issue the confirmation
token for that exact plan; the CLI cannot mint one. `change apply` requires the
token and a caller-generated idempotency key. Reuse the key for a retry of the
same request and never reuse it for a different plan.

## Exit codes

The process exit code is independent of the envelope:

- `0`: request completed successfully (including a successful read).
- `2`: invalid arguments or invalid JSON request.
- `3`: confirmation, stale-plan, path-conflict, or other user-actionable
  rejection.
- `4`: operation failed or was cancelled.
- `5`: catalog or integrity verification rejected the request.
- `10`: unexpected internal error; consult the safe error code and operation
  identifier, not raw logs.

When `--json` is used, exactly one JSON envelope is written to stdout. Logging
and diagnostics belong on stderr and must not include tokens, full environment
variables, or complete command lines.
