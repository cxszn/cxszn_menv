import { describe, expect, it } from 'vitest';
import { generateKeyPairSync, sign } from 'node:crypto';
import {
  CoreError,
  createArtifactDownloadPlan,
  stableStringify,
  verifyCatalogSignature,
  verifyStagedArtifact,
  type CatalogTrustRoot,
  type SignedRuntimeCatalog,
} from '../src/index.js';

function unsignedCatalog(): Omit<SignedRuntimeCatalog, 'signature'> & { signature: null } {
  return {
    schema_version: '1',
    catalog_id: 'test-catalog',
    catalog_revision: 1,
    generated_at: '2026-10-01T00:00:00.000Z',
    expires_at: '2026-11-01T00:00:00.000Z',
    fixture: false,
    signature: null,
    policy: { min_app_version: '0.0.1', revoked_artifact_ids: [] },
    artifacts: [
      {
        artifact_id: 'php-8.3.12-nts-x64',
        runtime_family: 'php',
        version: '8.3.12',
        release_channel: 'stable',
        lifecycle: 'supported',
        platform: 'windows',
        arch: 'x86_64',
        thread_safety: 'nts',
        package_format: 'zip',
        source: {
          kind: 'official',
          provider: 'php.net',
          host: 'windows.php.net',
          artifact_path: '/downloads/releases/php-8.3.12-nts-Win32-vs16-x64.zip',
          redirect_hosts: ['windows.php.net'],
        },
        file_name: 'php-8.3.12-nts-Win32-vs16-x64.zip',
        size_bytes: 5,
        sha256: '9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08',
        entrypoint: 'php.exe',
        root_layout: 'archive_root',
        dependencies: { runtime: [], extensions: [] },
        flags: { legacy: false, archival: false, public_listener_default: false },
      },
    ],
  };
}

function signedCatalog() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const catalog = unsignedCatalog();
  const payload = stableStringify(catalog);
  const value = sign(null, Buffer.from(payload, 'utf8'), privateKey).toString('base64url');
  catalog.signature = { algorithm: 'ed25519', key_id: 'test-key-1', value };
  const trustRoot: CatalogTrustRoot = {
    keys: [{ key_id: 'test-key-1', public_key: publicKey.export({ type: 'spki', format: 'pem' }).toString() }],
  };
  return { catalog, trustRoot };
}

