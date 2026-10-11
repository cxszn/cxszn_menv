import { CoreError } from './errors.js';
import { hashPathEntries, normalizePathEntry, normalizeWindowsPath } from './hash.js';
import type { PathLeaseState, PathLeaseStore } from './types.js';

function nowIso(): string {
  return new Date().toISOString();
}

export function normalizePathEntries(entries: string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const value of entries) {
    const trimmed = value.trim();
    if (!trimmed) continue;
    const key = normalizePathEntry(trimmed);
    if (seen.has(key)) continue;
    seen.add(key);
    normalized.push(normalizeWindowsPath(trimmed));
  }
  return normalized;
}

export function createPathLeaseState(
  appPath: string | null,
  entries: string[] = [],
  enabled = false,
  runtimeId: string | null = null,
  updatedAt: string | null = null,
): PathLeaseState {
  const normalizedEntries = normalizePathEntries(entries);
  const normalizedAppPath = appPath ? normalizeWindowsPath(appPath) : null;
  return {
    scope: 'user',
    enabled: Boolean(enabled && normalizedAppPath),
    path: normalizedAppPath,
    runtimeId: enabled ? runtimeId : null,
    entries: normalizedEntries,
    hash: hashPathEntries(normalizedEntries),
    updatedAt,
  };
}

export function withAppOwnedLease(
  current: PathLeaseState,
  appPath: string,
  runtimeId: string,
  now: () => Date = () => new Date(),
): PathLeaseState {
  const normalizedAppPath = normalizeWindowsPath(appPath);
  const entries = normalizePathEntries([...current.entries, normalizedAppPath]);
  return createPathLeaseState(normalizedAppPath, entries, true, runtimeId, now().toISOString());
}

export function withoutAppOwnedLease(
  current: PathLeaseState,
  appPath: string,
  now: () => Date = () => new Date(),
): PathLeaseState {
  const key = normalizePathEntry(appPath);
  const entries = normalizePathEntries(current.entries).filter((entry) => normalizePathEntry(entry) !== key);
  return createPathLeaseState(null, entries, false, null, now().toISOString());
}

/** In-memory lease adapter used by tests and by the default non-registry core. */
export class InMemoryPathLeaseStore implements PathLeaseStore {
  private state: PathLeaseState;

  public constructor(initialEntries: string[] = []) {
    this.state = createPathLeaseState(null, initialEntries, false, null, null);
  }

  public async read(): Promise<PathLeaseState> {
    return structuredClone(this.state);
  }

  public async write(next: PathLeaseState, expectedHash: string): Promise<PathLeaseState> {
    if (this.state.hash !== expectedHash) {
      throw new CoreError('PATH_CONFLICT', 'The user PATH changed while the plan was pending.', {
        retryable: true,
        nextActions: ['Recreate the plan from the current environment state.'],
      });
    }
    const normalizedEntries = normalizePathEntries(next.entries);
    this.state = {
      ...next,
      entries: normalizedEntries,
      hash: hashPathEntries(normalizedEntries),
    };
    return structuredClone(this.state);
  }
}

export function appLeasePlan(
  before: PathLeaseState,
  action: 'enable' | 'disable',
  appPath: string,
  runtimeId: string,
): PathLeaseState {
  if (action === 'enable') return withAppOwnedLease(before, appPath, runtimeId);
  if (before.runtimeId !== null && before.runtimeId !== runtimeId) return structuredClone(before);
  return withoutAppOwnedLease(before, appPath);
}

export function pathLeaseSnapshotForState(
  pathLease: PathLeaseState,
  appPath: string | null,
  now: () => Date = () => new Date(),
): PathLeaseState {
  const entries = normalizePathEntries(pathLease.entries);
  const enabled = Boolean(pathLease.enabled && appPath && entries.some((entry) => normalizePathEntry(entry) === normalizePathEntry(appPath)));
  // Do not synthesize a timestamp while merely reading. A new timestamp would
  // make a plan appear stale between two equivalent reads of an unchanged PATH.
  return createPathLeaseState(appPath, entries, enabled, enabled ? pathLease.runtimeId : null, pathLease.updatedAt);
}
