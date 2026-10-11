import { execFile } from 'node:child_process';
import { existsSync, lstatSync, realpathSync } from 'node:fs';
import { delimiter as pathDelimiter } from 'node:path';
import { promisify } from 'node:util';
import { CoreError, asCoreError } from './errors.js';
import { hashJson, newId, normalizePathEntry, normalizeWindowsPath, sha256, stableStringify } from './hash.js';
import {
  appLeasePlan,
  InMemoryPathLeaseStore,
  normalizePathEntries,
  pathLeaseSnapshotForState,
  withAppOwnedLease,
} from './path-lease.js';
import {
  emptyRuntimeState,
  InMemoryPlanStore,
  JsonPathLeaseStore,
  JsonPlanStore,
  JsonStateStore,
} from './persistence.js';
import { probePhpExecutable } from './php.js';
import type {
  ApplyEnvironmentChangeInput,
  ChangePlan,
  CoreServiceOptions,
  EnvironmentChangeInput,
  EnvironmentChangeAction,
  EnvironmentChangeRequestAction,
  EnvironmentScope,
  FileSystemProbe,
  OperationRecord,
  PathLeaseState,
  PlanStore,
  ProcessProbe,
  RuntimeCatalogEntry,
  RuntimeRecord,
  RuntimeState,
  StoredChangePlan,
  StateStore,
} from './types.js';

const execFileAsync = promisify(execFile);

const defaultFileSystem: FileSystemProbe = {
  exists: (path) => existsSync(path),
  isFile: (path) => {
    try {
      return lstatSync(path).isFile();
    } catch {
      return false;
    }
  },
  realPath: (path) => {
    try {
      return realpathSync.native(path);
    } catch {
      return path;
    }
  },
  isReparsePoint: (path) => {
    try {
      return lstatSync(path).isSymbolicLink();
    } catch {
      return false;
    }
  },
};

const defaultProcessProbe: ProcessProbe = {
  async run(executablePath, args, options) {
    try {
      const result = await execFileAsync(executablePath, args, {
        encoding: 'utf8',
        windowsHide: true,
        maxBuffer: 1024 * 1024,
        timeout: options?.timeoutMs ?? 5_000,
      });
      return { exitCode: 0, stdout: String(result.stdout), stderr: String(result.stderr) };
    } catch (error) {
      const candidate = error as { code?: unknown; stdout?: unknown; stderr?: unknown; message?: unknown };
      const exitCode = typeof candidate.code === 'number' ? candidate.code : 1;
      return {
        exitCode,
        stdout: typeof candidate.stdout === 'string' ? candidate.stdout : '',
        stderr: typeof candidate.stderr === 'string' ? candidate.stderr : String(candidate.message ?? error),
      };
    }
  },
};

