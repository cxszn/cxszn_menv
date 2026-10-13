export type RuntimeFamily = 'php' | 'node' | 'mysql' | 'python';

export type RuntimeAvailability =
  | 'not-installed'
  | 'detected-unmanaged'
  | 'manual'
  | 'managed'
  | 'invalid-path'
  | 'unsupported';

export type ActivationState = 'disabled' | 'enabling' | 'active' | 'disabling' | 'failed';
export type HealthState = 'unknown' | 'healthy' | 'warning' | 'error';
export type OperationState = 'idle' | 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';

export interface RuntimeRecord {
  id: string;
  family: RuntimeFamily;
  name: string;
  version: string;
  availability: RuntimeAvailability;
  activation: ActivationState;
  health: HealthState;
  operation: OperationState;
  architecture: 'x64' | 'x86' | 'arm64' | 'unknown';
  path: string | null;
  source: 'managed' | 'manual' | 'detected-unmanaged';
  lastCheckedAt: string;
  detail: string;
  eol?: boolean;
  ts?: 'ts' | 'nts';
}

export interface ActivityEntry {
  id: string;
  occurredAt: string;
  title: string;
  detail: string;
  tone: 'neutral' | 'success' | 'warning' | 'error';
}

export interface EnvironmentSnapshot {
  platform: 'windows';
  platformLabel: string;
  pathLease: 'active' | 'inactive' | 'conflict';
  runtimes: RuntimeRecord[];
  activities: ActivityEntry[];
  lastScanAt: string | null;
}

/**
 * A catalog entry is metadata only until the user confirms an install plan.
 * Keeping this separate from RuntimeRecord prevents a downloadable artifact
 * from being shown as installed or enabled before the native service reports
 * it in the environment snapshot.
 */
export interface RuntimeCatalogArtifact {
  artifactId: string;
  family: RuntimeFamily;
  version: string;
  architecture: 'x64' | 'x86' | 'arm64' | 'unknown';
  threadSafety: 'ts' | 'nts' | 'not-applicable' | 'unknown';
  lifecycle: 'supported' | 'eol' | 'archival' | 'revoked';
  legacy: boolean;
  archival: boolean;
  available: boolean;
  installed: boolean;
  fileName?: string;
  sizeBytes?: number;
}

export interface RuntimeInstallPlan {
  planId: string;
  artifact: RuntimeCatalogArtifact;
}

export interface RuntimeRegistrationPlan {
  planId: string;
  runtime: RuntimeRecord;
}

export interface TerminalEvent {
  kind: 'output' | 'closed' | 'exited';
  text?: string;
  code?: string;
}

export interface TerminalSession {
  sessionId: string;
  cwd: string;
}

export interface PhpIniResult {
  runtimeId: string;
  path: string | null;
  content: string | null;
  truncated: boolean;
}

export interface PhpIniBackupInfo {
  runtimeId: string;
  sourcePath: string;
  backupPath: string;
}

export interface PhpServerInfo {
  serverId: string;
  runtimeId: string;
  docroot: string;
  endpoint: string;
  logPath: string;
}

export interface WebServerDiagnostic {
  server: string;
  detected: boolean;
  configPaths: string[];
  phpReferences: string[];
  warnings: string[];
}

export interface RuntimeService {
  getSnapshot(): Promise<EnvironmentSnapshot>;
  scan(): Promise<EnvironmentSnapshot>;
  planEnable(runtimeId: string): Promise<{ planId: string; runtime: RuntimeRecord }>;
  applyEnable(planId: string): Promise<EnvironmentSnapshot>;
  /** Read verified catalog metadata; this never downloads or mutates files. */
  listPhpCatalog(): Promise<RuntimeCatalogArtifact[]>;
  /** Create an install plan. Applying it still requires explicit UI confirmation. */
  planPhpInstall(artifactId: string): Promise<RuntimeInstallPlan>;
  /** Recover unexpired plans persisted before a desktop restart. */
  listPhpInstallPlans(): Promise<RuntimeInstallPlan[]>;
  /** Apply a previously confirmed install plan and return the refreshed state. */
  applyPhpInstall(planId: string): Promise<EnvironmentSnapshot>;
  /** Cancel a persisted install plan before installation starts. */
  cancelPhpInstall(planId: string): Promise<void>;
  /** Verify and manually register an external PHP path after confirmation. */
  planPhpRegister(path: string): Promise<RuntimeRegistrationPlan>;
  applyPhpRegister(planId: string): Promise<EnvironmentSnapshot>;
  planPhpIniBackup(runtimeId: string): Promise<{ planId: string; runtime: RuntimeRecord; sourcePath: string; sourceSha256: string }>;
  applyPhpIniBackup(planId: string): Promise<PhpIniBackupInfo>;
  /** Start the fixed PowerShell terminal session. */
  startTerminal(cwd?: string): Promise<TerminalSession>;
  /** Send literal terminal input; the native adapter enforces its byte limit. */
  sendTerminalInput(sessionId: string, input: string): Promise<void>;
  /** Poll bounded terminal events without exposing arbitrary process execution. */
  pollTerminal(sessionId: string): Promise<TerminalEvent[]>;
  /** Close a terminal session and its child process tree. */
  closeTerminal(sessionId: string): Promise<void>;
  readPhpIni(runtimeId: string): Promise<PhpIniResult>;
  startPhpServer(input: { runtimeId: string; docroot: string; host: string; port: number }): Promise<PhpServerInfo>;
  stopPhpServer(serverId: string): Promise<void>;
  diagnoseWebServers(): Promise<WebServerDiagnostic[]>;
}

export const familyLabels: Record<RuntimeFamily, string> = {
  php: 'PHP',
  node: 'Node.js',
  mysql: 'MySQL',
  python: 'Python',
};

export const availabilityLabels: Record<RuntimeAvailability, string> = {
  'not-installed': '未安装',
  'detected-unmanaged': '已检测 · 未托管',
  manual: '手动登记',
  managed: '已托管',
  'invalid-path': '路径无效',
  unsupported: '不支持',
};

export const activationLabels: Record<ActivationState, string> = {
  disabled: '未启用',
  enabling: '启用中',
  active: '当前启用',
  disabling: '停用中',
  failed: '启用失败',
};

export const healthLabels: Record<HealthState, string> = {
  unknown: '未检查',
  healthy: '健康',
  warning: '需检查',
  error: '异常',
};
