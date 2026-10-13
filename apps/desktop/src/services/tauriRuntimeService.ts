import { invoke } from '@tauri-apps/api/core';
import type {
  ActivityEntry,
  EnvironmentSnapshot,
  RuntimeCatalogArtifact,
  RuntimeInstallPlan,
  RuntimeRegistrationPlan,
  RuntimeRecord,
  RuntimeService,
  PhpIniResult,
  PhpIniBackupInfo,
  PhpServerInfo,
  WebServerDiagnostic,
  TerminalEvent,
  TerminalSession,
} from '../types/runtime';

type Envelope<T> = {
  schema_version: string;
  request_id: string;
  ok: boolean;
  result?: T;
  error?: { code: string; message: string; retryable: boolean; next_actions: string[] };
};

/** Error returned by the native application service, including its stable
 * error code and recovery hints. The UI should never have to guess why a
 * PATH/shim operation failed from a generic string alone. */
export class TauriRuntimeError extends Error {
  readonly code: string;
  readonly retryable: boolean;
  readonly nextActions: string[];

  constructor(payload: { code: string; message: string; retryable: boolean; next_actions: string[] }) {
    super(payload.message);
    this.name = 'TauriRuntimeError';
    this.code = payload.code;
    this.retryable = payload.retryable;
    this.nextActions = payload.next_actions;
  }
}

type RawRuntime = {
  id: string;
  family: 'php' | 'node' | 'mysql' | 'python';
  version: string | null;
  executablePath: string;
  rootPath: string;
  architecture: 'x86' | 'x64' | 'arm64' | 'unknown';
  threadSafety: 'ts' | 'nts' | 'unknown';
  ownership: 'detected-unmanaged' | 'manual' | 'managed';
  availability: RuntimeRecord['availability'];
  activation: RuntimeRecord['activation'];
  health: RuntimeRecord['health'];
  ini: { source: 'unknown' | 'none' | 'explicit'; path: string | null };
  legacy: boolean;
  diagnostics: string[];
  lastCheckedAt: string;
};

type RawOperation = {
  operationId: string;
  action: 'register' | 'install' | 'enable' | 'disable' | 'switch' | 'backup-ini';
  runtimeId: string;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  updatedAt: string;
};

type RawSnapshot = {
  runtimes: RawRuntime[];
  pathLease: { enabled: boolean; runtimeId: string | null };
  operations: RawOperation[];
  updatedAt: string;
};

type RawPlan = {
  planId: string;
  planHash: string;
  request: { actor: string };
  expiresAtEpochSeconds: number;
  requiresConfirmation: boolean;
  backupSource?: { path: string; sha256: string };
};

type RawRegisterPlanResult = { plan: RawPlan; runtime: RawRuntime };

type RawToken = {
  value: string;
  planId: string;
  planHash: string;
  actor: string;
  expiresAtEpochSeconds: number;
};

type RawCatalogArtifact = {
  artifactId?: string;
  artifact_id?: string;
  family?: RuntimeCatalogArtifact['family'];
  runtimeFamily?: RuntimeCatalogArtifact['family'];
  runtime_family?: RuntimeCatalogArtifact['family'];
  version: string;
  architecture?: RuntimeCatalogArtifact['architecture'];
  arch?: 'x86_64' | 'x86' | 'arm64';
  threadSafety?: RuntimeCatalogArtifact['threadSafety'];
  thread_safety?: 'ts' | 'nts' | 'not_applicable';
  lifecycle: RuntimeCatalogArtifact['lifecycle'];
  legacy?: boolean;
  archival?: boolean;
  available?: boolean;
  installed?: boolean;
  fileName?: string;
  file_name?: string;
  sizeBytes?: number;
  size_bytes?: number;
};

type RawCatalogResult = { artifacts?: RawCatalogArtifact[] } | RawCatalogArtifact[];
type RawRecoveredInstallPlan = { plan: RawPlan; artifact: RawCatalogArtifact };

