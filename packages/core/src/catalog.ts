import { createHash, createPublicKey, verify as verifySignature } from 'node:crypto';
import { win32 as win32Path } from 'node:path';
import { CoreError } from './errors.js';
import { stableStringify } from './hash.js';

/** The only catalog signature scheme currently accepted by the application. */
export interface CatalogSignature {
  algorithm: 'ed25519';
  key_id: string;
  value: string;
}

export interface CatalogTrustKey {
  key_id: string;
  /** SPKI PEM or a base64/base64url encoded raw Ed25519 public key. */
  public_key: string;
  revoked?: boolean;
  not_before?: string;
  not_after?: string;
}

export interface CatalogTrustRoot {
  keys: CatalogTrustKey[];
}

export interface CatalogPolicy {
  min_app_version: string;
  revoked_artifact_ids: string[];
}

export type CatalogRuntimeFamily = 'php' | 'node' | 'mysql' | 'python';
export type CatalogArtifactArchitecture = 'x86_64' | 'x86' | 'arm64';
export type CatalogArtifactThreadSafety = 'ts' | 'nts' | 'not_applicable';
export type CatalogArtifactLifecycle = 'supported' | 'eol' | 'archival' | 'revoked';
export type CatalogArtifactChannel = 'stable' | 'beta' | 'nightly' | 'archival';
export type CatalogPackageFormat = 'zip' | 'tar_gz' | 'installer' | 'directory';

export interface OfficialArtifactSource {
  kind: 'official';
  provider: 'php.net' | 'nodejs.org' | 'mysql.com' | 'python.org' | 'other';
  host: string;
  artifact_path: string;
  redirect_hosts?: string[];
}

export interface FixtureArtifactSource {
  kind: 'fixture';
  fixture_id: string;
  not_for_download: true;
}

export interface RuntimeArtifact {
  artifact_id: string;
  runtime_family: CatalogRuntimeFamily;
  version: string;
  release_channel: CatalogArtifactChannel;
  lifecycle: CatalogArtifactLifecycle;
  platform: 'windows';
  arch: CatalogArtifactArchitecture;
  thread_safety: CatalogArtifactThreadSafety;
  package_format: CatalogPackageFormat;
  source: OfficialArtifactSource | FixtureArtifactSource;
  file_name: string;
  size_bytes: number;
  sha256: string;
  entrypoint: string;
  root_layout: 'archive_root' | 'direct' | 'nested';
  dependencies: { runtime: string[]; extensions: string[] };
  flags: { legacy: boolean; archival: boolean; public_listener_default: false };
  published_at?: string;
  deprecated_after?: string;
}

export interface SignedRuntimeCatalog {
  schema_version: '1';
  catalog_id: string;
  catalog_revision: number;
  generated_at: string;
  expires_at: string;
  fixture: boolean;
  fixture_note?: string;
  signature: CatalogSignature | null;
  policy: CatalogPolicy;
  artifacts: RuntimeArtifact[];
}

export interface VerifyCatalogOptions {
  trustRoot: CatalogTrustRoot;
  appVersion: string;
  now?: Date;
  /** Only test fixtures may use a null signature. Never enable this for production input. */
  allowFixture?: boolean;
}

export interface VerifiedCatalog {
  catalog: SignedRuntimeCatalog;
  signature: CatalogSignature;
  keyId: string;
  verifiedAt: string;
}

export interface ArtifactDownloadPlan {
  catalogId: string;
  catalogRevision: number;
  artifactId: string;
  runtimeFamily: CatalogRuntimeFamily;
  version: string;
  architecture: CatalogArtifactArchitecture;
  threadSafety: CatalogArtifactThreadSafety;
  packageFormat: CatalogPackageFormat;
  url: string;
  fileName: string;
  stagingDirectory: string;
  stagingPath: string;
  expectedSizeBytes: number;
  expectedSha256: string;
  entrypoint: string;
  rootLayout: RuntimeArtifact['root_layout'];
  legacy: boolean;
  archival: boolean;
}

export interface ArtifactDownloadPlanOptions {
  stagingRoot: string;
  architecture?: CatalogArtifactArchitecture | 'x64' | 'x86';
}

