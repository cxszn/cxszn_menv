<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import ActivityList from './components/ActivityList.vue';
import AppSidebar from './components/AppSidebar.vue';
import ConfirmDialog from './components/ConfirmDialog.vue';
import PhpCatalogDialog from './components/PhpCatalogDialog.vue';
import PhpInstallConfirmDialog from './components/PhpInstallConfirmDialog.vue';
import PhpIniBackupConfirmDialog from './components/PhpIniBackupConfirmDialog.vue';
import RuntimeRow from './components/RuntimeRow.vue';
import StatusPill from './components/StatusPill.vue';
import { createMockRuntimeService } from './services/mockRuntimeService';
import { createTauriRuntimeService, isTauriRuntime, TauriRuntimeError } from './services/tauriRuntimeService';
import { shouldScanOnStartup } from './services/startupScan';
import { validatePhpIni, type PhpIniValidationResult } from './services/phpIni';
import {
  activationLabels,
  availabilityLabels,
  familyLabels,
  type EnvironmentSnapshot,
  type PhpIniBackupInfo,
  type PhpServerInfo,
  type RuntimeCatalogArtifact,
  type RuntimeRecord,
} from './types/runtime';

const service = isTauriRuntime() ? createTauriRuntimeService() : createMockRuntimeService();
const activeView = ref('overview');
const filter = ref<'all' | 'php' | 'other'>('all');
const snapshot = ref<EnvironmentSnapshot | null>(null);
const selectedRuntimeId = ref<string | null>(null);
const isLoading = ref(true);
const isScanning = ref(false);
const isApplying = ref(false);
const isInstalling = ref(false);
const isCancellingInstall = ref(false);
const installError = ref('');
const errorMessage = ref('');
const confirmRuntime = ref<RuntimeRecord | null>(null);
const pendingPlanId = ref<string | null>(null);
const isCatalogOpen = ref(false);
const isCatalogLoading = ref(false);
const catalogError = ref('');
const catalogArtifacts = ref<RuntimeCatalogArtifact[]>([]);
const catalogBusyArtifactId = ref<string | null>(null);
const installArtifact = ref<RuntimeCatalogArtifact | null>(null);
const pendingInstallPlanId = ref<string | null>(null);
const installRecoveryChecked = ref(false);
const manualPath = ref('');
const manualRuntime = ref<RuntimeRecord | null>(null);
const pendingManualPlanId = ref<string | null>(null);
const manualPathBusy = ref(false);
const manualPathError = ref('');
const terminalSessionId = ref<string | null>(null);
const terminalCwd = ref('');
const terminalInput = ref('');
const terminalOutput = ref('');
const terminalStatus = ref<'idle' | 'starting' | 'running' | 'closed' | 'error'>('idle');
const terminalError = ref('');
let terminalPollTimer: ReturnType<typeof setInterval> | null = null;
const iniRuntimeId = ref<string | null>(null);
const iniPath = ref<string | null>(null);
const iniContent = ref<string | null>(null);
const iniLoading = ref(false);
const iniError = ref('');
const iniValidation = ref<PhpIniValidationResult | null>(null);
const iniBackupRuntime = ref<RuntimeRecord | null>(null);
const iniBackupPlanId = ref<string | null>(null);
const iniBackupSourcePath = ref('');
const iniBackupSourceSha256 = ref('');
const iniBackupBusy = ref(false);
const iniBackupInfo = ref<PhpIniBackupInfo | null>(null);
const iniBackupError = ref('');
const phpServerInfo = ref<PhpServerInfo | null>(null);
const phpServerDocroot = ref('C:\\Projects\\demo');
const phpServerPort = ref(8000);
const phpServerLoading = ref(false);
const phpServerError = ref('');
const webServerDiagnostics = ref<Awaited<ReturnType<typeof service.diagnoseWebServers>>>([]);
const webServerLoading = ref(false);
const webServerError = ref('');

const runtimes = computed(() => snapshot.value?.runtimes ?? []);
const selectedRuntime = computed(() => runtimes.value.find((runtime) => runtime.id === selectedRuntimeId.value) ?? runtimes.value[0] ?? null);
const activeRuntime = computed(() => runtimes.value.find((runtime) => runtime.activation === 'active') ?? null);
const managedCount = computed(() => runtimes.value.filter((runtime) => runtime.availability === 'managed').length);
const detectedCount = computed(() => runtimes.value.filter((runtime) => runtime.availability === 'detected-unmanaged').length);
const filteredRuntimes = computed(() => {
  if (filter.value === 'php') return runtimes.value.filter((runtime) => runtime.family === 'php');
  if (filter.value === 'other') return runtimes.value.filter((runtime) => runtime.family !== 'php');
  return runtimes.value;
});

const viewTitle = computed(() => {
  if (activeView.value === 'runtimes') return '运行时';
  if (activeView.value === 'terminal') return '终端';
  if (activeView.value === 'activity') return '活动日志';
  return '总览';
});