type RawTerminalStart = { sessionId: string; cwd: string };
type RawTerminalEvent = { kind: TerminalEvent['kind']; text?: string; code?: string };
type RawPhpIniResult = { runtimeId: string; path: string | null; content: string | null; truncated: boolean };
type RawPhpIniBackupInfo = { runtimeId: string; sourcePath: string; backupPath: string };
type RawPhpServerInfo = { serverId: string; runtimeId: string; docroot: string; endpoint: string; logPath: string };
type RawWebServerDiagnostic = { server: string; detected: boolean; configPaths: string[]; phpReferences: string[]; warnings: string[] };

function mapArchitecture(value: RawCatalogArtifact['architecture'] | RawCatalogArtifact['arch']): RuntimeCatalogArtifact['architecture'] {
  if (value === 'x86_64') return 'x64';
  if (value === 'x64' || value === 'x86' || value === 'arm64') return value;
  return 'unknown';
}

function mapThreadSafety(value: RawCatalogArtifact['threadSafety'] | RawCatalogArtifact['thread_safety']): RuntimeCatalogArtifact['threadSafety'] {
  if (value === 'not_applicable') return 'not-applicable';
  if (value === 'ts' || value === 'nts') return value;
  return 'unknown';
}

function mapCatalogArtifact(raw: RawCatalogArtifact): RuntimeCatalogArtifact {
  const artifactId = raw.artifactId ?? raw.artifact_id;
  const family = raw.family ?? raw.runtimeFamily ?? raw.runtime_family;
  if (!artifactId || !family) throw new Error('CATALOG_INVALID: catalog artifact is missing id or family');
  return {
    artifactId,
    family,
    version: raw.version,
    architecture: mapArchitecture(raw.architecture ?? raw.arch),
    threadSafety: mapThreadSafety(raw.threadSafety ?? raw.thread_safety),
    lifecycle: raw.lifecycle,
    legacy: raw.legacy ?? (raw.lifecycle === 'eol' || raw.lifecycle === 'archival'),
    archival: raw.archival ?? (raw.lifecycle === 'archival' || raw.lifecycle === 'revoked'),
    available: raw.available ?? raw.lifecycle !== 'revoked',
    installed: raw.installed ?? false,
    ...(raw.fileName ?? raw.file_name ? { fileName: raw.fileName ?? raw.file_name } : {}),
    ...(raw.sizeBytes ?? raw.size_bytes ? { sizeBytes: raw.sizeBytes ?? raw.size_bytes } : {}),
  };
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const envelope = await invoke<Envelope<T>>(command, args);
  if (!envelope.ok || envelope.result === undefined) {
    if (envelope.error) throw new TauriRuntimeError(envelope.error);
    throw new Error(`Tauri command failed: ${command}`);
  }
  return envelope.result;
}

function activityForOperation(operation: RawOperation): ActivityEntry {
  const action = operation.action === 'register'
    ? '登记'
    : operation.action === 'install'
      ? '安装'
    : operation.action === 'backup-ini'
      ? '备份配置'
    : operation.action === 'enable' ? '启用' : operation.action === 'disable' ? '停用' : '切换';
  const tone = operation.status === 'succeeded' ? 'success' : operation.status === 'failed' ? 'error' : 'neutral';
  return {
    id: operation.operationId,
    occurredAt: operation.updatedAt,
    title: `${action} ${operation.runtimeId}`,
    detail: `核心操作状态：${operation.status}`,
    tone,
  };
}

export function mapEnvironmentSnapshot(raw: RawSnapshot): EnvironmentSnapshot {
  const operationsByRuntime = new Map(raw.operations.map((operation) => [operation.runtimeId, operation]));
  const runtimes: RuntimeRecord[] = raw.runtimes.map((runtime) => {
    const operation = operationsByRuntime.get(runtime.id);
    const path = runtime.executablePath || null;
    const ini = runtime.ini.path ? ` · ini ${runtime.ini.path}` : '';
    const diagnostic = runtime.diagnostics[0] ? ` · ${runtime.diagnostics[0]}` : '';
    const detail = runtime.family === 'php'
      ? `${runtime.threadSafety.toUpperCase()}${ini}${diagnostic}`
      : `${runtime.architecture}${diagnostic}`;
    return {
      id: runtime.id,
      family: runtime.family,
      name: runtime.family === 'php' ? 'PHP' : runtime.family,
      version: runtime.version ?? '未知版本',
      availability: runtime.availability,
      activation: runtime.activation,
      health: runtime.health,
      operation: operation?.status ?? 'idle',
      architecture: runtime.architecture,
      path,
      source: runtime.ownership,
      lastCheckedAt: runtime.lastCheckedAt,
      detail,
      eol: runtime.legacy,
      ...(runtime.threadSafety === 'ts' || runtime.threadSafety === 'nts' ? { ts: runtime.threadSafety } : {}),
    };
  });
  const activities = raw.operations.slice().reverse().map(activityForOperation);
  return {
    platform: 'windows',
    platformLabel: 'Windows 11 · 当前用户',
    pathLease: raw.pathLease.enabled ? 'active' : 'inactive',
    runtimes,
    activities,
    lastScanAt: raw.updatedAt || null,
  };
}