const MAX_CATALOG_ARTIFACT_BYTES = 4 * 1024 * 1024 * 1024;
const ID_PATTERN = /^[a-z0-9][a-z0-9._-]{2,127}$/;
const KEY_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{2,63}$/;
const VERSION_PATTERN = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const HOST_PATTERN = /^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/;
const SHA256_PATTERN = /^[a-f0-9]{64}$/i;
const BASE64URL_PATTERN = /^[A-Za-z0-9_-]+$/;
const FILE_NAME_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/;
const ENTRYPOINT_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}\.exe$/;

const FIXED_SOURCE_HOSTS: Record<CatalogRuntimeFamily, ReadonlySet<string>> = {
  php: new Set(['php.net', 'www.php.net', 'windows.php.net', 'downloads.php.net']),
  node: new Set(['nodejs.org', 'www.nodejs.org', 'github.com', 'objects.githubusercontent.com']),
  mysql: new Set(['mysql.com', 'dev.mysql.com', 'cdn.mysql.com', 'downloads.mysql.com']),
  python: new Set(['python.org', 'www.python.org', 'downloads.python.org']),
};

function object(value: unknown, field: string): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new CoreError('CATALOG_INVALID', `${field} must be an object.`);
  }
  return value as Record<string, unknown>;
}

function string(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new CoreError('CATALOG_INVALID', `${field} must be a non-empty string.`);
  }
  return value;
}

function bool(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw new CoreError('CATALOG_INVALID', `${field} must be boolean.`);
  return value;
}

function array(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) throw new CoreError('CATALOG_INVALID', `${field} must be an array.`);
  return value;
}

function own(value: Record<string, unknown>, field: string): unknown {
  if (!Object.prototype.hasOwnProperty.call(value, field)) {
    throw new CoreError('CATALOG_INVALID', `${field} is required.`);
  }
  return value[field];
}

function rejectUnknown(value: Record<string, unknown>, allowed: readonly string[], field: string): void {
  const accepted = new Set(allowed);
  for (const key of Object.keys(value)) {
    if (!accepted.has(key)) throw new CoreError('CATALOG_INVALID', `${field}.${key} is not allowed.`);
  }
}

function parseSemver(value: string, field: string): { major: number; minor: number; patch: number; pre: string[] } {
  if (!VERSION_PATTERN.test(value)) throw new CoreError('CATALOG_INVALID', `${field} is not a valid semantic version.`);
  // Build metadata does not affect precedence; remove it before numeric
  // ordering so values such as 0.0.2+win11.1 never produce NaN components.
  const [withoutBuild] = value.split('+', 2);
  const [core, preValue] = (withoutBuild ?? value).split('-', 2);
  const parts = (core ?? '').split('.').map((part) => Number(part));
  const major = parts[0];
  const minor = parts[1];
  const patch = parts[2];
  if (major === undefined || minor === undefined || patch === undefined) {
    throw new CoreError('CATALOG_INVALID', `${field} is not a valid semantic version.`);
  }
  return { major, minor, patch, pre: preValue ? preValue.split('.') : [] };
}

function compareVersions(left: string, right: string): number {
  const a = parseSemver(left, 'appVersion');
  const b = parseSemver(right, 'min_app_version');
  for (const key of ['major', 'minor', 'patch'] as const) {
    if (a[key] !== b[key]) return a[key] > b[key] ? 1 : -1;
  }
  if (a.pre.length === 0 && b.pre.length !== 0) return 1;
  if (a.pre.length !== 0 && b.pre.length === 0) return -1;
  for (let index = 0; index < Math.max(a.pre.length, b.pre.length); index += 1) {
    const av = a.pre[index];
    const bv = b.pre[index];
    if (av === undefined) return -1;
    if (bv === undefined) return 1;
    if (av === bv) continue;
    const an = /^\d+$/.test(av) ? Number(av) : null;
    const bn = /^\d+$/.test(bv) ? Number(bv) : null;
    if (an !== null && bn !== null) return an > bn ? 1 : -1;
    if (an !== null) return -1;
    if (bn !== null) return 1;
    return av > bv ? 1 : -1;
  }
  return 0;
}

