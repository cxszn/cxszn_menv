import type {
  ActivityEntry,
  EnvironmentSnapshot,
  RuntimeCatalogArtifact,
  RuntimeInstallPlan,
  RuntimeRegistrationPlan,
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

const initialPhpCatalog: RuntimeCatalogArtifact[] = [
  {
    artifactId: 'php-8.2.34-nts-x64',
    family: 'php',
    version: '8.2.34',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'supported',
    legacy: false,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-8.2.34-nts-Win32-vs16-x64.zip',
  },
  {
    artifactId: 'php-8.3.35-nts-x64',
    family: 'php',
    version: '8.3.35',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'supported',
    legacy: false,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-8.3.35-nts-Win32-vs16-x64.zip',
  },
  {
    artifactId: 'php-8.4.26-nts-x64',
    family: 'php',
    version: '8.4.26',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'supported',
    legacy: false,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-8.4.26-nts-Win32-vs17-x64.zip',
  },
  {
    artifactId: 'php-8.5.11-nts-x64',
    family: 'php',
    version: '8.5.11',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'supported',
    legacy: false,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-8.5.11-nts-Win32-vs17-x64.zip',
  },
  {
    artifactId: 'php-8.1.34-nts-x64',
    family: 'php',
    version: '8.1.34',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'eol',
    legacy: true,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-8.1.34-nts-Win32-vs16-x64.zip',
  },
  {
    artifactId: 'php-8.0.30-nts-x64',
    family: 'php',
    version: '8.0.30',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'eol',
    legacy: true,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-8.0.30-nts-Win32-vs16-x64.zip',
  },
  {
    artifactId: 'php-7.4.33-nts-x64',
    family: 'php',
    version: '7.4.33',
    architecture: 'x64',
    threadSafety: 'nts',
    lifecycle: 'eol',
    legacy: true,
    archival: false,
    available: true,
    installed: false,
    fileName: 'php-7.4.33-nts-Win32-vc15-x64.zip',
  },
];

export function createMockRuntimeService(): RuntimeService {
  const plans = new Map<string, string>();
  const installPlans = new Map<string, RuntimeCatalogArtifact>();
  const cancelledInstallPlans = new Set<string>();
  const completedInstallPlans = new Set<string>();
  const registerPlans = new Map<string, RuntimeRecord>();
  const iniBackupPlans = new Map<string, string>();
  let catalog = structuredClone(initialPhpCatalog);
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

    async listPhpCatalog() {
      return structuredClone(catalog.map((artifact) => ({
        ...artifact,
        installed: artifact.installed || snapshot.runtimes.some((runtime) => (
          runtime.family === artifact.family
          && runtime.version === artifact.version
          && runtime.architecture === artifact.architecture
        )),
      })));
    },

    async planPhpInstall(artifactId): Promise<RuntimeInstallPlan> {
      const artifact = catalog.find((candidate) => candidate.artifactId === artifactId);
      if (!artifact) throw new Error('CATALOG_ARTIFACT_NOT_FOUND');
      if (!artifact.available || artifact.lifecycle === 'revoked') throw new Error('CATALOG_ARTIFACT_UNAVAILABLE');
      const planId = `install-plan-${crypto.randomUUID()}`;
      installPlans.set(planId, artifact);
      return { planId, artifact: structuredClone(artifact) };
    },

    async listPhpInstallPlans(): Promise<RuntimeInstallPlan[]> {
      return Array.from(installPlans, ([planId, artifact]) => ({ planId, artifact: structuredClone(artifact) }));
    },

    async cancelPhpInstall(planId) {
      if (cancelledInstallPlans.has(planId)) return;
      if (completedInstallPlans.has(planId)) throw new Error('OPERATION_NOT_CANCELLABLE');
      if (!installPlans.has(planId)) throw new Error('PLAN_REQUIRED');
      installPlans.delete(planId);
      cancelledInstallPlans.add(planId);
    },

    async applyPhpInstall(planId) {
      if (cancelledInstallPlans.has(planId)) throw new Error('PLAN_CANCELLED');
      const artifact = installPlans.get(planId);
      if (!artifact) throw new Error('PLAN_REQUIRED');
      installPlans.delete(planId);
      const runtimeId = `${artifact.family}-${artifact.version}-x64-nts`;
      if (!snapshot.runtimes.some((runtime) => runtime.id === runtimeId)) {
        snapshot = {
          ...snapshot,
          runtimes: [
            ...snapshot.runtimes,
            {
              id: runtimeId,
              family: 'php',
              name: 'PHP',
              version: artifact.version,
              availability: 'managed',
              activation: 'disabled',
              health: 'healthy',
              operation: 'idle',
              architecture: 'x64',
              path: `%LOCALAPPDATA%\\cxszn-menv\\versions\\php\\${artifact.version}\\x64\\php.exe`,
              source: 'managed',
              lastCheckedAt: '刚刚',
              detail: `${artifact.threadSafety.toUpperCase()} · 已完成 SHA-256 校验`,
              ...(artifact.threadSafety === 'ts' || artifact.threadSafety === 'nts' ? { ts: artifact.threadSafety } : {}),
              eol: artifact.legacy,
            },
          ],
          activities: [
            {
              id: `activity-${Date.now()}`,
              occurredAt: '刚刚',
              title: `PHP ${artifact.version} 已下载`,
              detail: '已校验并登记为受管版本 · 默认未启用',
              tone: 'success',
            },
            ...snapshot.activities,
          ],
        };
      }
      catalog = catalog.map((candidate) => candidate.artifactId === artifact.artifactId
        ? { ...candidate, installed: true }
        : candidate);
      completedInstallPlans.add(planId);
      return structuredClone(snapshot);
    },

    async planPhpRegister(path): Promise<RuntimeRegistrationPlan> {
      if (!path.trim()) throw new Error('INVALID_RUNTIME_PATH');
      const planId = `register-plan-${Date.now()}`;
      const runtime: RuntimeRecord = {
        id: `php-manual-${Date.now()}`,
        family: 'php',
        name: 'PHP',
        version: 'unknown',
        availability: 'manual',
        activation: 'disabled',
        health: 'warning',
        operation: 'idle',
        architecture: 'unknown',
        path,
        source: 'manual',
        lastCheckedAt: '刚刚',
        detail: '手动路径 · 等待确认登记',
      };
      registerPlans.set(planId, runtime);
      return { planId, runtime: structuredClone(runtime) };
    },

    async applyPhpRegister(planId) {
      const runtime = registerPlans.get(planId);
      if (!runtime) throw new Error('PLAN_REQUIRED');
      registerPlans.delete(planId);
      if (!snapshot.runtimes.some((candidate) => candidate.path?.toLowerCase() === runtime.path?.toLowerCase())) {
        snapshot = {
          ...snapshot,
          runtimes: [...snapshot.runtimes, runtime],
          activities: [
            {
              id: `activity-${Date.now()}`,
              occurredAt: '刚刚',
              title: 'PHP 手动路径已登记',
              detail: '外部文件未复制、未覆盖、未启用',
              tone: 'success',
            },
            ...snapshot.activities,
          ],
        };
      }
      return structuredClone(snapshot);
    },

    async planPhpIniBackup(runtimeId) {
      const runtime = snapshot.runtimes.find((candidate) => candidate.id === runtimeId);
      if (!runtime) throw new Error('RUNTIME_NOT_FOUND');
      const ini = await this.readPhpIni(runtimeId);
      if (!ini.path || ini.content === null) throw new Error('PHP_INI_NOT_FOUND');
      const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(ini.content));
      const sourceSha256 = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
      const planId = `ini-backup-plan-${Date.now()}`;
      iniBackupPlans.set(planId, runtimeId);
      return { planId, runtime: structuredClone(runtime), sourcePath: ini.path, sourceSha256 };
    },

    async applyPhpIniBackup(planId) {
      const runtimeId = iniBackupPlans.get(planId);
      if (!runtimeId) throw new Error('PLAN_REQUIRED');
      iniBackupPlans.delete(planId);
      const runtime = snapshot.runtimes.find((candidate) => candidate.id === runtimeId);
      if (!runtime) throw new Error('RUNTIME_NOT_FOUND');
      return {
        runtimeId,
        sourcePath: runtime.path ?? '',
        backupPath: `%LOCALAPPDATA%\\cxszn-menv\\backups\\php-ini\\${planId}.php.ini.bak`,
      };
    },

    async startTerminal(cwd) {
      return { sessionId: `mock-terminal-${Date.now()}`, cwd: cwd ?? 'C:\\Projects\\demo' };
    },

    async sendTerminalInput() {
      // The mock terminal is intentionally display-only; the native service
      // owns process creation and input delivery.
    },

    async pollTerminal() {
      return [];
    },

    async closeTerminal() {
      // No process exists in the mock service.
    },

    async readPhpIni(runtimeId) {
      const runtime = snapshot.runtimes.find((candidate) => candidate.id === runtimeId);
      if (!runtime) throw new Error('RUNTIME_NOT_FOUND');
      return {
        runtimeId,
        path: null,
        content: null,
        truncated: false,
      };
    },

    async startPhpServer(input) {
      return {
        serverId: `mock-php-server-${Date.now()}`,
        runtimeId: input.runtimeId,
        docroot: input.docroot,
        endpoint: `${input.host}:${input.port}`,
        logPath: '%LOCALAPPDATA%\\cxszn-menv\\logs\\php-server\\mock.log',
      };
    },

    async stopPhpServer() {
      // No process exists in the mock service.
    },

    async diagnoseWebServers() {
      return [
        { server: 'IIS', detected: false, configPaths: [], phpReferences: [], warnings: [] },
        { server: 'Apache', detected: false, configPaths: [], phpReferences: [], warnings: [] },
        { server: 'Nginx', detected: false, configPaths: [], phpReferences: [], warnings: [] },
      ];
    },
  };
}