function describeError(error: unknown, fallback: string): string {
  if (error instanceof TauriRuntimeError) {
    if (error.code === 'PLATFORM_ERROR' && /share a volume|hard link/i.test(error.message)) {
      return `${fallback}：当前 PHP 与 MEnv shim 不在同一磁盘，Windows 硬链接无法创建。请更新到包含跨磁盘 shim 修复的版本后重试（${error.code}）。`;
    }
    const stableMessages: Record<string, string> = {
      DOWNLOAD_FAILED: '下载失败，请检查网络后重试',
      SIZE_MISMATCH: '下载文件大小与签名 catalog 不一致，已拒绝安装',
      HASH_MISMATCH: '下载文件 SHA-256 校验失败，已拒绝安装',
      ARCHIVE_REJECTED: '压缩包路径或内容不安全，已拒绝解压',
      ENTRYPOINT_MISSING: '压缩包中未找到有效的 php.exe',
      VERSION_EXISTS: '目标 PHP 版本已经存在，请刷新运行时列表',
      INSTALL_FILESYSTEM_ERROR: '安装目录写入失败，请检查磁盘空间和权限',
      CATALOG_REJECTED: '版本清单校验失败，未执行下载或安装',
      OPERATION_BUSY: '另一个 MEnv 操作正在执行，请稍后重试',
      OPERATION_NOT_CANCELLABLE: '该操作已开始或已结束，当前状态不能取消',
    };
    const message = stableMessages[error.code] ?? error.message;
    const hint = error.nextActions.length > 0 ? ` ${error.nextActions.join(' ')}` : '';
    return `${fallback}：${message}（${error.code}）。${hint}`;
  }
  if (error instanceof Error && error.message) return `${fallback}：${error.message}`;
  return fallback;
}

async function refresh() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    const loaded = await service.getSnapshot();
    if (shouldScanOnStartup(loaded)) {
      isScanning.value = true;
      snapshot.value = await service.scan();
    } else {
      snapshot.value = loaded;
    }
    selectedRuntimeId.value ??= snapshot.value.runtimes[0]?.id ?? null;
    if (!installRecoveryChecked.value) {
      installRecoveryChecked.value = true;
      try {
        const recovered = await service.listPhpInstallPlans();
        const pending = recovered[0];
        if (pending) {
          installArtifact.value = pending.artifact;
          pendingInstallPlanId.value = pending.planId;
        }
      } catch (error) {
        // A stale/corrupt recovery file must not hide the current environment
        // snapshot; the native command remains fail-closed and the user can
        // create a fresh plan from the catalog.
        errorMessage.value = describeError(error, '待恢复的 PHP 下载计划读取失败');
      }
    }
  } catch (error) {
    errorMessage.value = describeError(error, '暂时无法读取本地环境状态，请稍后重试');
  } finally {
    isScanning.value = false;
    isLoading.value = false;
  }
}

async function scanEnvironment() {
  isScanning.value = true;
  errorMessage.value = '';
  try {
    snapshot.value = await service.scan();
  } catch (error) {
    errorMessage.value = describeError(error, '环境扫描未完成，当前状态保持不变');
  } finally {
    isScanning.value = false;
  }
}

async function loadPhpIni(runtime: RuntimeRecord) {
  iniRuntimeId.value = runtime.id;
  iniLoading.value = true;
  iniError.value = '';
  try {
    const result = await service.readPhpIni(runtime.id);
    iniPath.value = result.path;
    iniContent.value = result.content;
    iniValidation.value = null;
    if (result.truncated) iniError.value = 'php.ini 超过 512 KiB，仅显示前 512 KiB。';
  } catch (error) {
    iniPath.value = null;
    iniContent.value = null;
    iniValidation.value = null;
    iniError.value = describeError(error, 'php.ini 读取失败');
  } finally {
    iniLoading.value = false;
  }
}

function validateLoadedPhpIni() {
  if (iniContent.value === null) return;
  iniValidation.value = validatePhpIni(iniContent.value, Boolean(iniError.value && /512 KiB/.test(iniError.value)));
}

async function requestPhpIniBackup(runtime: RuntimeRecord) {
  iniBackupError.value = '';
  try {
    const plan = await service.planPhpIniBackup(runtime.id);
    iniBackupRuntime.value = plan.runtime;
    iniBackupPlanId.value = plan.planId;
    iniBackupSourcePath.value = plan.sourcePath;
    iniBackupSourceSha256.value = plan.sourceSha256;
  } catch (error) {
    iniBackupError.value = describeError(error, '无法生成 php.ini 备份计划');
  }
}

async function applyPhpIniBackup() {
  if (!iniBackupPlanId.value) return;
  iniBackupBusy.value = true;
  iniBackupError.value = '';
  try {
    iniBackupInfo.value = await service.applyPhpIniBackup(iniBackupPlanId.value);
    iniBackupRuntime.value = null;
    iniBackupPlanId.value = null;
  } catch (error) {
    iniBackupError.value = describeError(error, 'php.ini 备份未完成');
  } finally {
    iniBackupBusy.value = false;
  }
}