function parseTimestamp(value: unknown, field: string): string {
  const timestamp = string(value, field);
  const parsed = Date.parse(timestamp);
  if (!Number.isFinite(parsed)) throw new CoreError('CATALOG_INVALID', `${field} must be an ISO date-time.`);
  return timestamp;
}

function parseHost(value: unknown, field: string): string {
  const host = string(value, field);
  if (host !== host.toLowerCase() || !HOST_PATTERN.test(host) || host.includes('..')) {
    throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', `${field} is not a valid fixed host.`);
  }
  return host;
}

function validateArtifactPath(value: unknown): string {
  const path = string(value, 'source.artifact_path');
  let decoded = path;
  try {
    decoded = decodeURIComponent(path);
  } catch {
    throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', 'source.artifact_path contains invalid escaping.');
  }
  const segments = decoded.split('/');
  if (!path.startsWith('/') || path.includes('\\') || path.includes('?') || path.includes('#') || segments.includes('..') || segments.slice(1).includes('') || path === '/') {
    throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', 'source.artifact_path contains an unsafe path segment.');
  }
  return path;
}

function decodeBase64Url(value: string): Buffer {
  if (!BASE64URL_PATTERN.test(value)) throw new CoreError('CATALOG_SIGNATURE_INVALID', 'Catalog signature is not base64url encoded.');
  const padded = value.replaceAll('-', '+').replaceAll('_', '/') + '='.repeat((4 - (value.length % 4)) % 4);
  return Buffer.from(padded, 'base64');
}

function publicKey(value: string): ReturnType<typeof createPublicKey> {
  try {
    if (value.includes('BEGIN PUBLIC KEY')) return createPublicKey(value);
    const raw = decodeBase64Url(value);
    if (raw.length !== 32) throw new Error('raw Ed25519 public keys must be 32 bytes');
    const der = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw]);
    return createPublicKey({ key: der, format: 'der', type: 'spki' });
  } catch (error) {
    throw new CoreError('CATALOG_SIGNATURE_INVALID', `Catalog trust key is invalid: ${error instanceof Error ? error.message : String(error)}`);
  }
}

function validateSignature(value: unknown): CatalogSignature {
  const signature = object(value, 'signature');
  rejectUnknown(signature, ['algorithm', 'key_id', 'value'], 'signature');
  if (signature.algorithm !== 'ed25519') throw new CoreError('CATALOG_SIGNATURE_INVALID', 'Only Ed25519 catalog signatures are accepted.');
  const keyId = string(own(signature, 'key_id'), 'signature.key_id');
  const encoded = string(own(signature, 'value'), 'signature.value');
  if (!KEY_ID_PATTERN.test(keyId) || !BASE64URL_PATTERN.test(encoded)) {
    throw new CoreError('CATALOG_SIGNATURE_INVALID', 'Catalog signature key id or value is malformed.');
  }
  const bytes = decodeBase64Url(encoded);
  if (bytes.length !== 64) throw new CoreError('CATALOG_SIGNATURE_INVALID', 'Ed25519 signature must be 64 bytes.');
  return { algorithm: 'ed25519', key_id: keyId, value: encoded };
}