describe('signed runtime catalog', () => {
  it('verifies an Ed25519 catalog and rejects a payload tamper', () => {
    const { catalog, trustRoot } = signedCatalog();
    const verified = verifyCatalogSignature(catalog, {
      trustRoot,
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    });

    expect(verified.catalog.catalog_id).toBe('test-catalog');
    expect(verified.signature.key_id).toBe('test-key-1');

    const tampered = structuredClone(catalog);
    tampered.artifacts[0]!.size_bytes = 6;
    expect(() => verifyCatalogSignature(tampered, {
      trustRoot,
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    })).toThrowError(expect.objectContaining({ code: 'CATALOG_SIGNATURE_INVALID' }));
  });

  it('compares application versions with build metadata without producing NaN ordering', () => {
    const { catalog, trustRoot } = signedCatalog();
    const verified = verifyCatalogSignature(catalog, {
      trustRoot,
      appVersion: '0.0.2+win11.1',
      now: new Date('2026-10-11T00:00:00.000Z'),
    });

    expect(verified.catalog.policy.min_app_version).toBe('0.0.1');
  });

  it('fails closed for unknown keys, expired catalogs, revoked artifacts, and old apps', () => {
    const { catalog, trustRoot } = signedCatalog();
    expect(() => verifyCatalogSignature(catalog, {
      trustRoot: { keys: [] },
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    })).toThrowError(expect.objectContaining({ code: 'CATALOG_KEY_UNKNOWN' }));

    const expired = structuredClone(catalog);
    expired.expires_at = '2026-10-10T00:00:00.000Z';
    expect(() => verifyCatalogSignature(expired, {
      trustRoot,
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    })).toThrowError(expect.objectContaining({ code: 'CATALOG_SIGNATURE_INVALID' }));

    const oldApp = structuredClone(catalog);
    oldApp.policy.min_app_version = '0.0.3';
    {
      const { privateKey, publicKey } = generateKeyPairSync('ed25519');
      oldApp.signature = {
        algorithm: 'ed25519',
        key_id: 'old-app-key',
        value: sign(null, Buffer.from(stableStringify({ ...oldApp, signature: null })), privateKey).toString('base64url'),
      };
      trustRoot.keys = [{ key_id: 'old-app-key', public_key: publicKey.export({ type: 'spki', format: 'pem' }).toString() }];
    }
    expect(() => verifyCatalogSignature(oldApp, {
      trustRoot,
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    })).toThrowError(expect.objectContaining({ code: 'CATALOG_MIN_APP_VERSION' }));
  });

  it('creates a fixed-host HTTPS staging plan and verifies size/hash before install', () => {
    const { catalog, trustRoot } = signedCatalog();
    const verified = verifyCatalogSignature(catalog, {
      trustRoot,
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    });

    const plan = createArtifactDownloadPlan(verified, 'php-8.3.12-nts-x64', {
      stagingRoot: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\staging',
      architecture: 'x64',
    });
    expect(plan).toMatchObject({
      url: 'https://windows.php.net/downloads/releases/php-8.3.12-nts-Win32-vs16-x64.zip',
      expectedSizeBytes: 5,
      expectedSha256: '9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08',
    });
    expect(plan.stagingPath.toLowerCase()).toContain('staging');

    expect(() => verifyStagedArtifact(plan, new Uint8Array([1, 2, 3]))).toThrowError(
      expect.objectContaining({ code: 'ARTIFACT_SIZE_MISMATCH' }),
    );
    expect(() => verifyStagedArtifact(plan, new Uint8Array([1, 2, 3, 4, 5]))).toThrowError(
      expect.objectContaining({ code: 'ARTIFACT_HASH_MISMATCH' }),
    );
  });

  it('rejects fixture sources, revoked artifacts, non-HTTPS paths, and host escapes', () => {
    const { catalog, trustRoot } = signedCatalog();
    const verified = verifyCatalogSignature(catalog, {
      trustRoot,
      appVersion: '0.0.2',
      now: new Date('2026-10-11T00:00:00.000Z'),
    });
    const artifact = catalog.artifacts[0]!;

    const revoked = structuredClone(catalog);
    revoked.policy.revoked_artifact_ids = [artifact.artifact_id];
    const revokedSigned = (() => {
      const { privateKey, publicKey } = generateKeyPairSync('ed25519');
      const payload = stableStringify({ ...revoked, signature: null });
      revoked.signature = {
        algorithm: 'ed25519',
        key_id: 'revoked-key',
        value: sign(null, Buffer.from(payload), privateKey).toString('base64url'),
      };
      return verifyCatalogSignature(revoked, {
        trustRoot: { keys: [{ key_id: 'revoked-key', public_key: publicKey.export({ type: 'spki', format: 'pem' }).toString() }] },
        appVersion: '0.0.2',
        now: new Date('2026-10-11T00:00:00.000Z'),
      });
    })();
    expect(() => createArtifactDownloadPlan(revokedSigned, artifact.artifact_id, {
      stagingRoot: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\staging',
      architecture: 'x64',
    })).toThrowError(expect.objectContaining({ code: 'CATALOG_ARTIFACT_REVOKED' }));

    const hostile = structuredClone(catalog);
    hostile.artifacts[0]!.source = {
      ...hostile.artifacts[0]!.source,
      host: 'attacker.example',
      artifact_path: '/../payload.zip',
    };
    expect(() => createArtifactDownloadPlan({ ...verified, catalog: hostile }, artifact.artifact_id, {
      stagingRoot: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\staging',
      architecture: 'x64',
    })).toThrowError(expect.objectContaining({ code: 'ARTIFACT_SOURCE_NOT_ALLOWED' }));
  });
});