function cancelPhpIniBackup() {
  if (iniBackupBusy.value) return;
  iniBackupRuntime.value = null;
  iniBackupPlanId.value = null;
}

async function startPhpServer() {
  if (!selectedRuntime.value || selectedRuntime.value.family !== 'php') return;
  phpServerLoading.value = true;
  phpServerError.value = '';
  try {
    phpServerInfo.value = await service.startPhpServer({
      runtimeId: selectedRuntime.value.id,
      docroot: phpServerDocroot.value,
      host: '127.0.0.1',
      port: phpServerPort.value,
    });
  } catch (error) {
    phpServerError.value = describeError(error, 'PHP 内置 Web Server 启动失败');
  } finally {
    phpServerLoading.value = false;
  }
}

async function stopPhpServer() {
  if (!phpServerInfo.value) return;
  phpServerLoading.value = true;
  try {
    await service.stopPhpServer(phpServerInfo.value.serverId);
    phpServerInfo.value = null;
  } catch (error) {
    phpServerError.value = describeError(error, 'PHP 内置 Web Server 关闭失败');
  } finally {
    phpServerLoading.value = false;
  }
}

async function diagnoseWebServers() {
  webServerLoading.value = true;
  webServerError.value = '';
  try {
    webServerDiagnostics.value = await service.diagnoseWebServers();
  } catch (error) {
    webServerError.value = describeError(error, '外部 Web Server 诊断失败');
  } finally {
    webServerLoading.value = false;
  }
}

async function requestEnable(runtime: RuntimeRecord) {
  errorMessage.value = '';
  if (runtime.family !== 'php') {
    errorMessage.value = '当前版本只支持 PHP 启用；其他运行时目前仅提供只读检测。';
    return;
  }
  try {
    const plan = await service.planEnable(runtime.id);
    confirmRuntime.value = plan.runtime;
    pendingPlanId.value = plan.planId;
  } catch (error) {
    errorMessage.value = describeError(error, '无法生成变更计划，请刷新后重试');
  }
}

async function applyEnable() {
  if (!pendingPlanId.value) return;
  isApplying.value = true;
  errorMessage.value = '';
  try {
    snapshot.value = await service.applyEnable(pendingPlanId.value);
    confirmRuntime.value = null;
    pendingPlanId.value = null;
  } catch (error) {
    errorMessage.value = describeError(error, '启用未完成，原有活动版本保持不变');
  } finally {
    isApplying.value = false;
  }
}

async function openPhpCatalog() {
  isCatalogOpen.value = true;
  catalogError.value = '';
  await loadPhpCatalog();
}

async function loadPhpCatalog() {
  isCatalogLoading.value = true;
  catalogError.value = '';
  try {
    catalogArtifacts.value = await service.listPhpCatalog();
  } catch (error) {
    catalogError.value = describeError(error, '暂时无法读取 PHP 版本清单');
    catalogArtifacts.value = [];
  } finally {
    isCatalogLoading.value = false;
  }
}

function closePhpCatalog() {
  if (catalogBusyArtifactId.value) return;
  isCatalogOpen.value = false;
  catalogError.value = '';
}

async function requestPhpInstall(artifact: RuntimeCatalogArtifact) {
  catalogBusyArtifactId.value = artifact.artifactId;
  catalogError.value = '';
  installError.value = '';
  try {
    const plan = await service.planPhpInstall(artifact.artifactId);
    installArtifact.value = plan.artifact;
    pendingInstallPlanId.value = plan.planId;
    isCatalogOpen.value = false;
  } catch (error) {
    catalogError.value = describeError(error, '无法生成下载计划');
  } finally {
    catalogBusyArtifactId.value = null;
  }
}

async function cancelPhpInstall() {
  if (!pendingInstallPlanId.value || isInstalling.value || isCancellingInstall.value) return;
  isCancellingInstall.value = true;
  installError.value = '';
  try {
    await service.cancelPhpInstall(pendingInstallPlanId.value);
    installArtifact.value = null;
    pendingInstallPlanId.value = null;
  } catch (error) {
    installError.value = describeError(error, 'PHP 安装计划取消失败，请重试');
  } finally {
    isCancellingInstall.value = false;
  }
}

async function applyPhpInstall() {
  if (!pendingInstallPlanId.value || isInstalling.value || isCancellingInstall.value) return;
  isInstalling.value = true;
  installError.value = '';
  errorMessage.value = '';
  try {
    snapshot.value = await service.applyPhpInstall(pendingInstallPlanId.value);
    installArtifact.value = null;
    pendingInstallPlanId.value = null;
  } catch (error) {
    installError.value = describeError(error, 'PHP 下载或安装未完成，未发布半成品目录');
  } finally {
    isInstalling.value = false;
  }
}