function validateArtifact(value: unknown, index: number, fixture: boolean): RuntimeArtifact {
  const field = `artifacts[${index}]`;
  const artifact = object(value, field);
  rejectUnknown(artifact, [
    'artifact_id', 'runtime_family', 'version', 'release_channel', 'lifecycle', 'platform', 'arch', 'thread_safety',
    'package_format', 'source', 'file_name', 'size_bytes', 'sha256', 'entrypoint', 'root_layout', 'dependencies', 'flags',
    'published_at', 'deprecated_after',
  ], field);
  const id = string(own(artifact, 'artifact_id'), `${field}.artifact_id`);
  if (!ID_PATTERN.test(id)) throw new CoreError('CATALOG_INVALID', `${field}.artifact_id is malformed.`);
  const family = own(artifact, 'runtime_family');
  if (family !== 'php' && family !== 'node' && family !== 'mysql' && family !== 'python') throw new CoreError('CATALOG_INVALID', `${field}.runtime_family is invalid.`);
  const version = string(own(artifact, 'version'), `${field}.version`);
  parseSemver(version, `${field}.version`);
  const channel = own(artifact, 'release_channel');
  if (!['stable', 'beta', 'nightly', 'archival'].includes(String(channel))) throw new CoreError('CATALOG_INVALID', `${field}.release_channel is invalid.`);
  const lifecycle = own(artifact, 'lifecycle');
  if (!['supported', 'eol', 'archival', 'revoked'].includes(String(lifecycle))) throw new CoreError('CATALOG_INVALID', `${field}.lifecycle is invalid.`);
  if (own(artifact, 'platform') !== 'windows') throw new CoreError('ARTIFACT_PLATFORM_UNSUPPORTED', `${field}.platform is not windows.`);
  const arch = own(artifact, 'arch');
  if (!['x86_64', 'x86', 'arm64'].includes(String(arch))) throw new CoreError('ARTIFACT_PLATFORM_UNSUPPORTED', `${field}.arch is unsupported.`);
  const threadSafety = own(artifact, 'thread_safety');
  if (!['ts', 'nts', 'not_applicable'].includes(String(threadSafety))) throw new CoreError('CATALOG_INVALID', `${field}.thread_safety is invalid.`);
  const packageFormat = own(artifact, 'package_format');
  if (!['zip', 'tar_gz', 'installer', 'directory'].includes(String(packageFormat))) throw new CoreError('CATALOG_INVALID', `${field}.package_format is invalid.`);

  const sourceValue = object(own(artifact, 'source'), `${field}.source`);
  let source: OfficialArtifactSource | FixtureArtifactSource;
  if (sourceValue.kind === 'fixture') {
    rejectUnknown(sourceValue, ['kind', 'fixture_id', 'not_for_download'], `${field}.source`);
    if (!fixture || sourceValue.not_for_download !== true) throw new CoreError('CATALOG_FIXTURE_NOT_DOWNLOADABLE', `${field}.source fixture is not downloadable.`);
    const fixtureId = string(own(sourceValue, 'fixture_id'), `${field}.source.fixture_id`);
    if (!ID_PATTERN.test(fixtureId)) throw new CoreError('CATALOG_INVALID', `${field}.source.fixture_id is malformed.`);
    source = { kind: 'fixture', fixture_id: fixtureId, not_for_download: true };
  } else {
    rejectUnknown(sourceValue, ['kind', 'provider', 'host', 'artifact_path', 'redirect_hosts'], `${field}.source`);
    if (sourceValue.kind !== 'official') throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', `${field}.source.kind is not official.`);
    const provider = own(sourceValue, 'provider');
    if (!['php.net', 'nodejs.org', 'mysql.com', 'python.org', 'other'].includes(String(provider))) throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', `${field}.source.provider is invalid.`);
    const host = parseHost(own(sourceValue, 'host'), `${field}.source.host`);
    const artifactPath = validateArtifactPath(own(sourceValue, 'artifact_path'));
    const redirectHostsValue = sourceValue.redirect_hosts === undefined ? [] : array(sourceValue.redirect_hosts, `${field}.source.redirect_hosts`);
    const redirectHosts = redirectHostsValue.map((hostValue, redirectIndex) => parseHost(hostValue, `${field}.source.redirect_hosts[${redirectIndex}]`));
    source = { kind: 'official', provider: provider as OfficialArtifactSource['provider'], host, artifact_path: artifactPath, ...(redirectHosts.length > 0 ? { redirect_hosts: redirectHosts } : {}) };
  }

  const fileName = string(own(artifact, 'file_name'), `${field}.file_name`);
  if (!FILE_NAME_PATTERN.test(fileName) || fileName.includes('..')) throw new CoreError('CATALOG_INVALID', `${field}.file_name is unsafe.`);
  const size = own(artifact, 'size_bytes');
  if (typeof size !== 'number' || !Number.isSafeInteger(size) || size < 1 || size > MAX_CATALOG_ARTIFACT_BYTES) throw new CoreError('CATALOG_INVALID', `${field}.size_bytes is outside the allowed range.`);
  const digest = string(own(artifact, 'sha256'), `${field}.sha256`);
  if (!SHA256_PATTERN.test(digest)) throw new CoreError('CATALOG_INVALID', `${field}.sha256 is malformed.`);
  const entrypoint = string(own(artifact, 'entrypoint'), `${field}.entrypoint`);
  if (!ENTRYPOINT_PATTERN.test(entrypoint) || entrypoint.includes('..')) throw new CoreError('CATALOG_INVALID', `${field}.entrypoint is unsafe.`);
  const rootLayout = own(artifact, 'root_layout');
  if (!['archive_root', 'direct', 'nested'].includes(String(rootLayout))) throw new CoreError('CATALOG_INVALID', `${field}.root_layout is invalid.`);
  const deps = object(own(artifact, 'dependencies'), `${field}.dependencies`);
  rejectUnknown(deps, ['runtime', 'extensions'], `${field}.dependencies`);
  const runtimeDeps = array(own(deps, 'runtime'), `${field}.dependencies.runtime`).map((item, depIndex) => string(item, `${field}.dependencies.runtime[${depIndex}]`));
  const extensionDeps = array(own(deps, 'extensions'), `${field}.dependencies.extensions`).map((item, depIndex) => string(item, `${field}.dependencies.extensions[${depIndex}]`));
  const flags = object(own(artifact, 'flags'), `${field}.flags`);
  rejectUnknown(flags, ['legacy', 'archival', 'public_listener_default'], `${field}.flags`);
  const legacy = bool(own(flags, 'legacy'), `${field}.flags.legacy`);
  const archival = bool(own(flags, 'archival'), `${field}.flags.archival`);
  if (own(flags, 'public_listener_default') !== false) throw new CoreError('CATALOG_INVALID', `${field}.flags.public_listener_default must be false.`);
  if ((lifecycle === 'eol' || lifecycle === 'archival' || lifecycle === 'revoked') && !legacy) throw new CoreError('CATALOG_INVALID', `${field}.flags.legacy must be true for EOL/archival artifacts.`);
  if ((lifecycle === 'archival' || lifecycle === 'revoked') && !archival) throw new CoreError('CATALOG_INVALID', `${field}.flags.archival must be true for archival/revoked artifacts.`);

  const result: RuntimeArtifact = {
    artifact_id: id,
    runtime_family: family as CatalogRuntimeFamily,
    version,
    release_channel: channel as CatalogArtifactChannel,
    lifecycle: lifecycle as CatalogArtifactLifecycle,
    platform: 'windows',
    arch: arch as CatalogArtifactArchitecture,
    thread_safety: threadSafety as CatalogArtifactThreadSafety,
    package_format: packageFormat as CatalogPackageFormat,
    source,
    file_name: fileName,
    size_bytes: size,
    sha256: digest.toLowerCase(),
    entrypoint,
    root_layout: rootLayout as RuntimeArtifact['root_layout'],
    dependencies: { runtime: runtimeDeps, extensions: extensionDeps },
    flags: { legacy, archival, public_listener_default: false },
  };
  if (artifact.published_at !== undefined) result.published_at = parseTimestamp(artifact.published_at, `${field}.published_at`);
  if (artifact.deprecated_after !== undefined) result.deprecated_after = parseTimestamp(artifact.deprecated_after, `${field}.deprecated_after`);
  return result;
}