export function createTauriRuntimeService(): RuntimeService {
  const plans = new Map<string, RawPlan>();
  const installPlans = new Map<string, { artifact: RuntimeCatalogArtifact; plan: RawPlan }>();
  const registerPlans = new Map<string, RawPlan>();

  return {
    async getSnapshot() {
      return mapEnvironmentSnapshot(await call<RawSnapshot>('get_environment_state'));
    },

    async scan() {
      return mapEnvironmentSnapshot(await call<RawSnapshot>('scan_installed_runtimes'));
    },

    async planEnable(runtimeId) {
      const snapshot = await this.getSnapshot();
      const runtime = snapshot.runtimes.find((candidate) => candidate.id === runtimeId);
      if (!runtime) throw new Error('RUNTIME_NOT_FOUND');
      const plan = await call<RawPlan>('plan_environment_change', {
        request: {
          action: 'enable',
          family: runtime.family,
          runtimeId: runtime.id,
          targetRuntimeId: null,
          idempotencyKey: `desktop-${crypto.randomUUID()}`,
          actor: 'desktop-ui',
        },
      });
      plans.set(plan.planId, plan);
      return { planId: plan.planId, runtime };
    },

    async applyEnable(planId) {
      const plan = plans.get(planId);
      if (!plan) throw new Error('PLAN_REQUIRED');
      const token = await call<RawToken>('issue_confirmation_token', { plan });
      await call('apply_environment_change', { plan, confirmationToken: token });
      plans.delete(planId);
      return this.getSnapshot();
    },

    async listPhpCatalog() {
      const result = await call<RawCatalogResult>('list_php_catalog');
      const artifacts = Array.isArray(result) ? result : result.artifacts ?? [];
      const mapped = artifacts.map(mapCatalogArtifact);
      // The native catalog loader intentionally reports metadata only. Merge
      // Match the actual build, not just its version: TS and NTS packages
      // must remain independently selectable.
      const snapshot = await this.getSnapshot();
      return mapped.map((artifact) => ({
        ...artifact,
        installed: artifact.installed || snapshot.runtimes.some((runtime) => (
          runtime.family === artifact.family
          && runtime.version === artifact.version
          && (artifact.architecture === 'unknown' || runtime.architecture === artifact.architecture)
          && (artifact.family !== 'php' || runtime.ts === artifact.threadSafety)
        )),
      }));
    },

    async planPhpInstall(artifactId) {
      const catalog = await this.listPhpCatalog();
      const artifact = catalog.find((candidate) => candidate.artifactId === artifactId);
      if (!artifact) throw new Error('CATALOG_ARTIFACT_NOT_FOUND');
      if (!artifact.available || artifact.lifecycle === 'revoked') throw new Error('CATALOG_ARTIFACT_UNAVAILABLE');
      const plan = await call<RawPlan>('plan_php_install', {
        artifactId,
        actor: 'desktop-ui',
        idempotencyKey: `desktop-install-${crypto.randomUUID()}`,
      });
      installPlans.set(plan.planId, { artifact, plan });
      return { planId: plan.planId, artifact };
    },

    async listPhpInstallPlans() {
      const recovered = await call<RawRecoveredInstallPlan[]>('list_php_install_plans');
      return recovered.map(({ plan, artifact: rawArtifact }) => {
        const artifact = mapCatalogArtifact(rawArtifact);
        installPlans.set(plan.planId, { artifact, plan });
        return { planId: plan.planId, artifact };
      });
    },

    async applyPhpInstall(planId) {
      const entry = installPlans.get(planId);
      if (!entry) throw new Error('PLAN_REQUIRED');
      const token = await call<RawToken>('issue_confirmation_token', { plan: entry.plan });
      await call('apply_php_install', { plan: entry.plan, confirmationToken: token });
      installPlans.delete(planId);
      return this.getSnapshot();
    },

    async cancelPhpInstall(planId) {
      await call('cancel_php_install', { planId });
      installPlans.delete(planId);
    },

    async planPhpRegister(path): Promise<RuntimeRegistrationPlan> {
      const result = await call<RawRegisterPlanResult>('plan_php_path_registration', {
        path,
        actor: 'desktop-ui',
        idempotencyKey: `desktop-register-${crypto.randomUUID()}`,
      });
      registerPlans.set(result.plan.planId, result.plan);
      const runtime = mapEnvironmentSnapshot({
        runtimes: [result.runtime],
        pathLease: { enabled: false, runtimeId: null },
        operations: [],
        updatedAt: '',
      }).runtimes[0];
      if (!runtime) throw new Error('RUNTIME_INVALID');
      return { planId: result.plan.planId, runtime };
    },

    async applyPhpRegister(planId) {
      const plan = registerPlans.get(planId);
      if (!plan) throw new Error('PLAN_REQUIRED');
      const token = await call<RawToken>('issue_confirmation_token', { plan });
      await call('apply_php_path_registration', { plan, confirmationToken: token });
      registerPlans.delete(planId);
      return this.getSnapshot();
    },

    async planPhpIniBackup(runtimeId) {
      const snapshot = await this.getSnapshot();
      const runtime = snapshot.runtimes.find((candidate) => candidate.id === runtimeId);
      if (!runtime) throw new Error('RUNTIME_NOT_FOUND');
      const plan = await call<RawPlan>('plan_environment_change', {
        request: {
          action: 'backup-ini',
          family: runtime.family,
          runtimeId: runtime.id,
          targetRuntimeId: null,
          idempotencyKey: `desktop-ini-backup-${crypto.randomUUID()}`,
          actor: 'desktop-ui',
        },
      });
      if (!plan.backupSource?.path || !/^[0-9a-f]{64}$/i.test(plan.backupSource.sha256)) {
        throw new Error('备份计划缺少原文件校验信息，请更新应用并重新生成计划。');
      }
      plans.set(plan.planId, plan);
      return { planId: plan.planId, runtime, sourcePath: plan.backupSource.path, sourceSha256: plan.backupSource.sha256 };
    },

    async applyPhpIniBackup(planId): Promise<PhpIniBackupInfo> {
      const plan = plans.get(planId);
      if (!plan) throw new Error('PLAN_REQUIRED');
      const token = await call<RawToken>('issue_confirmation_token', { plan });
      const info = await call<RawPhpIniBackupInfo>('apply_php_ini_backup', {
        plan,
        confirmationToken: token,
      });
      plans.delete(planId);
      await this.getSnapshot();
      return info;
    },

    async startTerminal(cwd?: string): Promise<TerminalSession> {
      return call<RawTerminalStart>('terminal_start', {
        request: { cwd: cwd ?? null },
      });
    },

    async sendTerminalInput(sessionId: string, input: string): Promise<void> {
      await call('terminal_send_input', { sessionId, input });
    },

    async pollTerminal(sessionId: string): Promise<TerminalEvent[]> {
      return call<RawTerminalEvent[]>('terminal_poll', { sessionId });
    },

    async closeTerminal(sessionId: string): Promise<void> {
      await call('terminal_close', { sessionId });
    },

    async readPhpIni(runtimeId: string): Promise<PhpIniResult> {
      return call<RawPhpIniResult>('read_php_ini', { runtimeId });
    },

    async startPhpServer(input): Promise<PhpServerInfo> {
      return call<RawPhpServerInfo>('php_server_start', { request: input });
    },

    async stopPhpServer(serverId: string): Promise<void> {
      await call('php_server_stop', { serverId });
    },

    async diagnoseWebServers(): Promise<WebServerDiagnostic[]> {
      return call<RawWebServerDiagnostic[]>('diagnose_web_servers');
    },
  };
}

export { isTauriRuntime };
