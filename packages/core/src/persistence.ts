import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { dirname } from 'node:path';
import type { PathLeaseState, PathLeaseStore, PlanStore, RuntimeState, StateStore, StoredChangePlan } from './types.js';
import { SCHEMA_VERSION } from './types.js';
import { createPathLeaseState, normalizePathEntries } from './path-lease.js';
import { CoreError } from './errors.js';

type SqliteDatabase = import('node:sqlite').DatabaseSync;
type SqliteTextRow = {
  content?: unknown;
};
type SqliteMetaRow = {
  value?: unknown;
};
type SqliteMigrationRow = {
  version?: unknown;
};

export function emptyRuntimeState(now: () => Date = () => new Date()): RuntimeState {
  const timestamp = now().toISOString();
  return {
    schemaVersion: SCHEMA_VERSION,
    runtimes: [],
    pathLease: createPathLeaseState(null, [], false, null, null),
    operations: [],
    updatedAt: timestamp,
  };
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

export class InMemoryStateStore implements StateStore {
  private state: RuntimeState | null;

  public constructor(initialState: RuntimeState | null = null) {
    this.state = initialState ? clone(initialState) : null;
  }

  public async load(): Promise<RuntimeState | null> {
    return this.state ? clone(this.state) : null;
  }

  public async save(state: RuntimeState): Promise<void> {
    this.state = clone(state);
  }
}

export interface JsonTextStore {
  read(path: string): Promise<string | null>;
  write(path: string, content: string): Promise<void>;
}

export class NodeJsonTextStore implements JsonTextStore {
  public async read(path: string): Promise<string | null> {
    try {
      return await readFile(path, 'utf8');
    } catch (error) {
      if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') return null;
      throw error;
    }
  }

  public async write(path: string, content: string): Promise<void> {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content, { encoding: 'utf8' });
  }
}

/**
 * Optional SQLite-backed implementation of the existing JSON text contract.
 *
 * A single database can back JsonStateStore, JsonPathLeaseStore, and
 * JsonPlanStore by using each adapter's logical path as the key. The default
 * constructors continue to use ordinary files, so importing core does not
 * require node:sqlite and existing JSON persistence remains unchanged.
 */
export class NodeSqliteJsonTextStore implements JsonTextStore {
  private database: SqliteDatabase | null = null;
  private opening: Promise<SqliteDatabase> | null = null;

  public constructor(
    private readonly databasePath: string,
    busyTimeoutMs = 5_000,
  ) {
    if (!Number.isSafeInteger(busyTimeoutMs) || busyTimeoutMs < 0 || busyTimeoutMs > 2_147_483_647) {
      throw new RangeError('busyTimeoutMs must be a non-negative 32-bit integer.');
    }
    this.busyTimeoutMs = busyTimeoutMs;
  }

  private readonly busyTimeoutMs: number;

