import type {
  ActivityEntry,
  EnvironmentSnapshot,
  RuntimeRecord,
  RuntimeService,
} from '../types/runtime';

const now = () => new Date().toISOString();

const initialRuntimes: RuntimeRecord[] = [
  {
    id: 'php-8.3-x64-nts',
    family: 'php',
    name: 'PHP',
    version: '8.3.14',
    availability: 'managed',
    activation: 'active',
    health: 'healthy',
    operation: 'idle',
    architecture: 'x64',
    path: '%LOCALAPPDATA%\\cxszn-menv\\versions\\php\\8.3.14\\x64\\php.exe',
    source: 'managed',
    lastCheckedAt: '刚刚',
    detail: 'NTS · 已通过 php -n -v 检查',
    ts: 'nts',
  },
  {
    id: 'php-8.2-x64-nts',
    family: 'php',
    name: 'PHP',
    version: '8.2.26',
    availability: 'managed',
    activation: 'disabled',
    health: 'healthy',
    operation: 'idle',
    architecture: 'x64',
    path: '%LOCALAPPDATA%\\cxszn-menv\\versions\\php\\8.2.26\\x64\\php.exe',
    source: 'managed',
    lastCheckedAt: '今天 09:41',
    detail: 'NTS · 可切换',
    ts: 'nts',
  },
  {
    id: 'php-7.4-x64-ts',
    family: 'php',
    name: 'PHP',
    version: '7.4.33',
    availability: 'detected-unmanaged',
    activation: 'disabled',
    health: 'warning',
    operation: 'idle',
    architecture: 'x64',
    path: 'D:\\DevTools\\php-7.4.33\\php.exe',
    source: 'detected-unmanaged',
    lastCheckedAt: '今天 09:41',
    detail: 'TS · 外部路径 · EOL',
    eol: true,
    ts: 'ts',
  },
  {
    id: 'python-3.12-x64',
    family: 'python',
    name: 'Python',
    version: '3.12.7',
    availability: 'detected-unmanaged',
    activation: 'disabled',
    health: 'warning',
    operation: 'idle',
    architecture: 'x64',
    path: 'C:\\Users\\Developer\\AppData\\Local\\Programs\\Python\\Python312\\python.exe',
    source: 'detected-unmanaged',
    lastCheckedAt: '今天 09:41',
    detail: '外部安装 · 仅当前环境可用',
  },
];

const initialActivities: ActivityEntry[] = [
  {
    id: 'activity-1',
    occurredAt: '刚刚',
    title: 'PHP 8.3 已启用',
    detail: '用户级 shim 已指向 PHP 8.3.14 · 新终端生效',
    tone: 'success',
  },
  {
    id: 'activity-2',
    occurredAt: '今天 09:41',
    title: '扫描发现 4 个运行时',
    detail: '检测仅读，未接管外部安装',
    tone: 'neutral',
  },
  {
    id: 'activity-3',
    occurredAt: '昨天 18:26',
    title: 'PHP 7.4 标记为 legacy',
    detail: '该版本已停止上游安全维护',
    tone: 'warning',
  },
];

export function createMockRuntimeService(): RuntimeService {
  const plans = new Map<string, string>();
  let snapshot: EnvironmentSnapshot = {
    platform: 'windows',
    platformLabel: 'Windows 11 · 当前用户',
    pathLease: 'active',
    runtimes: structuredClone(initialRuntimes),
    activities: structuredClone(initialActivities),
    lastScanAt: '今天 09:41',
  };

  return {
    async getSnapshot() {
      return structuredClone(snapshot);
    },

    async scan() {
      const checkedAt = now();
      snapshot = {
        ...snapshot,
        lastScanAt: checkedAt,
        runtimes: snapshot.runtimes.map((runtime) => ({ ...runtime, lastCheckedAt: '刚刚' })),
        activities: [
          {
            id: `activity-${Date.now()}`,
            occurredAt: '刚刚',
            title: '环境扫描完成',
            detail: `发现 ${snapshot.runtimes.length} 个候选运行时 · 未修改系统配置`,
            tone: 'success',
          },
          ...snapshot.activities,
        ],
      };
      return structuredClone(snapshot);
    },

    async planEnable(runtimeId) {
      const runtime = snapshot.runtimes.find((candidate) => candidate.id === runtimeId);
      if (!runtime) {
        throw new Error('RUNTIME_NOT_FOUND');
      }
      const planId = `plan-${Date.now()}`;
      plans.set(planId, runtime.id);
      return { planId, runtime: structuredClone(runtime) };
    },

    async applyEnable(planId) {
      const targetId = plans.get(planId);
      if (!targetId) {
        throw new Error('PLAN_REQUIRED');
      }
      plans.delete(planId);
      const target = snapshot.runtimes.find((runtime) => runtime.id === targetId);
      if (!target) {
        return structuredClone(snapshot);
      }
      snapshot = {
        ...snapshot,
        pathLease: 'active',
        runtimes: snapshot.runtimes.map((runtime) => ({
          ...runtime,
          activation: runtime.id === target.id ? 'active' : runtime.family === target.family ? 'disabled' : runtime.activation,
        })),
        activities: [
          {
            id: `activity-${Date.now()}`,
            occurredAt: '刚刚',
            title: `${target.name} ${target.version} 已启用`,
            detail: '应用级 shim 已更新 · 已有终端不会热切换',
            tone: 'success',
          },
          ...snapshot.activities,
        ],
      };
      return structuredClone(snapshot);
    },
  };
}
