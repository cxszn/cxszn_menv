import { describe, expect, it } from 'vitest';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import {
  InMemoryPlanStore,
  JsonPathLeaseStore,
  JsonPlanStore,
  JsonStateStore,
  NodeSqliteJsonTextStore,
  emptyRuntimeState,
  type JsonTextStore,
} from '../src/index.js';

class MemoryTextStore implements JsonTextStore {
  public readonly values = new Map<string, string>();

  public async read(path: string): Promise<string | null> {
    return this.values.get(path) ?? null;
  }

  public async write(path: string, content: string): Promise<void> {
    this.values.set(path, content);
  }
}

describe('JSON persistence adapters', () => {
  it('round-trips runtime state without requiring a native SQLite/registry adapter', async () => {
    const text = new MemoryTextStore();
    const store = new JsonStateStore('state.json', text);
    const state = emptyRuntimeState(() => new Date('2026-10-11T00:00:00.000Z'));
    state.runtimes.push({
      id: 'php-test',
      family: 'php',
      version: '8.3.12',
      executablePath: 'C:\\Tools\\PHP\\php.exe',
      rootPath: 'C:\\Tools\\PHP',
      architecture: 'x64',
      threadSafety: 'nts',
      availability: 'manual',
      ownership: 'manual',
      activation: 'disabled',
      health: 'healthy',
      ini: { source: 'unknown', path: null },
      legacy: false,
      diagnostics: [],
      firstSeenAt: state.updatedAt,
      lastCheckedAt: state.updatedAt,
    });

    await store.save(state);
    const restored = await store.load();

    expect(restored).toEqual(state);
  });

  it('preserves external PATH entries and detects lease conflicts', async () => {
    const text = new MemoryTextStore();
    const store = new JsonPathLeaseStore('path.json', text);
    const initial = await store.read();
    const next = {
      ...initial,
      enabled: true,
      path: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin',
      runtimeId: 'php-test',
      entries: ['C:\\Windows\\System32', 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin'],
      updatedAt: '2026-10-11T00:00:00.000Z',
    };
    const written = await store.write(next, initial.hash);

    expect(written.entries).toEqual(['C:\\Windows\\System32', 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin']);
    await expect(store.write(next, initial.hash)).rejects.toMatchObject({ code: 'PATH_CONFLICT' });
  });

  it('stores confirmation hashes rather than plaintext tokens', async () => {
    const text = new MemoryTextStore();
    const store = new JsonPlanStore('plans.json', text);
    const plan = {
      schemaVersion: '1' as const,
      planId: 'plan-test',
      planHash: 'hash',
      action: 'enable' as const,
      runtimeId: 'php-test',
      targetRuntimeId: null,
      scope: 'process' as const,
      requiresConfirmation: true as const,
      createdAt: '2026-10-11T00:00:00.000Z',
      expiresAt: '2026-10-11T01:00:00.000Z',
      currentStateHash: 'state',
      pathBefore: {
        scope: 'user' as const,
        enabled: false,
        path: null,
        runtimeId: null,
        entries: [],
        hash: 'hash',
        updatedAt: null,
      },
      pathAfter: {
        scope: 'user' as const,
        enabled: false,
        path: null,
        runtimeId: null,
        entries: [],
        hash: 'hash',
        updatedAt: null,
      },
      confirmationTokenHash: 'sha256-token',
      confirmedBy: 'test-user',
      appliedOperationId: null,
    };

    await store.save(plan);
    const restored = await store.get(plan.planId);

    expect(restored).toEqual(plan);
    expect(text.values.get('plans.json')).not.toContain('plaintext');
  });

  it('provides a process-local plan store for isolated tests', async () => {
    const store = new InMemoryPlanStore();
    await expect(store.get('missing')).resolves.toBeNull();
  });

  it('round-trips the existing JSON stores through the opt-in Node SQLite adapter', async () => {
    const tempRoot = await mkdtemp(join(process.env.TEMP ?? process.env.TMP ?? '.', 'cxszn-menv-sqlite-'));
    const databasePath = join(tempRoot, 'nested', 'state.sqlite');
    const text = new NodeSqliteJsonTextStore(databasePath);
    const stateStore = new JsonStateStore('state.json', text);
    const state = emptyRuntimeState(() => new Date('2026-10-11T00:00:00.000Z'));

    try {
      await expect(text.read('missing.json')).resolves.toBeNull();
      await stateStore.save(state);

      const { DatabaseSync } = await import('node:sqlite');
      const database = new DatabaseSync(databasePath, { timeout: 5_000 });
      try {
        expect(database.prepare('PRAGMA journal_mode').get()).toMatchObject({ journal_mode: 'wal' });
        expect(database.prepare('PRAGMA synchronous').get()).toMatchObject({ synchronous: 2 });
        expect(database.prepare('SELECT version FROM menv_migrations').all()).toEqual([{ version: 1 }]);
        expect(database.prepare("SELECT value FROM menv_meta WHERE key = 'schema_version'").get()).toEqual({ value: '1' });
      } finally {
        database.close();
      }

      const reopenedText = new NodeSqliteJsonTextStore(databasePath);
      try {
        await expect(new JsonStateStore('state.json', reopenedText).load()).resolves.toEqual(state);
        await reopenedText.write('state.json', '{"updated":true}');
        await expect(reopenedText.read('state.json')).resolves.toBe('{"updated":true}');
      } finally {
        reopenedText.close();
      }
    } finally {
      text.close();
      await rm(tempRoot, { recursive: true, force: true });
    }
  });

  it('keeps malformed SQLite payloads fail-closed through the existing JSON adapter', async () => {
    const tempRoot = await mkdtemp(join(process.env.TEMP ?? process.env.TMP ?? '.', 'cxszn-menv-sqlite-'));
    const text = new NodeSqliteJsonTextStore(join(tempRoot, 'state.sqlite'));
    try {
      await text.write('state.json', '{"schemaVersion":"unknown","runtimes":[],"pathLease":{}}');
      await expect(new JsonStateStore('state.json', text).load()).rejects.toMatchObject({ code: 'PERSISTENCE_ERROR', retryable: false });
    } finally {
      text.close();
      await rm(tempRoot, { recursive: true, force: true });
    }
  });
});
