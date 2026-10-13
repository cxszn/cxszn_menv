export const SCHEMA_VERSION = '1' as const;

export type RuntimeFamily = 'php' | 'node' | 'mysql' | 'python';
export type RuntimeAvailability =
  | 'not-installed'
  | 'detected-unmanaged'
  | 'manual'
  | 'managed'
  | 'invalid-path'
  | 'unsupported';
export type RuntimeActivation = 'disabled' | 'enabling' | 'active' | 'disabling' | 'failed';
export type RuntimeHealth = 'unknown' | 'healthy' | 'warning' | 'error';
export type RuntimeOwnership = 'detected-unmanaged' | 'manual' | 'managed';
export type RuntimeArchitecture = 'x64' | 'x86' | 'arm64' | 'unknown';
export type PhpThreadSafety = 'ts' | 'nts' | 'unknown';
export type OperationStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
export type EnvironmentScope = 'process' | 'user';
export type EnvironmentChangeAction = 'enable' | 'disable' | 'switch' | 'runtime.add';
export type OperationAction = EnvironmentChangeAction | 'register' | 'install';
export type EnvironmentChangeRequestAction = EnvironmentChangeAction | 'runtime.enable' | 'runtime.disable' | 'register_path';

export interface RuntimeIniInfo {
  source: 'unknown' | 'none' | 'explicit';
  path: string | null;
}

export interface RuntimeRecord {
  id: string;
  family: RuntimeFamily;
  version: string | null;
  executablePath: string;
  rootPath: string;
  architecture: RuntimeArchitecture;
  threadSafety: PhpThreadSafety;
  availability: RuntimeAvailability;
  ownership: RuntimeOwnership;
  activation: RuntimeActivation;
  health: RuntimeHealth;
  ini: RuntimeIniInfo;
  legacy: boolean;
  diagnostics: string[];
  firstSeenAt: string;
  lastCheckedAt: string;
}

export interface PathLeaseState {
  scope: 'user';
  enabled: boolean;
  path: string | null;
  runtimeId: string | null;
  entries: string[];
  hash: string;
  updatedAt: string | null;
}

export interface OperationRecord {
  operationId: string;
  planId: string;
  idempotencyKey: string;
  action: OperationAction;
  runtimeId: string;
  status: OperationStatus;
  createdAt: string;
  updatedAt: string;
  error?: SerializedCoreError;
}

export interface SerializedCoreError {
  code: string;
  message: string;
  retryable: boolean;
  nextActions: string[];
  details?: Record<string, unknown>;
}

export interface RuntimeState {
  schemaVersion: typeof SCHEMA_VERSION;
  runtimes: RuntimeRecord[];
  pathLease: PathLeaseState;
  operations: OperationRecord[];
  updatedAt: string;
}

export interface RuntimeCatalogEntry {
  family: RuntimeFamily;
  version: string;
  architecture: RuntimeArchitecture;
  threadSafety?: PhpThreadSafety;
  url?: string;
  sha256?: string;
  sizeBytes?: number;
  legacy?: boolean;
  available: boolean;
}

export interface EnvironmentChangeInput {
  action: EnvironmentChangeRequestAction;
  runtimeId?: string;
  path?: string;
  targetRuntimeId?: string;
  scope?: EnvironmentScope;
  actor?: string;
}

export interface ChangePlan {
  schemaVersion: typeof SCHEMA_VERSION;
  planId: string;
  planHash: string;
  action: EnvironmentChangeAction;
  runtimeId: string;
  targetRuntimeId: string | null;
  scope: EnvironmentScope;
  requiresConfirmation: true;
  createdAt: string;
  expiresAt: string;
  currentStateHash: string;
  pathBefore: PathLeaseState;
  pathAfter: PathLeaseState;
  runtimePath?: string;
  runtimeSnapshot?: RuntimeRecord;
}

export interface StoredChangePlan extends ChangePlan {
  confirmationTokenHash: string | null;
  confirmedBy: string | null;
  appliedOperationId: string | null;
}

export interface ApplyEnvironmentChangeInput {
  planId: string;
  planHash?: string;
  confirmationToken?: string;
  idempotencyKey: string;
  actor?: string;
}

export interface ProcessProbeResult {
  exitCode: number;
  stdout: string;
  stderr: string;
}

export interface ProcessProbe {
  run(executablePath: string, args: string[], options?: { timeoutMs?: number }): Promise<ProcessProbeResult>;
}

export interface FileSystemProbe {
  exists(path: string): boolean;
  isFile(path: string): boolean;
  realPath(path: string): string;
  isReparsePoint(path: string): boolean;
}

export interface StateStore {
  load(): Promise<RuntimeState | null>;
  save(state: RuntimeState): Promise<void>;
}

export interface PathLeaseStore {
  read(): Promise<PathLeaseState>;
  write(next: PathLeaseState, expectedHash: string): Promise<PathLeaseState>;
}

export interface CoreServiceOptions {
  stateStore?: StateStore;
  pathLeaseStore?: PathLeaseStore;
  planStore?: PlanStore;
  fileSystem?: FileSystemProbe;
  processProbe?: ProcessProbe;
  scannerPaths?: string[];
  appBinPath?: string;
  catalog?: RuntimeCatalogEntry[];
  now?: () => Date;
  planTtlMs?: number;
}

export interface PlanStore {
  get(planId: string): Promise<StoredChangePlan | null>;
  save(plan: StoredChangePlan): Promise<void>;
}
