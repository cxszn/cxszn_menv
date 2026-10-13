#!/usr/bin/env node

/**
 * Generate the checked-in PHP catalog and a local development signing key.
 *
 * The generated private key is intentionally written below catalog/private/,
 * which is ignored by Git. Never copy it into the repository or logs.
 */
import { generateKeyPairSync, sign } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const trustPath = resolve(root, 'catalog/trust/runtime-root.json');
const catalogPath = resolve(root, 'catalog/manifests/php-windows-official.json');
const privateKeyPath = resolve(root, 'catalog/private/runtime-root-dev.pem');
const keyId = 'cxszn-menv-runtime-dev-2026';

const artifacts = [
  {
    artifact_id: 'php-8.2.34-nts-x64',
    version: '8.2.34',
    file_name: 'php-8.2.34-nts-Win32-vs16-x64.zip',
    size_bytes: 33472978,
    sha256: '03249b5c9414c6dbe30276f4a7598bd9d2a7417ee81f709b06b99e9c4a2aff4f',
    published_at: '2026-09-22T09:50:00.000Z',
  },
  {
    artifact_id: 'php-8.3.35-nts-x64',
    version: '8.3.35',
    file_name: 'php-8.3.35-nts-Win32-vs16-x64.zip',
    size_bytes: 33873909,
    sha256: '25a8e2ac9ff30f1d768d1447c09a600617fa6e6082729f6e95f008b59c91fe45',
    published_at: '2026-09-22T12:05:00.000Z',
  },
  {
    artifact_id: 'php-8.4.26-nts-x64',
    version: '8.4.26',
    file_name: 'php-8.4.26-nts-Win32-vs17-x64.zip',
    size_bytes: 35103568,
    sha256: 'da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7',
    published_at: '2026-09-22T15:50:00.000Z',
  },
  {
    artifact_id: 'php-8.5.11-nts-x64',
    version: '8.5.11',
    file_name: 'php-8.5.11-nts-Win32-vs17-x64.zip',
    size_bytes: 36048897,
    sha256: '0ea96e0d2b9b737a6036f05cf4e95c49313faa6d0f27bd97edb2742503f0c043',
    published_at: '2026-09-22T14:35:00.000Z',
  },
  {
    artifact_id: 'php-8.1.34-nts-x64',
    version: '8.1.34',
    file_name: 'php-8.1.34-nts-Win32-vs16-x64.zip',
    size_bytes: 30822368,
    sha256: '9cfe246cb144076c16f5913a3ef88a474c3dd7e60f0f0c8bb95faf68674016cc',
    published_at: '2025-12-16T18:39:53.000Z',
    lifecycle: 'eol',
    release_channel: 'archival',
    legacy: true,
    archival: false,
  },
  {
    artifact_id: 'php-8.0.30-nts-x64',
    version: '8.0.30',
    file_name: 'php-8.0.30-nts-Win32-vs16-x64.zip',
    size_bytes: 26766957,
    sha256: 'dfb70498ffa2c617f2f655a155564697e3c9cca41709938fd1a5997d1d5b0785',
    published_at: '2023-09-01T14:08:48.000Z',
    lifecycle: 'eol',
    release_channel: 'archival',
    legacy: true,
    archival: false,
  },
  {
    artifact_id: 'php-7.4.33-nts-x64',
    version: '7.4.33',
    file_name: 'php-7.4.33-nts-Win32-vc15-x64.zip',
    size_bytes: 26132214,
    sha256: '14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace',
    published_at: '2022-11-02T15:06:48.000Z',
    lifecycle: 'eol',
    release_channel: 'archival',
    legacy: true,
    archival: false,
  },
].map((artifact) => {
  const {
    lifecycle = 'supported',
    release_channel = 'stable',
    legacy = false,
    archival = false,
    ...metadata
  } = artifact;
  return {
    ...metadata,
    runtime_family: 'php',
    release_channel,
    lifecycle,
    platform: 'windows',
    arch: 'x86_64',
    thread_safety: 'nts',
    package_format: 'zip',
    source: {
      kind: 'official',
      provider: 'php.net',
      host: 'downloads.php.net',
      artifact_path: `/~windows/releases/${artifact.file_name}`,
    },
    entrypoint: 'php.exe',
    root_layout: 'archive_root',
    dependencies: { runtime: [], extensions: [] },
    flags: { legacy, archival, public_listener_default: false },
  };
});

function stableStringify(value) {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(',')}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(',')}}`;
}

async function loadOrCreateKey() {
  try {
    const privatePem = await readFile(privateKeyPath, 'utf8');
    return { privateKey: privatePem, publicKey: null };
  } catch {
    const pair = generateKeyPairSync('ed25519');
    const privatePem = pair.privateKey.export({ type: 'pkcs8', format: 'pem' }).toString();
    const publicDer = pair.publicKey.export({ type: 'spki', format: 'der' });
    const rawPublic = publicDer.subarray(publicDer.length - 32).toString('base64url');
    await mkdir(dirname(privateKeyPath), { recursive: true });
    await writeFile(privateKeyPath, privatePem, { encoding: 'utf8', mode: 0o600 });
    return { privateKey: privatePem, publicKey: rawPublic };
  }
}

const key = await loadOrCreateKey();
let publicKey = key.publicKey;
if (publicKey === null) {
  const { createPublicKey } = await import('node:crypto');
  const der = createPublicKey(key.privateKey).export({ type: 'spki', format: 'der' });
  publicKey = der.subarray(der.length - 32).toString('base64url');
}

const payload = {
  schema_version: '1',
  catalog_id: 'php-windows-official',
  catalog_revision: 2,
  generated_at: '2026-10-12T00:00:00.000Z',
  expires_at: '2027-01-12T00:00:00.000Z',
  fixture: false,
  signature: null,
  policy: { min_app_version: '0.0.2', revoked_artifact_ids: [] },
  artifacts,
};
const signature = sign(null, Buffer.from(stableStringify(payload), 'utf8'), key.privateKey).toString('base64url');
const catalog = { ...payload, signature: { algorithm: 'ed25519', key_id: keyId, value: signature } };

await mkdir(dirname(trustPath), { recursive: true });
await mkdir(dirname(catalogPath), { recursive: true });
await writeFile(trustPath, `${JSON.stringify({ schema_version: '1', purpose: 'runtime-catalog', keys: [{ key_id: keyId, algorithm: 'ed25519', public_key: publicKey, not_before: '2026-10-12T00:00:00.000Z', not_after: '2027-10-12T00:00:00.000Z' }] }, null, 2)}\n`, 'utf8');
await writeFile(catalogPath, `${JSON.stringify(catalog, null, 2)}\n`, 'utf8');
console.log(`wrote ${trustPath}`);
console.log(`wrote ${catalogPath}`);
console.log(`private key kept at ignored path ${privateKeyPath}`);