function validateCatalog(value: unknown, allowFixture: boolean): SignedRuntimeCatalog {
  const catalog = object(value, 'catalog');
  rejectUnknown(catalog, ['schema_version', 'catalog_id', 'catalog_revision', 'generated_at', 'expires_at', 'fixture', 'fixture_note', 'signature', 'policy', 'artifacts'], 'catalog');
  if (own(catalog, 'schema_version') !== '1') throw new CoreError('CATALOG_INVALID', 'Unsupported catalog schema version.');
  const catalogId = string(own(catalog, 'catalog_id'), 'catalog.catalog_id');
  if (!/^[a-z0-9][a-z0-9._-]{2,63}$/.test(catalogId)) throw new CoreError('CATALOG_INVALID', 'catalog.catalog_id is malformed.');
  const revision = own(catalog, 'catalog_revision');
  if (typeof revision !== 'number' || !Number.isSafeInteger(revision) || revision < 1) throw new CoreError('CATALOG_INVALID', 'catalog.catalog_revision must be a positive integer.');
  const generatedAt = parseTimestamp(own(catalog, 'generated_at'), 'catalog.generated_at');
  const expiresAt = parseTimestamp(own(catalog, 'expires_at'), 'catalog.expires_at');
  if (Date.parse(expiresAt) <= Date.parse(generatedAt)) throw new CoreError('CATALOG_INVALID', 'catalog.expires_at must be after generated_at.');
  const fixture = bool(own(catalog, 'fixture'), 'catalog.fixture');
  if (fixture && !allowFixture) throw new CoreError('CATALOG_FIXTURE_NOT_DOWNLOADABLE', 'Fixture catalogs are not accepted for production verification.');
  if (fixture) {
    const note = string(own(catalog, 'fixture_note'), 'catalog.fixture_note');
    if (note.length < 12 || note.length > 240) throw new CoreError('CATALOG_INVALID', 'catalog.fixture_note has an invalid length.');
  }

  const signatureValue = own(catalog, 'signature');
  let signature: CatalogSignature | null;
  if (fixture) {
    if (signatureValue !== null) throw new CoreError('CATALOG_INVALID', 'Fixture catalogs must not contain a signature.');
    signature = null;
  } else {
    signature = validateSignature(signatureValue);
  }

  const policyValue = object(own(catalog, 'policy'), 'catalog.policy');
  rejectUnknown(policyValue, ['min_app_version', 'revoked_artifact_ids'], 'catalog.policy');
  const minAppVersion = string(own(policyValue, 'min_app_version'), 'catalog.policy.min_app_version');
  parseSemver(minAppVersion, 'catalog.policy.min_app_version');
  const revoked = array(own(policyValue, 'revoked_artifact_ids'), 'catalog.policy.revoked_artifact_ids').map((id, index) => {
    const value = string(id, `catalog.policy.revoked_artifact_ids[${index}]`);
    if (!ID_PATTERN.test(value)) throw new CoreError('CATALOG_INVALID', `catalog.policy.revoked_artifact_ids[${index}] is malformed.`);
    return value;
  });
  if (new Set(revoked).size !== revoked.length) throw new CoreError('CATALOG_INVALID', 'catalog.policy.revoked_artifact_ids must be unique.');
  const artifactValues = array(own(catalog, 'artifacts'), 'catalog.artifacts');
  if (artifactValues.length === 0) throw new CoreError('CATALOG_INVALID', 'catalog.artifacts must not be empty.');
  const artifacts = artifactValues.map((artifact, index) => validateArtifact(artifact, index, fixture));
  if (new Set(artifacts.map((artifact) => artifact.artifact_id)).size !== artifacts.length) throw new CoreError('CATALOG_INVALID', 'catalog.artifacts contains duplicate artifact ids.');

  return {
    schema_version: '1',
    catalog_id: catalogId,
    catalog_revision: revision,
    generated_at: generatedAt,
    expires_at: expiresAt,
    fixture,
    ...(catalog.fixture_note === undefined ? {} : { fixture_note: string(catalog.fixture_note, 'catalog.fixture_note') }),
    signature,
    policy: { min_app_version: minAppVersion, revoked_artifact_ids: revoked },
    artifacts,
  };
}