  public async read(path: string): Promise<string | null> {
    const database = await this.openDatabase();
    try {
      const row = database.prepare('SELECT content FROM menv_json_store WHERE store_key = ?').get(path) as SqliteTextRow | undefined;
      if (!row) return null;
      if (typeof row.content !== 'string') throw new Error('Malformed SQLite text row.');
      return row.content;
    } catch (error) {
      if (error instanceof CoreError) throw error;
      throw new CoreError('PERSISTENCE_ERROR', `Unable to read SQLite text store: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
  }

  public async write(path: string, content: string): Promise<void> {
    const database = await this.openDatabase();
    try {
      database.exec('BEGIN IMMEDIATE');
      database
        .prepare(
          `INSERT INTO menv_json_store (store_key, content)
           VALUES (?, ?)
           ON CONFLICT(store_key) DO UPDATE SET
             content = excluded.content,
             updated_at = CURRENT_TIMESTAMP`,
        )
        .run(path, content);
      database.exec('COMMIT');
    } catch (error) {
      try {
        database.exec('ROLLBACK');
      } catch {
        // Preserve the original write failure for the caller.
      }
      throw new CoreError('PERSISTENCE_ERROR', `Unable to write SQLite text store: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
  }

  /** Close the opt-in adapter when its owning process is shutting down. */
  public close(): void {
    if (!this.database) return;
    this.database.close();
    this.database = null;
  }

  private async openDatabase(): Promise<SqliteDatabase> {
    if (this.database) return this.database;
    if (!this.opening) this.opening = this.initializeDatabase();
    const pending = this.opening;
    try {
      return await pending;
    } finally {
      if (this.opening === pending) this.opening = null;
    }
  }

  private async initializeDatabase(): Promise<SqliteDatabase> {
    let DatabaseSync: typeof import('node:sqlite').DatabaseSync;
    try {
      ({ DatabaseSync } = await import('node:sqlite'));
    } catch (error) {
      throw new CoreError(
        'PERSISTENCE_ERROR',
        `SQLite persistence requires a Node runtime exposing node:sqlite: ${error instanceof Error ? error.message : String(error)}`,
        { nextActions: ['Use Node.js 22.5 or newer, or keep the JSON adapter.'] },
      );
    }

    let database: SqliteDatabase | null = null;
    try {
      await mkdir(dirname(this.databasePath), { recursive: true });
      database = new DatabaseSync(this.databasePath, { timeout: this.busyTimeoutMs });
      database.exec(`
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = FULL;
        PRAGMA busy_timeout = ${this.busyTimeoutMs};
      `);
      database.exec('BEGIN IMMEDIATE');
      database.exec(`
        CREATE TABLE IF NOT EXISTS menv_migrations (
          version INTEGER PRIMARY KEY,
          applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) STRICT;
        CREATE TABLE IF NOT EXISTS menv_meta (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        ) STRICT;
        CREATE TABLE IF NOT EXISTS menv_json_store (
          store_key TEXT PRIMARY KEY,
          content TEXT NOT NULL,
          updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) STRICT;
        INSERT OR IGNORE INTO menv_migrations (version) VALUES (1);
      `);
      const latestMigration = database.prepare('SELECT MAX(version) AS version FROM menv_migrations').get() as SqliteMigrationRow | undefined;
      if (typeof latestMigration?.version === 'number' && latestMigration.version > 1) {
        throw new CoreError('PERSISTENCE_ERROR', `Unsupported SQLite schema migration ${latestMigration.version}.`, {
          nextActions: ['Run a migration-aware build before opening this database.'],
        });
      }
      const existingMeta = database.prepare("SELECT value FROM menv_meta WHERE key = 'schema_version'").get() as SqliteMetaRow | undefined;
      if (existingMeta && existingMeta.value !== SCHEMA_VERSION) {
        throw new CoreError('PERSISTENCE_ERROR', `Unsupported SQLite schema version ${String(existingMeta.value)}.`, {
          nextActions: ['Run a migration-aware build before opening this database.'],
        });
      }
      database
        .prepare(
          `INSERT INTO menv_meta (key, value)
           VALUES ('schema_version', ?)
           ON CONFLICT(key) DO NOTHING`,
        )
        .run(SCHEMA_VERSION);
      database.exec('COMMIT');
    } catch (error) {
      try {
        database?.exec('ROLLBACK');
      } catch {
        // Preserve the original open/migration failure.
      }
      try {
        database?.close();
      } catch {
        // Preserve the original open/migration failure.
      }
      if (error instanceof CoreError) throw error;
      throw new CoreError('PERSISTENCE_ERROR', `Unable to open SQLite text store: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
    if (!database) throw new CoreError('PERSISTENCE_ERROR', 'Unable to open SQLite text store.');
    this.database = database;
    return database;
  }
}

function parseRuntimeState(content: string): RuntimeState {
  const parsed = JSON.parse(content) as RuntimeState;
  if (parsed.schemaVersion !== SCHEMA_VERSION || !Array.isArray(parsed.runtimes) || !parsed.pathLease) {
    throw new Error('Unsupported or malformed state schema.');
  }
  return clone(parsed);
}

export class JsonStateStore implements StateStore {
  private readonly textStore: JsonTextStore;

  public constructor(
    private readonly path: string,
    textStore: JsonTextStore = new NodeJsonTextStore(),
  ) {
    this.textStore = textStore;
  }

  public async load(): Promise<RuntimeState | null> {
    let content: string | null;
    try {
      content = await this.textStore.read(this.path);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to read state: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
    if (content === null || content.trim() === '') return null;
    try {
      return parseRuntimeState(content);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to parse state: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  public async save(state: RuntimeState): Promise<void> {
    try {
      await this.textStore.write(this.path, `${JSON.stringify(state, null, 2)}\n`);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to write state: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
  }
}

/** Durable user-scope lease adapter. It stores only the app-owned lease view;
 * unrelated PATH entries remain in `entries` and are round-tripped unchanged. */
export class JsonPathLeaseStore implements PathLeaseStore {
  private readonly textStore: JsonTextStore;

  public constructor(
    private readonly path: string,
    textStore: JsonTextStore = new NodeJsonTextStore(),
  ) {
    this.textStore = textStore;
  }

  public async read(): Promise<PathLeaseState> {
    let content: string | null;
    try {
      content = await this.textStore.read(this.path);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to read PATH lease: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
    if (content === null || content.trim() === '') return createPathLeaseState(null, [], false, null, null);
    try {
      const parsed = JSON.parse(content) as PathLeaseState;
      if (!parsed || parsed.scope !== 'user' || !Array.isArray(parsed.entries) || typeof parsed.hash !== 'string') {
        throw new Error('Malformed PATH lease state.');
      }
      return {
        ...parsed,
        entries: normalizePathEntries(parsed.entries),
      };
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to parse PATH lease: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  public async write(next: PathLeaseState, expectedHash: string): Promise<PathLeaseState> {
    const current = await this.read();
    if (current.hash !== expectedHash) {
      throw new CoreError('PATH_CONFLICT', 'The user PATH changed while the plan was pending.', {
        retryable: true,
        nextActions: ['Recreate the plan from the current environment state.'],
      });
    }
    const normalized = createPathLeaseState(next.path, next.entries, next.enabled, next.runtimeId, next.updatedAt);
    try {
      await this.textStore.write(this.path, `${JSON.stringify(normalized, null, 2)}\n`);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to write PATH lease: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
    return normalized;
  }
}

export class InMemoryPlanStore implements PlanStore {
  private readonly plans = new Map<string, StoredChangePlan>();

  public constructor(initialPlans: StoredChangePlan[] = []) {
    for (const plan of initialPlans) this.plans.set(plan.planId, clone(plan));
  }

  public async get(planId: string): Promise<StoredChangePlan | null> {
    const plan = this.plans.get(planId);
    return plan ? clone(plan) : null;
  }

  public async save(plan: StoredChangePlan): Promise<void> {
    this.plans.set(plan.planId, clone(plan));
  }
}

/**
 * Durable plan storage for CLI/MCP processes. Only confirmation hashes are
 * stored; the one-time plaintext token is returned to the caller and never
 * written to disk.
 */
export class JsonPlanStore implements PlanStore {
  private readonly textStore: JsonTextStore;

  public constructor(
    private readonly path: string,
    textStore: JsonTextStore = new NodeJsonTextStore(),
  ) {
    this.textStore = textStore;
  }

  public async get(planId: string): Promise<StoredChangePlan | null> {
    const plans = await this.readAll();
    const plan = plans[planId];
    return plan ? clone(plan) : null;
  }

  public async save(plan: StoredChangePlan): Promise<void> {
    const plans = await this.readAll();
    plans[plan.planId] = clone(plan);
    try {
      await this.textStore.write(this.path, `${JSON.stringify(plans, null, 2)}\n`);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to write plans: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
  }

  private async readAll(): Promise<Record<string, StoredChangePlan>> {
    let content: string | null;
    try {
      content = await this.textStore.read(this.path);
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to read plans: ${error instanceof Error ? error.message : String(error)}`, {
        retryable: true,
      });
    }
    if (content === null || content.trim() === '') return {};
    try {
      const parsed = JSON.parse(content) as Record<string, StoredChangePlan>;
      return parsed && typeof parsed === 'object' ? parsed : {};
    } catch (error) {
      throw new CoreError('PERSISTENCE_ERROR', `Unable to parse plans: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
}

export function clonePathLease(pathLease: PathLeaseState): PathLeaseState {
  return clone(pathLease);
}
