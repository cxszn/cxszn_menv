import { createHash, randomUUID } from 'node:crypto';

export function newId(prefix: string): string {
  return `${prefix}_${randomUUID()}`;
}

export function sha256(value: string): string {
  return createHash('sha256').update(value, 'utf8').digest('hex');
}

/**
 * JSON canonicalization for small protocol/state objects. Object keys are sorted;
 * arrays retain semantic order. This is intentionally not a general-purpose
 * canonical JSON implementation and should only be used with our own records.
 */
export function stableStringify(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map((item) => stableStringify(item)).join(',')}]`;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${stableStringify(record[key])}`).join(',')}}`;
}

export function hashJson(value: unknown): string {
  return sha256(stableStringify(value));
}

export function normalizeWindowsPath(value: string): string {
  let normalized = value.trim().replaceAll('/', '\\');
  normalized = normalized.replace(/\\+$/, '');
  return normalized.length === 2 && normalized[1] === ':' ? `${normalized}\\` : normalized;
}

export function normalizePathEntry(value: string): string {
  return normalizeWindowsPath(value).toLocaleLowerCase('en-US');
}

export function hashPathEntries(entries: string[]): string {
  return sha256(stableStringify(entries.map((entry) => normalizePathEntry(entry))));
}