function catalogPayload(catalog: Record<string, unknown>): string {
  const payload = { ...catalog, signature: null };
  return stableStringify(payload);
}

/** Verify a signed catalog before exposing any artifact metadata to an installer. */
export function verifyCatalogSignature(value: unknown, options: VerifyCatalogOptions): VerifiedCatalog {
  const catalog = validateCatalog(value, options.allowFixture === true);
  const now = options.now ?? new Date();
  const nowMs = now.getTime();
  if (!Number.isFinite(nowMs)) throw new CoreError('CATALOG_INVALID', 'Verification clock is invalid.');
  if (catalog.signature === null) {
    if (nowMs < Date.parse(catalog.generated_at)) throw new CoreError('CATALOG_NOT_YET_VALID', 'Catalog is not valid yet.');
    if (nowMs >= Date.parse(catalog.expires_at)) throw new CoreError('CATALOG_EXPIRED', 'Catalog has expired.');
    if (compareVersions(options.appVersion, catalog.policy.min_app_version) < 0) {
      throw new CoreError('CATALOG_MIN_APP_VERSION', 'The installed application is below the catalog minimum version.');
    }
    return { catalog, signature: { algorithm: 'ed25519', key_id: 'fixture', value: '' }, keyId: 'fixture', verifiedAt: now.toISOString() };
  }
  const key = options.trustRoot.keys.find((candidate) => candidate.key_id === catalog.signature?.key_id);
  if (!key) throw new CoreError('CATALOG_KEY_UNKNOWN', `Catalog signing key ${catalog.signature.key_id} is not trusted.`);
  if (key.revoked === true) throw new CoreError('CATALOG_KEY_REVOKED', `Catalog signing key ${key.key_id} is revoked.`);
  if (key.not_before !== undefined && nowMs < Date.parse(parseTimestamp(key.not_before, `trustRoot.${key.key_id}.not_before`))) throw new CoreError('CATALOG_KEY_REVOKED', `Catalog signing key ${key.key_id} is not active yet.`);
  if (key.not_after !== undefined && nowMs >= Date.parse(parseTimestamp(key.not_after, `trustRoot.${key.key_id}.not_after`))) throw new CoreError('CATALOG_KEY_REVOKED', `Catalog signing key ${key.key_id} has expired.`);

  const signatureBytes = decodeBase64Url(catalog.signature.value);
  let verified = false;
  try {
    verified = verifySignature(null, Buffer.from(catalogPayload(value as Record<string, unknown>), 'utf8'), publicKey(key.public_key), signatureBytes);
  } catch {
    verified = false;
  }
  if (!verified) throw new CoreError('CATALOG_SIGNATURE_INVALID', 'Catalog signature verification failed.');
  if (nowMs < Date.parse(catalog.generated_at)) throw new CoreError('CATALOG_NOT_YET_VALID', 'Catalog is not valid yet.');
  if (nowMs >= Date.parse(catalog.expires_at)) throw new CoreError('CATALOG_EXPIRED', 'Catalog has expired.');
  if (compareVersions(options.appVersion, catalog.policy.min_app_version) < 0) {
    throw new CoreError('CATALOG_MIN_APP_VERSION', 'The installed application is below the catalog minimum version.');
  }
  return { catalog, signature: catalog.signature, keyId: key.key_id, verifiedAt: now.toISOString() };
}