function dismissPhpInstall() {
  if (isInstalling.value || isCancellingInstall.value) return;
  installArtifact.value = null;
  installError.value = '';
}

async function planManualPhpPath() {
  if (!manualPath.value.trim()) {
    manualPathError.value = '请输入 php.exe 的绝对路径。';
    return;
  }
  manualPathBusy.value = true;
  manualPathError.value = '';
  try {
    const plan = await service.planPhpRegister(manualPath.value.trim());
    manualRuntime.value = plan.runtime;
    pendingManualPlanId.value = plan.planId;
  } catch (error) {
    manualPathError.value = describeError(error, 'PHP 路径验证失败');
  } finally {
    manualPathBusy.value = false;
  }
}

async function applyManualPhpPath() {
  if (!pendingManualPlanId.value) return;
  manualPathBusy.value = true;
  manualPathError.value = '';
  try {
    snapshot.value = await service.applyPhpRegister(pendingManualPlanId.value);
    pendingManualPlanId.value = null;
    manualRuntime.value = null;
    manualPath.value = '';
  } catch (error) {
    manualPathError.value = describeError(error, '手动路径登记未完成');
  } finally {
    manualPathBusy.value = false;
  }
}

function cancelManualPhpPath() {
  if (manualPathBusy.value) return;
  pendingManualPlanId.value = null;
  manualRuntime.value = null;
}

function appendTerminalOutput(text: string) {
  terminalOutput.value = `${terminalOutput.value}${text}`.slice(-256 * 1024);
}

async function pollTerminal() {
  if (!terminalSessionId.value) return;
  try {
    const events = await service.pollTerminal(terminalSessionId.value);
    for (const event of events) {
      if (event.kind === 'output' && event.text) appendTerminalOutput(event.text);
      if (event.kind === 'closed' || event.kind === 'exited') terminalStatus.value = 'closed';
    }
    if (terminalStatus.value === 'closed' && terminalPollTimer) {
      clearInterval(terminalPollTimer);
      terminalPollTimer = null;
    }
  } catch (error) {
    terminalError.value = describeError(error, '终端输出读取失败');
    terminalStatus.value = 'error';
  }
}

async function startTerminal() {
  if (terminalStatus.value === 'starting' || terminalStatus.value === 'running') return;
  terminalStatus.value = 'starting';
  terminalError.value = '';
  terminalOutput.value = '';
  try {
    const session = await service.startTerminal();
    terminalSessionId.value = session.sessionId;
    terminalCwd.value = session.cwd;
    terminalStatus.value = 'running';
    if (terminalPollTimer) clearInterval(terminalPollTimer);
    terminalPollTimer = setInterval(() => { void pollTerminal(); }, 100);
    await pollTerminal();
  } catch (error) {
    terminalStatus.value = 'error';
    terminalError.value = describeError(error, '终端启动失败');
  }
}

async function sendTerminalInput() {
  if (!terminalSessionId.value || terminalStatus.value !== 'running' || !terminalInput.value) return;
  const input = `${terminalInput.value}\r\n`;
  terminalInput.value = '';
  try {
    await service.sendTerminalInput(terminalSessionId.value, input);
  } catch (error) {
    terminalStatus.value = 'error';
    terminalError.value = describeError(error, '终端输入发送失败');
  }
}

async function closeTerminal() {
  if (terminalPollTimer) {
    clearInterval(terminalPollTimer);
    terminalPollTimer = null;
  }
  const sessionId = terminalSessionId.value;
  terminalSessionId.value = null;
  if (!sessionId) {
    terminalStatus.value = 'idle';
    return;
  }
  try {
    await service.closeTerminal(sessionId);
  } catch (error) {
    terminalError.value = describeError(error, '终端关闭失败');
  } finally {
    terminalStatus.value = 'closed';
  }
}

function selectRuntime(runtime: RuntimeRecord) {
  selectedRuntimeId.value = runtime.id;
  iniRuntimeId.value = null;
  iniPath.value = null;
  iniContent.value = null;
  iniError.value = '';
  iniValidation.value = null;
  iniBackupInfo.value = null;
  iniBackupError.value = '';
  if (activeView.value === 'overview') activeView.value = 'runtimes';
}

function formatPlatform() {
  return snapshot.value?.platformLabel ?? 'Windows 11 · 当前用户';
}

onMounted(refresh);
onBeforeUnmount(() => { void closeTerminal(); });
</script>