function defaultScannerPaths(dataRoot: string): string[] {
  const candidates = [
    ...(process.env.PATH ?? '').split(pathDelimiter),
    `${dataRoot}\\versions\\php`,
    process.env.LOCALAPPDATA ? `${process.env.LOCALAPPDATA}\\Programs` : '',
    process.env.ProgramFiles ? `${process.env.ProgramFiles}\\PHP` : '',
    process.env['ProgramFiles(x86)'] ? `${process.env['ProgramFiles(x86)']}\\PHP` : '',
    process.env.USERPROFILE ? `${process.env.USERPROFILE}\\scoop\\apps\\php\\current` : '',
  ];
  const seen = new Set<string>();
  return candidates.filter((candidate) => {
    const trimmed = candidate.trim();
    if (!trimmed) return false;
    const key = normalizePathEntry(trimmed);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

type InternalPlan = StoredChangePlan;

function nowIso(now: () => Date): string {
  return now().toISOString();
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function joinWindowsPath(directory: string, file: string): string {
  return `${directory.replace(/[\\/]+$/, '')}\\${file}`;
}

function resolvePhpCandidate(path: string, fileSystem: FileSystemProbe): string {
  return fileSystem.isFile(path) ? path : joinWindowsPath(path, 'php.exe');
}

function normalizeAction(action: string): EnvironmentChangeAction {
  if (action === 'runtime.enable') return 'enable';
  if (action === 'runtime.disable') return 'disable';
  if (action === 'register_path') return 'runtime.add';
  if (action === 'runtime.add' || action === 'enable' || action === 'disable' || action === 'switch') return action;
  throw new CoreError('INVALID_INPUT', `Unsupported environment change action: ${action}.`);
}

function stateHash(state: RuntimeState): string {
  return hashJson({ runtimes: state.runtimes, pathLease: state.pathLease });
}

function pathLeaseForPlan(
  before: PathLeaseState,
  action: 'enable' | 'disable' | 'switch' | 'runtime.add',
  scope: EnvironmentScope,
  appBinPath: string,
  runtimeId: string,
  now: () => Date,
): PathLeaseState {
  if (scope !== 'user' || action === 'runtime.add') return clone(before);
  if (action === 'enable' || action === 'switch') return withAppOwnedLease(before, appBinPath, runtimeId, now);
  return appLeasePlan(before, 'disable', appBinPath, runtimeId);
}

function updateRuntimeActivation(
  runtimes: RuntimeRecord[],
  action: 'enable' | 'disable' | 'switch',
  runtimeId: string,
  targetRuntimeId: string | null,
): RuntimeRecord[] {
  return runtimes.map((runtime) => {
    if (action === 'switch') {
      if (runtime.id === targetRuntimeId) return { ...runtime, activation: 'active' };
      if (runtime.activation === 'active') return { ...runtime, activation: 'disabled' };
      return runtime;
    }
    if (runtime.id !== runtimeId) return runtime;
    return { ...runtime, activation: action === 'enable' ? 'active' : 'disabled' };
  });
}

function upsertRuntime(runtimes: RuntimeRecord[], next: RuntimeRecord): RuntimeRecord[] {
  const index = runtimes.findIndex((runtime) => runtime.id === next.id);
  if (index === -1) return [...runtimes, next];
  const copy = [...runtimes];
  copy[index] = next;
  return copy;
}

export class CoreService {
  private readonly stateStore: StateStore;
  private readonly pathLeaseStore;
  private readonly fileSystem: FileSystemProbe;
  private readonly processProbe: ProcessProbe;
  private readonly scannerPaths: string[];
  private readonly appBinPath: string;
  private readonly catalog: RuntimeCatalogEntry[];
  private readonly now: () => Date;
  private readonly planTtlMs: number;
  private readonly planStore: PlanStore;
  private readonly idempotency = new Map<string, OperationRecord>();

  public constructor(options: CoreServiceOptions = {}) {
    const dataRoot = process.env.LOCALAPPDATA
      ? `${process.env.LOCALAPPDATA}\\cxszn-menv`
      : 'C:\\Users\\Default\\AppData\\Local\\cxszn-menv';
    this.stateStore = options.stateStore ?? new JsonStateStore(`${dataRoot}\\state.json`);
    this.pathLeaseStore = options.pathLeaseStore ?? (options.stateStore ? new InMemoryPathLeaseStore() : new JsonPathLeaseStore(`${dataRoot}\\path-lease.json`));
    this.planStore = options.planStore ?? (options.stateStore ? new InMemoryPlanStore() : new JsonPlanStore(`${dataRoot}\\plans.json`));
    this.fileSystem = options.fileSystem ?? defaultFileSystem;
    this.processProbe = options.processProbe ?? defaultProcessProbe;
    this.scannerPaths = options.scannerPaths ?? defaultScannerPaths(dataRoot);
    const defaultAppBinPath = process.env.LOCALAPPDATA
      ? `${process.env.LOCALAPPDATA}\\cxszn-menv\\bin`
      : 'C:\\Users\\Default\\AppData\\Local\\cxszn-menv\\bin';
    this.appBinPath = normalizeWindowsPath(options.appBinPath ?? defaultAppBinPath);
    this.catalog = options.catalog ? clone(options.catalog) : [];
    this.now = options.now ?? (() => new Date());
    this.planTtlMs = options.planTtlMs ?? 10 * 60 * 1000;
  }

  public async getEnvironmentState(_input?: unknown): Promise<RuntimeState> {
    const state = await this.loadState();
    return clone(state);
  }

  public async scanInstalledRuntimes(input?: unknown): Promise<RuntimeState> {
    const requestedFamily = typeof input === 'object' && input !== null
      ? (input as { family?: unknown; runtimeFamily?: unknown }).family
        ?? (input as { family?: unknown; runtimeFamily?: unknown }).runtimeFamily
      : undefined;
    if (requestedFamily !== undefined && requestedFamily !== 'php') {
      throw new CoreError('UNSUPPORTED_RUNTIME', `Runtime family ${String(requestedFamily)} is not implemented in this release.`);
    }
    const state = await this.loadState();
    const discovered: RuntimeRecord[] = [];
    const seen = new Set<string>();
    for (const configuredPath of this.scannerPaths) {
      const candidate = resolvePhpCandidate(configuredPath, this.fileSystem);
      const key = normalizePathEntry(candidate);
      if (seen.has(key)) continue;
      seen.add(key);
      if (!this.fileSystem.exists(candidate)) continue;
      try {
        const probed = await probePhpExecutable(candidate, this.fileSystem, this.processProbe, this.now);
        const previous = state.runtimes.find((runtime) => normalizePathEntry(runtime.executablePath) === normalizePathEntry(probed.executablePath));
        discovered.push(
          previous && previous.ownership !== 'detected-unmanaged'
            ? { ...probed, ownership: previous.ownership, availability: previous.availability, activation: previous.activation, firstSeenAt: previous.firstSeenAt }
            : probed,
        );
      } catch (error) {
        const coreError = asCoreError(error, 'PROBE_FAILED');
        discovered.push({
          id: `php_${sha256(normalizeWindowsPath(candidate).toLocaleLowerCase('en-US')).slice(0, 24)}`,
          family: 'php',
          version: null,
          executablePath: normalizeWindowsPath(candidate),
          rootPath: normalizeWindowsPath(candidate).replace(/[\\/][^\\/]+$/, ''),
          architecture: 'unknown',
          threadSafety: 'unknown',
          availability: 'invalid-path',
          ownership: 'detected-unmanaged',
          activation: 'disabled',
          health: 'error',
          ini: { source: 'unknown', path: null },
          legacy: false,
          diagnostics: [coreError.message],
          firstSeenAt: nowIso(this.now),
          lastCheckedAt: nowIso(this.now),
        });
      }
    }

    const discoveredIds = new Set(discovered.map((runtime) => runtime.id));
    const retained = state.runtimes.filter((runtime) => runtime.ownership !== 'detected-unmanaged' && !discoveredIds.has(runtime.id));
    state.runtimes = [...retained, ...discovered];
    state.pathLease = await this.readPathLease();
    await this.saveState(state);
    return clone(state);
  }

  public async verifyRuntimePath(path: string, family?: string): Promise<RuntimeRecord> {
    if (family !== undefined && family !== 'php') {
      throw new CoreError('UNSUPPORTED_RUNTIME', `Runtime family ${family} is not implemented in this release.`);
    }
    return probePhpExecutable(resolvePhpCandidate(path, this.fileSystem), this.fileSystem, this.processProbe, this.now);
  }

  public async listRuntimeCatalog(kind?: string | { runtimeFamily?: string }): Promise<RuntimeCatalogEntry[]> {
    const family = typeof kind === 'string' ? kind : kind?.runtimeFamily;
    return clone(family ? this.catalog.filter((entry) => entry.family === family) : this.catalog);
  }

  public async planEnvironmentChange(input: EnvironmentChangeInput | Record<string, unknown>): Promise<ChangePlan> {
    if (!input || typeof input !== 'object') throw new CoreError('INVALID_INPUT', 'A change request is required.');
    const request = input as EnvironmentChangeInput;
    const action = normalizeAction(String(request.action ?? ''));
    // User-scope activation is the product default. A caller may explicitly
    // request process scope for a session-owned environment.
    const scope: EnvironmentScope = request.scope ?? 'user';
    const state = await this.loadState();
    let runtimeId = request.runtimeId;
    let runtimeSnapshot: RuntimeRecord | undefined;
    let runtimePath: string | undefined;

    if (action === 'runtime.add') {
      if (!request.path) throw new CoreError('INVALID_INPUT', 'runtime.add requires a path.');
      const probed = await probePhpExecutable(resolvePhpCandidate(request.path, this.fileSystem), this.fileSystem, this.processProbe, this.now);
      runtimeId = probed.id;
      runtimeSnapshot = { ...probed, availability: 'manual', ownership: 'manual', activation: 'disabled' };
      runtimePath = probed.executablePath;
    } else if (request.path) {
      throw new CoreError('INVALID_INPUT', 'Register a path with runtime.add before enabling, disabling, or switching it.');
    }

    if (!runtimeId) throw new CoreError('INVALID_INPUT', 'A runtimeId or verified path is required.');
    const runtime = state.runtimes.find((candidate) => candidate.id === runtimeId);
    if (action !== 'runtime.add' && !runtime && !runtimeSnapshot) {
      throw new CoreError('RUNTIME_NOT_FOUND', `Runtime ${runtimeId} was not found.`);
    }
    if (action === 'switch' && !request.targetRuntimeId) {
      throw new CoreError('INVALID_INPUT', 'switch requires targetRuntimeId.');
    }
    if (action === 'switch' && !state.runtimes.some((candidate) => candidate.id === request.targetRuntimeId)) {
      throw new CoreError('RUNTIME_NOT_FOUND', `Target runtime ${request.targetRuntimeId} was not found.`);
    }

    const before = await this.readPathLease();
    const targetId = action === 'switch' ? request.targetRuntimeId ?? runtimeId : runtimeId;
    const after = pathLeaseForPlan(before, action, scope, this.appBinPath, targetId, this.now);
    const createdAt = this.now();
    const planId = newId('plan');
    const expiresAt = new Date(createdAt.getTime() + this.planTtlMs).toISOString();
    const hashable = {
      schemaVersion: '1' as const,
      action,
      runtimeId,
      targetRuntimeId: request.targetRuntimeId ?? null,
      scope,
      createdAt: createdAt.toISOString(),
      expiresAt,
      currentStateHash: stateHash({ ...state, pathLease: before }),
      pathBefore: before,
      pathAfter: after,
      runtimePath: runtimePath ?? null,
      runtimeSnapshot: runtimeSnapshot ?? null,
    };
    const plan: InternalPlan = {
      schemaVersion: '1',
      planId,
      planHash: sha256(stableStringify(hashable)),
      action,
      runtimeId,
      targetRuntimeId: request.targetRuntimeId ?? null,
      scope,
      createdAt: createdAt.toISOString(),
      expiresAt,
      currentStateHash: hashable.currentStateHash,
      pathBefore: before,
      pathAfter: after,
      requiresConfirmation: true,
      confirmationTokenHash: null,
      confirmedBy: null,
      appliedOperationId: null,
      ...(runtimePath === undefined ? {} : { runtimePath }),
      ...(runtimeSnapshot === undefined ? {} : { runtimeSnapshot }),
    };
    await this.planStore.save(plan);
    return clone(plan);
  }

  public async confirmPlan(planId: string, actor = 'user'): Promise<string> {
    const plan = await this.planStore.get(planId);
    if (!plan) throw new CoreError('PLAN_NOT_FOUND', `Plan ${planId} was not found.`);
    if (new Date(plan.expiresAt).getTime() <= this.now().getTime()) {
      throw new CoreError('PLAN_EXPIRED', 'The change plan has expired.', { retryable: true });
    }
    const token = `${plan.planId}.${newId('confirm')}`;
    plan.confirmationTokenHash = sha256(token);
    plan.confirmedBy = actor;
    await this.planStore.save(plan);
    return token;
  }

  public async applyEnvironmentChange(input: ApplyEnvironmentChangeInput): Promise<OperationRecord> {
    if (!input.idempotencyKey || input.idempotencyKey.trim() === '') {
      throw new CoreError('IDEMPOTENCY_REQUIRED', 'A non-empty idempotencyKey is required.');
    }
    const existing = this.idempotency.get(input.idempotencyKey);
    if (existing) {
      if (existing.planId !== input.planId) throw new CoreError('IDEMPOTENCY_CONFLICT', 'The idempotency key was used for another plan.');
      return clone(existing);
    }
    const plan = await this.planStore.get(input.planId);
    if (!plan) throw new CoreError('PLAN_NOT_FOUND', `Plan ${input.planId} was not found.`);
    if (new Date(plan.expiresAt).getTime() <= this.now().getTime()) throw new CoreError('PLAN_EXPIRED', 'The change plan has expired.', { retryable: true });
    if (input.planHash && input.planHash !== plan.planHash) throw new CoreError('PLAN_STALE', 'The plan hash does not match the current plan.');
    if (!input.confirmationToken) throw new CoreError('CONFIRMATION_REQUIRED', 'A user confirmation token is required.', { nextActions: ['Confirm the plan, then retry apply.'] });
    if (!plan.confirmationTokenHash || sha256(input.confirmationToken) !== plan.confirmationTokenHash) {
      throw new CoreError('CONFIRMATION_INVALID', 'The confirmation token is invalid or has expired.');
    }
    if (input.actor && plan.confirmedBy && input.actor !== plan.confirmedBy) throw new CoreError('CONFIRMATION_INVALID', 'The confirmation actor does not match the plan confirmer.');

    const state = await this.loadState();
    const persistedExisting = state.operations.find((candidate) => candidate.idempotencyKey === input.idempotencyKey);
    if (persistedExisting) {
      if (persistedExisting.planId !== input.planId) throw new CoreError('IDEMPOTENCY_CONFLICT', 'The idempotency key was used for another plan.');
      this.idempotency.set(input.idempotencyKey, clone(persistedExisting));
      return clone(persistedExisting);
    }
    const currentPath = await this.readPathLease();
    const currentHash = stateHash({ ...state, pathLease: currentPath });
    if (currentHash !== plan.currentStateHash) throw new CoreError('PLAN_STALE', 'The environment changed after the plan was created.', { retryable: true });

    const operationId = newId('op');
    const startedAt = nowIso(this.now);
    const operation: OperationRecord = {
      operationId,
      planId: plan.planId,
      idempotencyKey: input.idempotencyKey,
      action: plan.action,
      runtimeId: plan.runtimeId,
      status: 'running',
      createdAt: startedAt,
      updatedAt: startedAt,
    };

    try {
      if (plan.action === 'runtime.add') {
        if (!plan.runtimePath) throw new CoreError('INVALID_INPUT', 'The runtime.add plan has no executable path.');
        const verified = await probePhpExecutable(resolvePhpCandidate(plan.runtimePath, this.fileSystem), this.fileSystem, this.processProbe, this.now);
        const manual = { ...verified, availability: verified.availability === 'unsupported' ? 'unsupported' : 'manual', ownership: 'manual', activation: 'disabled' } as RuntimeRecord;
        state.runtimes = upsertRuntime(state.runtimes, manual);
      } else {
        const runtime = state.runtimes.find((candidate) => candidate.id === plan.runtimeId);
        if (!runtime) throw new CoreError('RUNTIME_NOT_FOUND', `Runtime ${plan.runtimeId} was not found.`);
        if (plan.scope === 'user') await this.pathLeaseStore.write(plan.pathAfter, plan.pathBefore.hash);
        state.runtimes = updateRuntimeActivation(state.runtimes, plan.action, plan.runtimeId, plan.targetRuntimeId);
      }
      state.pathLease = await this.readPathLease();
      operation.status = 'succeeded';
      operation.updatedAt = nowIso(this.now);
      state.operations = [...state.operations, operation];
      await this.saveState(state);
      this.idempotency.set(input.idempotencyKey, clone(operation));
      plan.appliedOperationId = operationId;
      await this.planStore.save(plan);
      return clone(operation);
    } catch (error) {
      if (plan.scope === 'user' && plan.action !== 'runtime.add') {
        try {
          const current = await this.pathLeaseStore.read();
          if (current.hash !== plan.pathBefore.hash) await this.pathLeaseStore.write(plan.pathBefore, current.hash);
        } catch {
          // Preserve the original failure; the next doctor run can expose a lease conflict.
        }
      }
      const coreError = asCoreError(error);
      operation.status = 'failed';
      operation.updatedAt = nowIso(this.now);
      operation.error = coreError.toJSON();
      state.operations = [...state.operations, operation];
      await this.saveState(state);
      throw coreError;
    }
  }

  public async getOperationStatus(operationId: string): Promise<OperationRecord> {
    const state = await this.loadState();
    const operation = state.operations.find((candidate) => candidate.operationId === operationId) ?? [...this.idempotency.values()].find((candidate) => candidate.operationId === operationId);
    if (!operation) throw new CoreError('OPERATION_NOT_FOUND', `Operation ${operationId} was not found.`);
    return clone(operation);
  }

  public async cancelOperation(operationId: string): Promise<OperationRecord> {
    const state = await this.loadState();
    const operation = state.operations.find((candidate) => candidate.operationId === operationId);
    if (!operation) throw new CoreError('OPERATION_NOT_FOUND', `Operation ${operationId} was not found.`);
    if (operation.status !== 'queued' && operation.status !== 'running') throw new CoreError('OPERATION_NOT_CANCELLABLE', 'Only queued or running operations can be cancelled.');
    operation.status = 'cancelled';
    operation.updatedAt = nowIso(this.now);
    await this.saveState(state);
    return clone(operation);
  }

  private async readPathLease(): Promise<PathLeaseState> {
    const lease = await this.pathLeaseStore.read();
    return pathLeaseSnapshotForState(lease, lease.path ?? (lease.enabled ? this.appBinPath : null), this.now);
  }

  private async loadState(): Promise<RuntimeState> {
    const loaded = await this.stateStore.load();
    const state = loaded ? clone(loaded) : emptyRuntimeState(this.now);
    state.pathLease = await this.readPathLease();
    return state;
  }

  private async saveState(state: RuntimeState): Promise<void> {
    state.updatedAt = nowIso(this.now);
    await this.stateStore.save(state);
  }
}

export function createCoreService(options: CoreServiceOptions = {}): CoreService {
  return new CoreService(options);
}