function architecture(value: ArtifactDownloadPlanOptions['architecture']): CatalogArtifactArchitecture | undefined {
  if (value === undefined) return undefined;
  if (value === 'x64') return 'x86_64';
  if (value === 'x86') return 'x86';
  return value;
}

function validateStagingRoot(value: string): string {
  if (typeof value !== 'string' || value.trim().length === 0) throw new CoreError('STAGING_PATH_INVALID', 'stagingRoot must be an absolute local path.');
  const root = value.trim().replaceAll('/', '\\').replace(/\\+$/, '');
  if (!/^[A-Za-z]:\\/.test(root) || root.includes('\\\\') || root.includes('..')) throw new CoreError('STAGING_PATH_INVALID', 'stagingRoot must be a local absolute path without traversal.');
  return root;
}

function sourceForPlan(artifact: RuntimeArtifact): OfficialArtifactSource {
  if (artifact.source.kind !== 'official') throw new CoreError('CATALOG_FIXTURE_NOT_DOWNLOADABLE', 'Fixture artifacts never produce download plans.');
  const hosts = FIXED_SOURCE_HOSTS[artifact.runtime_family];
  if (!hosts.has(artifact.source.host)) throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', `Source host ${artifact.source.host} is not fixed for ${artifact.runtime_family}.`);
  for (const host of artifact.source.redirect_hosts ?? []) {
    if (!hosts.has(host)) throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', `Redirect host ${host} is not fixed for ${artifact.runtime_family}.`);
  }
  if (artifact.source.artifact_path.includes('..')) throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', 'Artifact path contains traversal.');
  const url = `https://${artifact.source.host}${artifact.source.artifact_path}`;
  const parsed = new URL(url);
  if (parsed.protocol !== 'https:' || parsed.hostname !== artifact.source.host || parsed.search || parsed.hash) throw new CoreError('ARTIFACT_SOURCE_NOT_ALLOWED', 'Artifact URL is not a fixed HTTPS URL.');
  return artifact.source;
}

