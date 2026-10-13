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

`trust/runtime-root.json` is the checked-in Ed25519 public trust root and is
validated by `schema/runtime-trust-root.schema.json`. The accompanying
`manifests/php-windows-official.json` is a generated, signed metadata catalog
for the verified x64 NTS PHP artifacts. Its sources are fixed to
`downloads.php.net/~windows/releases/`; the catalog contains no binary payload.
It currently lists seven x64 NTS artifacts: 8.2.34, 8.3.35, 8.4.26 and 8.5.11
as supported releases, plus EOL/legacy 8.1.34, 8.0.30 and 7.4.33. PHP 5.6 and
older 7.x builds remain unavailable until an official, verifiable x64 artifact
can be added.
Run `pnpm catalog:verify` to verify the signature and print safe, non-writing
download plans. Run `pnpm catalog:generate` only when refreshing the catalog;
the development private key remains in the ignored `catalog/private/` folder.

The catalog verifier remains fail-closed: signature, key, expiry, revocation,
hash, size, source-host, archive-path, or platform checks must reject the
artifact rather than downgrade it to an unverified download.
