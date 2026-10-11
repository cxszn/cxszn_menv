# Runtime catalog

`schema/runtime-catalog.schema.json` is the versioned metadata contract for
runtime artifacts. It intentionally describes a source host and a relative
artifact path rather than accepting an arbitrary URL. The application must
resolve the host against its signed allowlist before downloading anything.

`fixtures/php-manifest.fixture.json` is a metadata-only test fixture. It is
marked `fixture: true`, has a `null` signature, and uses `source.kind: fixture`
with `not_for_download: true`. It is never a download manifest and contains no
binary payload or signing material.

Production catalogs must be signed by the catalog trust root and must not be
copied from the fixture. They must include a non-null signature, an approved
official source, a SHA-256 digest, an artifact size, an architecture, and a
non-expired validity window. Keep private catalog material under
`catalog/private/`, which is intentionally ignored by version control.

The catalog verifier remains fail-closed: signature, key, expiry, revocation,
hash, size, source-host, archive-path, or platform checks must reject the
artifact rather than downgrade it to an unverified download.
