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

export interface RuntimeService {
  getSnapshot(): Promise<EnvironmentSnapshot>;
  scan(): Promise<EnvironmentSnapshot>;
  planEnable(runtimeId: string): Promise<{ planId: string; runtime: RuntimeRecord }>;
  applyEnable(planId: string): Promise<EnvironmentSnapshot>;
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
