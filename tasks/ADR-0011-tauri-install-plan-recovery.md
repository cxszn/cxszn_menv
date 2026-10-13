# ADR-0011: Recover pending PHP install plans after desktop restart

## Status

Accepted for the 0.0.2 developer preview.

## Context

The Tauri host already persists confirmation-bound PHP install plans under the
user data root, but the desktop service kept the plan id only in a Vue-process
map. After a crash or restart, the persisted plan could not be presented again,
even though its five-minute confirmation window was still valid.

## Decision

- Add a read-only native `list_php_install_plans` command.
- Return only unexpired plans without a completed install result.
- Resolve the artifact summary from the embedded, signature-verified catalog
  using the persisted fixed filename; never accept a URL from the frontend.
- Hydrate the Tauri service's in-memory plan map from the returned plans.
- On desktop startup, show the existing install confirmation dialog for the
  first recovered plan. Applying it still issues a fresh host confirmation
  token and revalidates the persisted plan hash, snapshot and artifact spec.
- A corrupt or unavailable recovery file must not hide the current environment
  snapshot; the UI reports the recovery error and allows a new plan.

## Consequences

- A crash before confirmation no longer silently strands a valid download plan.
- Recovery is read-only until the user confirms; no download or extraction is
  triggered during startup.
- The JSON plan file remains transitional persistence. This decision does not
  claim that the final SQLite/WAL transaction model is complete.

## Verification

- Native Tauri workspace check passes.
- Desktop Tauri-service regression test maps a persisted native plan and fixed
  catalog artifact.
- Packaged install smoke and PHP activation smoke remain unchanged and pass.