<template>
  <a class="skip-link" href="#main-content">跳转到主要内容</a>
  <div class="app-shell">
    <AppSidebar :active-view="activeView" @select="activeView = $event" />

    <main id="main-content" class="main-content">
      <header class="topbar">
        <div>
          <p class="breadcrumb"><span>MEnv</span><span aria-hidden="true">/</span>{{ viewTitle }}</p>
          <h1>{{ viewTitle }}</h1>
        </div>
        <div class="topbar-actions">
          <span class="platform-label"><span class="platform-dot" aria-hidden="true" />{{ formatPlatform() }}</span>
          <button type="button" class="icon-button" aria-label="打开设置" title="设置" @click="activeView = 'settings'">⚙</button>
          <button type="button" class="avatar-button" aria-label="当前用户 Developer">D</button>
        </div>
      </header>

      <div v-if="errorMessage" class="inline-alert" role="alert">
        <span aria-hidden="true">!</span>{{ errorMessage }}
        <button type="button" class="alert-close" aria-label="关闭提示" @click="errorMessage = ''">×</button>
      </div>

      <div v-if="isLoading" class="loading-state" role="status" aria-live="polite">
        <div class="loading-bar" />
        <p>正在读取本地环境状态…</p>
      </div>

      <template v-else-if="activeView === 'overview'">
        <section class="hero-grid" aria-labelledby="hero-title">
          <div class="hero-card">
            <div class="hero-copy">
              <p class="eyebrow">ENVIRONMENT CONTROL CENTER</p>
              <h2 id="hero-title">把开发环境，<em>交给你</em>。</h2>
              <p class="hero-description">统一发现、切换和诊断本机运行时。每次变更都有明确范围，外部安装保持原样。</p>
              <div class="hero-actions">
                <button type="button" class="button primary" :disabled="isScanning" @click="scanEnvironment">
                  <span v-if="isScanning" class="button-spinner" aria-hidden="true" />
                  {{ isScanning ? '扫描中…' : '扫描环境' }}
                </button>
                <button type="button" class="button ghost" @click="activeView = 'runtimes'">查看运行时 <span aria-hidden="true">→</span></button>
              </div>
            </div>
            <div class="hero-orbit" aria-hidden="true">
              <span class="orbit-ring ring-one" />
              <span class="orbit-ring ring-two" />
              <span class="orbit-core">M</span>
              <span class="orbit-tag tag-php">php 8.3</span>
              <span class="orbit-tag tag-python">python</span>
              <span class="orbit-tag tag-node">node</span>
            </div>
          </div>

          <div class="active-card">
            <div class="card-heading">
              <div>
                <p class="eyebrow">ACTIVE RUNTIME</p>
                <h2>{{ activeRuntime ? `${familyLabels[activeRuntime.family]} ${activeRuntime.version}` : '暂无活动版本' }}</h2>
              </div>
              <StatusPill :label="activeRuntime ? '当前启用' : '未启用'" :tone="activeRuntime ? 'active' : 'neutral'" />
            </div>
            <div v-if="activeRuntime" class="active-details">
              <div class="active-icon" :class="`family-${activeRuntime.family}`">{{ activeRuntime.family === 'php' ? 'php' : 'Py' }}</div>
              <div>
                <p>{{ activeRuntime.detail }}</p>
                <code>{{ activeRuntime.path }}</code>
              </div>
            </div>
            <div class="lease-row">
              <span class="lease-dot" aria-hidden="true" />
              <span>用户级 shim lease</span>
              <strong>{{ snapshot?.pathLease === 'active' ? '已生效' : '未启用' }}</strong>
            </div>
            <button type="button" class="card-link" @click="activeView = 'terminal'">打开新终端 <span aria-hidden="true">↗</span></button>
          </div>
        </section>

        <section class="metric-grid" aria-label="环境概览">
          <div class="metric-card"><span class="metric-label">已托管运行时</span><strong>{{ managedCount }}</strong><span class="metric-note">可由 MEnv 管理</span></div>
          <div class="metric-card"><span class="metric-label">检测到的外部环境</span><strong>{{ detectedCount }}</strong><span class="metric-note">只读 · 未接管</span></div>
          <div class="metric-card"><span class="metric-label">当前用户 PATH</span><strong class="metric-status"><span class="status-dot" />已保护</strong><span class="metric-note">仅包含 MEnv lease</span></div>
          <div class="metric-card"><span class="metric-label">最近扫描</span><strong class="metric-time">{{ snapshot?.lastScanAt ? '刚刚' : '尚未扫描' }}</strong><span class="metric-note">Windows 11 x64</span></div>
        </section>

        <section class="content-grid">
          <div class="panel runtime-panel" aria-labelledby="runtime-panel-title">
            <div class="panel-heading">
              <div><p class="eyebrow">RUNTIMES</p><h2 id="runtime-panel-title">运行时状态</h2></div>
              <div class="panel-heading-actions">
                <button type="button" class="button ghost compact-button" @click="openPhpCatalog">下载 PHP</button>
                <button type="button" class="text-button" @click="activeView = 'runtimes'">全部查看 <span aria-hidden="true">→</span></button>
              </div>
            </div>
            <div class="filter-bar" role="group" aria-label="运行时筛选">
              <button type="button" class="filter-button" :class="{ selected: filter === 'all' }" @click="filter = 'all'">全部 <span>{{ runtimes.length }}</span></button>
              <button type="button" class="filter-button" :class="{ selected: filter === 'php' }" @click="filter = 'php'">PHP <span>{{ runtimes.filter((runtime) => runtime.family === 'php').length }}</span></button>
              <button type="button" class="filter-button" :class="{ selected: filter === 'other' }" @click="filter = 'other'">其他 <span>{{ runtimes.filter((runtime) => runtime.family !== 'php').length }}</span></button>
            </div>
            <div class="runtime-list" role="list" aria-label="运行时列表">
              <RuntimeRow
                v-for="runtime in filteredRuntimes.slice(0, 4)"
                :key="runtime.id"
                :runtime="runtime"
                :selected="selectedRuntime?.id === runtime.id"
                @select="selectRuntime(runtime)"
                @enable="requestEnable(runtime)"
              />
            </div>
          </div>

          <aside class="panel activity-panel" aria-labelledby="activity-panel-title">
            <div class="panel-heading"><div><p class="eyebrow">AUDIT TRAIL</p><h2 id="activity-panel-title">最近活动</h2></div><button type="button" class="icon-button subtle" aria-label="刷新活动" title="刷新" @click="refresh">↻</button></div>
            <ActivityList :activities="snapshot?.activities ?? []" />
            <button type="button" class="card-link activity-link" @click="activeView = 'activity'">查看完整日志 <span aria-hidden="true">→</span></button>
          </aside>
        </section>
      </template>

      <template v-else-if="activeView === 'runtimes'">
        <section class="page-intro"><p class="eyebrow">RUNTIME INVENTORY</p><h2>所有运行时</h2><p>检测、登记与启用是三个独立动作。选择一个版本查看它的健康检查和路径来源。</p></section>
        <section class="panel full-runtime-panel" aria-labelledby="all-runtime-title">
          <div class="panel-heading"><div><h2 id="all-runtime-title">本机运行时</h2><span class="panel-subtitle">{{ runtimes.length }} 个候选 · 最后检查 {{ snapshot?.lastScanAt ?? '—' }}</span></div><div class="panel-heading-actions"><button type="button" class="button primary" @click="openPhpCatalog">下载 PHP</button><button type="button" class="button secondary" :disabled="isScanning" @click="scanEnvironment">{{ isScanning ? '扫描中…' : '重新扫描' }}</button></div></div>
          <div class="filter-bar large" role="group" aria-label="运行时筛选"><button type="button" class="filter-button" :class="{ selected: filter === 'all' }" @click="filter = 'all'">全部 <span>{{ runtimes.length }}</span></button><button type="button" class="filter-button" :class="{ selected: filter === 'php' }" @click="filter = 'php'">PHP</button><button type="button" class="filter-button" :class="{ selected: filter === 'other' }" @click="filter = 'other'">其他</button></div>
          <div class="runtime-list full" role="list"><RuntimeRow v-for="runtime in filteredRuntimes" :key="runtime.id" :runtime="runtime" :selected="selectedRuntime?.id === runtime.id" @select="selectRuntime(runtime)" @enable="requestEnable(runtime)" /></div>
          <section class="runtime-detail-card" aria-labelledby="manual-php-title">
            <div class="panel-heading"><div><p class="eyebrow">MANUAL REGISTRATION</p><h2 id="manual-php-title">手动登记 PHP</h2><span class="panel-subtitle">只读验证绝对路径；不会复制、覆盖或自动启用外部文件。</span></div></div>
            <div class="manual-path-form">
              <label for="manual-php-path">php.exe 路径</label>
              <div class="manual-path-row"><input id="manual-php-path" v-model="manualPath" type="text" placeholder="例如 R:\\App\\Env\\php\\php8.5.11\\php.exe" :disabled="manualPathBusy" /><button type="button" class="button secondary compact-button" :disabled="manualPathBusy || !manualPath.trim()" @click="planManualPhpPath">{{ manualPathBusy && !pendingManualPlanId ? '验证中…' : '验证路径' }}</button></div>
            </div>
            <div v-if="manualPathError" class="inline-alert" role="alert"><span aria-hidden="true">!</span>{{ manualPathError }}</div>
            <div v-if="manualRuntime && pendingManualPlanId" class="manual-path-confirm"><div><strong>已验证：PHP {{ manualRuntime.version }}</strong><span>{{ manualRuntime.path }} · {{ manualRuntime.architecture }}</span></div><div class="terminal-action-group"><button type="button" class="button primary compact-button" :disabled="manualPathBusy" @click="applyManualPhpPath">{{ manualPathBusy ? '登记中…' : '确认登记' }}</button><button type="button" class="button ghost compact-button" :disabled="manualPathBusy" @click="cancelManualPhpPath">取消</button></div></div>
          </section>
          <section v-if="selectedRuntime?.family === 'php'" class="runtime-detail-card" aria-labelledby="php-ini-title">
            <div class="panel-heading"><div><p class="eyebrow">PHP CONFIGURATION</p><h2 id="php-ini-title">php.ini</h2><span class="panel-subtitle">只读查看 · {{ iniPath ?? '当前版本未发现明确配置文件' }}</span></div><div class="panel-heading-actions"><button type="button" class="button secondary compact-button" :disabled="iniLoading" @click="loadPhpIni(selectedRuntime!)">{{ iniLoading ? '读取中…' : '读取 php.ini' }}</button><button type="button" class="button ghost compact-button" :disabled="!iniContent" @click="validateLoadedPhpIni">验证格式</button><button type="button" class="button ghost compact-button" :disabled="!iniPath || iniBackupBusy" @click="requestPhpIniBackup(selectedRuntime!)">备份</button></div></div>
            <div v-if="iniError" class="inline-alert" role="alert"><span aria-hidden="true">!</span>{{ iniError }}</div>
            <pre v-else-if="iniContent" class="ini-viewer">{{ iniContent }}</pre>
            <p v-else class="panel-subtitle">点击“读取 php.ini”查看当前 PHP 的配置内容。不会修改配置文件。</p>
            <div v-if="iniValidation" class="ini-validation" :class="{ valid: iniValidation.valid }" role="status"><strong>{{ iniValidation.valid ? '格式检查通过' : '发现格式问题' }}</strong><ul v-if="iniValidation.diagnostics.length"><li v-for="diagnostic in iniValidation.diagnostics" :key="`${diagnostic.code}-${diagnostic.line}-${diagnostic.message}`">{{ diagnostic.line ? `第 ${diagnostic.line} 行：` : '' }}{{ diagnostic.message }}</li></ul><p v-else>未发现明显的节或指令格式错误。未知扩展参数会被保留。</p></div>
            <div v-if="iniBackupError" class="inline-alert" role="alert"><span aria-hidden="true">!</span>{{ iniBackupError }}</div>
            <p v-if="iniBackupInfo" class="panel-subtitle">备份已创建：{{ iniBackupInfo.backupPath }}</p>
          </section>
          <section v-if="selectedRuntime?.family === 'php'" class="runtime-detail-card" aria-labelledby="php-server-title">
            <div class="panel-heading"><div><p class="eyebrow">PHP BUILT-IN SERVER</p><h2 id="php-server-title">内置 Web Server</h2><span class="panel-subtitle">固定 loopback host · 不加载 php.ini</span></div><StatusPill :label="phpServerInfo ? '运行中' : '未启动'" :tone="phpServerInfo ? 'success' : 'neutral'" compact /></div>
            <div class="server-form"><label>Docroot<input v-model="phpServerDocroot" :disabled="Boolean(phpServerInfo)" aria-label="PHP Web Server docroot" /></label><label>Port<input v-model.number="phpServerPort" type="number" min="1024" max="65535" :disabled="Boolean(phpServerInfo)" aria-label="PHP Web Server port" /></label></div>
            <p v-if="phpServerInfo" class="panel-subtitle">访问 {{ phpServerInfo.endpoint }} · 日志 {{ phpServerInfo.logPath }}</p>
            <div v-if="phpServerError" class="inline-alert" role="alert"><span aria-hidden="true">!</span>{{ phpServerError }}</div>
            <div class="terminal-action-group"><button v-if="!phpServerInfo" type="button" class="button primary compact-button" :disabled="phpServerLoading" @click="startPhpServer">{{ phpServerLoading ? '启动中…' : '启动 Web Server' }}</button><button v-else type="button" class="button secondary compact-button" :disabled="phpServerLoading" @click="stopPhpServer">{{ phpServerLoading ? '关闭中…' : '关闭 Web Server' }}</button></div>
          </section>
          <section class="runtime-detail-card" aria-labelledby="web-server-diagnostics-title">
            <div class="panel-heading"><div><p class="eyebrow">EXTERNAL WEB SERVERS</p><h2 id="web-server-diagnostics-title">外部 Web Server 诊断</h2><span class="panel-subtitle">只读检查 IIS、Apache、Nginx 配置，不会安装或重启服务。</span></div><button type="button" class="button secondary compact-button" :disabled="webServerLoading" @click="diagnoseWebServers">{{ webServerLoading ? '检查中…' : '开始诊断' }}</button></div>
            <div v-if="webServerError" class="inline-alert" role="alert"><span aria-hidden="true">!</span>{{ webServerError }}</div>
            <div v-else-if="webServerDiagnostics.length" class="web-server-diagnostic-list"><article v-for="diagnostic in webServerDiagnostics" :key="diagnostic.server" class="diagnostic-row"><div><strong>{{ diagnostic.server }}</strong><span>{{ diagnostic.detected ? '已发现配置' : '未发现固定配置' }}</span></div><small v-if="diagnostic.phpReferences.length">{{ diagnostic.phpReferences[0] }}</small><small v-else-if="diagnostic.warnings.length">{{ diagnostic.warnings[0] }}</small></article></div>
            <p v-else class="panel-subtitle">点击“开始诊断”读取固定配置位置。</p>
          </section>
        </section>
      </template>

      <template v-else-if="activeView === 'terminal'">
        <section class="page-intro"><p class="eyebrow">SESSION ENVIRONMENT</p><h2>新终端</h2><p>每个会话在启动时绑定当前版本。切换运行时后，请打开新的终端窗口。</p></section>
        <section class="terminal-layout"><div class="terminal-card"><div class="terminal-toolbar"><div class="window-dots"><span /><span /><span /></div><span>PowerShell · menv session</span><StatusPill :label="terminalStatus === 'running' ? '运行中' : terminalStatus === 'starting' ? '启动中' : terminalStatus === 'error' ? '异常' : '未启动'" :tone="terminalStatus === 'running' ? 'success' : terminalStatus === 'error' ? 'error' : 'neutral'" compact /></div><div class="terminal-screen" role="log" aria-live="polite" aria-label="MEnv PowerShell 输出"><p><span class="terminal-muted">MEnv session · Windows 11 · {{ terminalCwd || '尚未启动' }}</span></p><pre v-if="terminalOutput" class="terminal-output terminal-output-live">{{ terminalOutput }}</pre><p v-else class="terminal-muted">点击“启动终端”创建受控 PowerShell 会话。</p></div><div class="terminal-input-row"><label class="sr-only" for="terminal-input">终端输入</label><input id="terminal-input" v-model="terminalInput" :disabled="terminalStatus !== 'running'" placeholder="输入命令，例如 php -v" @keydown.enter="sendTerminalInput" /><button type="button" class="button secondary compact-button" :disabled="terminalStatus !== 'running' || !terminalInput" @click="sendTerminalInput">发送</button></div><div v-if="terminalError" class="inline-alert terminal-alert" role="alert"><span aria-hidden="true">!</span>{{ terminalError }}</div><div class="terminal-actions"><div class="terminal-action-group"><button type="button" class="button primary" :disabled="terminalStatus === 'starting' || terminalStatus === 'running'" @click="startTerminal">{{ terminalStatus === 'starting' ? '启动中…' : '启动终端' }}</button><button type="button" class="button ghost compact-button" :disabled="!terminalSessionId" @click="closeTerminal">关闭会话</button></div><span>当前环境：{{ activeRuntime ? `${familyLabels[activeRuntime.family]} ${activeRuntime.version}` : '未选择运行时' }}</span></div></div><aside class="panel terminal-info"><p class="eyebrow">SESSION RULES</p><h2>安全边界</h2><ul><li>只使用固定 Windows PowerShell executable</li><li>新会话读取启动时环境快照</li><li>输入、输出均有大小上限</li><li>关闭会话回收 Job Object 进程树</li></ul></aside></section>
      </template>

      <template v-else-if="activeView === 'activity'">
        <section class="page-intro"><p class="eyebrow">AUDIT TRAIL</p><h2>活动日志</h2><p>记录运行时状态与变更结果，不包含完整环境变量、Token 或原始命令输出。</p></section>
        <section class="panel full-activity-panel"><div class="panel-heading"><div><h2>最近操作</h2><span class="panel-subtitle">按时间倒序 · 仅本机</span></div><StatusPill label="日志正常" tone="success" /></div><ActivityList :activities="snapshot?.activities ?? []" /></section>
      </template>

      <template v-else>
        <section class="empty-view" role="status"><div class="empty-icon" aria-hidden="true">⚙</div><p class="eyebrow">COMING SOON</p><h2>设置即将开放</h2><p>这里将管理目录、更新策略和用户级 PATH lease。当前原型不会修改系统设置。</p><button type="button" class="button secondary" @click="activeView = 'overview'">返回总览</button></section>
      </template>
    </main>
  </div>

  <ConfirmDialog :open="Boolean(confirmRuntime)" :runtime="confirmRuntime" :busy="isApplying" @cancel="confirmRuntime = null; pendingPlanId = null" @confirm="applyEnable" />
  <PhpCatalogDialog
    :open="isCatalogOpen"
    :artifacts="catalogArtifacts"
    :loading="isCatalogLoading"
    :busy-artifact-id="catalogBusyArtifactId"
    :error="catalogError"
    @cancel="closePhpCatalog"
    @refresh="loadPhpCatalog"
    @install="requestPhpInstall"
  />
  <PhpInstallConfirmDialog
    :open="Boolean(installArtifact)"
    :artifact="installArtifact"
    :busy="isInstalling || isCancellingInstall"
    :cancelling="isCancellingInstall"
    :error="installError"
    @cancel="cancelPhpInstall"
    @confirm="applyPhpInstall"
    @dismiss="dismissPhpInstall"
  />
  <PhpIniBackupConfirmDialog
    :open="Boolean(iniBackupRuntime)"
    :runtime="iniBackupRuntime"
    :source-path="iniBackupSourcePath"
    :source-sha256="iniBackupSourceSha256"
    :busy="iniBackupBusy"
    @cancel="cancelPhpIniBackup"
    @confirm="applyPhpIniBackup"
  />
</template>
