#!/usr/bin/env node

/** Verify the checked-in trust root and catalog without downloading anything. */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  createArtifactDownloadPlan,
  verifyCatalogSignature,
} from '../../packages/core/dist/index.js';

const root = resolve(import.meta.dirname, '../..');
const trustRoot = JSON.parse(readFileSync(resolve(root, 'catalog/trust/runtime-root.json'), 'utf8'));
const catalog = JSON.parse(readFileSync(resolve(root, 'catalog/manifests/php-windows-official.json'), 'utf8'));
const verified = verifyCatalogSignature(catalog, {
  trustRoot,
  appVersion: process.env.MENV_APP_VERSION ?? '0.0.2',
  now: new Date(process.env.MENV_CATALOG_NOW ?? '2026-10-12T12:00:00.000Z'),
});

const plans = verified.catalog.artifacts.map((artifact) => createArtifactDownloadPlan(verified, artifact.artifact_id, {
  stagingRoot: process.env.MENV_STAGING_ROOT ?? 'C:\\Users\\Public\\AppData\\Local\\cxszn-menv\\staging',
  architecture: 'x64',
}));
console.log(JSON.stringify({
  ok: true,
  catalog_id: verified.catalog.catalog_id,
  revision: verified.catalog.catalog_revision,
  key_id: verified.keyId,
  expires_at: verified.catalog.expires_at,
  artifacts: plans.map(({ artifactId, version, url, expectedSizeBytes, expectedSha256 }) => ({
    artifact_id: artifactId,
    version,
    url,
    size_bytes: expectedSizeBytes,
    sha256: expectedSha256,
  })),
}, null, 2));