/** Build a non-executable staging plan. This function performs no network or filesystem writes. */
export function createArtifactDownloadPlan(verified: VerifiedCatalog, artifactId: string, options: ArtifactDownloadPlanOptions): ArtifactDownloadPlan {
  if (!ID_PATTERN.test(artifactId)) throw new CoreError('CATALOG_INVALID', 'artifactId is malformed.');
  const artifact = verified.catalog.artifacts.find((candidate) => candidate.artifact_id === artifactId);
  if (!artifact) throw new CoreError('CATALOG_INVALID', `Artifact ${artifactId} was not found in the verified catalog.`);
  if (verified.catalog.policy.revoked_artifact_ids.includes(artifact.artifact_id) || artifact.lifecycle === 'revoked') {
    throw new CoreError('CATALOG_ARTIFACT_REVOKED', `Artifact ${artifact.artifact_id} is revoked.`);
  }
  if (artifact.platform !== 'windows' || (options.architecture !== undefined && architecture(options.architecture) !== artifact.arch)) {
    throw new CoreError('ARTIFACT_PLATFORM_UNSUPPORTED', `Artifact ${artifact.artifact_id} does not match this Windows architecture.`);
  }
  const source = sourceForPlan(artifact);
  const root = validateStagingRoot(options.stagingRoot);
  const stagingDirectory = win32Path.join(root, verified.catalog.catalog_id, artifact.artifact_id);
  const stagingPath = win32Path.join(stagingDirectory, artifact.file_name);
  const rootPrefix = root.endsWith('\\') ? root : `${root}\\`;
  if (!stagingPath.toLowerCase().startsWith(rootPrefix.toLowerCase())) throw new CoreError('STAGING_PATH_INVALID', 'Resolved staging path escapes the staging root.');
  return {
    catalogId: verified.catalog.catalog_id,
    catalogRevision: verified.catalog.catalog_revision,
    artifactId: artifact.artifact_id,
    runtimeFamily: artifact.runtime_family,
    version: artifact.version,
    architecture: artifact.arch,
    threadSafety: artifact.thread_safety,
    packageFormat: artifact.package_format,
    url: `https://${source.host}${source.artifact_path}`,
    fileName: artifact.file_name,
    stagingDirectory,
    stagingPath,
    expectedSizeBytes: artifact.size_bytes,
    expectedSha256: artifact.sha256,
    entrypoint: artifact.entrypoint,
    rootLayout: artifact.root_layout,
    legacy: artifact.flags.legacy,
    archival: artifact.flags.archival,
  };
}

/** Verify downloaded bytes before an installer is allowed to move them out of staging. */
export function verifyStagedArtifact(plan: ArtifactDownloadPlan, bytes: Uint8Array): void {
  if (bytes.byteLength !== plan.expectedSizeBytes) throw new CoreError('ARTIFACT_SIZE_MISMATCH', `Downloaded artifact size ${bytes.byteLength} does not match the catalog size ${plan.expectedSizeBytes}.`);
  const digest = createHash('sha256').update(bytes).digest('hex');
  if (digest !== plan.expectedSha256.toLowerCase()) throw new CoreError('ARTIFACT_HASH_MISMATCH', 'Downloaded artifact SHA-256 does not match the catalog.');
}
