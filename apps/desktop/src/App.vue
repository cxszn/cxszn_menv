<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import ActivityList from './components/ActivityList.vue';
import AppSidebar from './components/AppSidebar.vue';
import ConfirmDialog from './components/ConfirmDialog.vue';
import RuntimeRow from './components/RuntimeRow.vue';
import StatusPill from './components/StatusPill.vue';
import { createMockRuntimeService } from './services/mockRuntimeService';
import {
  activationLabels,
  availabilityLabels,
  familyLabels,
  type EnvironmentSnapshot,
  type RuntimeRecord,
} from './types/runtime';

const service = createMockRuntimeService();
const activeView = ref('overview');
const filter = ref<'all' | 'php' | 'other'>('all');
const snapshot = ref<EnvironmentSnapshot | null>(null);
const selectedRuntimeId = ref<string | null>(null);
const isLoading = ref(true);
const isScanning = ref(false);
const isApplying = ref(false);
const errorMessage = ref('');
const confirmRuntime = ref<RuntimeRecord | null>(null);
const pendingPlanId = ref<string | null>(null);

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

async function refresh() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    snapshot.value = await service.getSnapshot();
    selectedRuntimeId.value ??= snapshot.value.runtimes[0]?.id ?? null;
  } catch {
    errorMessage.value = '暂时无法读取本地环境状态，请稍后重试。';
  } finally {
    isLoading.value = false;
  }
}

async function scanEnvironment() {
  isScanning.value = true;
  errorMessage.value = '';
  try {
    snapshot.value = await service.scan();
  } catch {
    errorMessage.value = '环境扫描未完成，当前状态保持不变。';
  } finally {
    isScanning.value = false;
  }
}

async function requestEnable(runtime: RuntimeRecord) {
  errorMessage.value = '';
  try {
    const plan = await service.planEnable(runtime.id);
    confirmRuntime.value = plan.runtime;
    pendingPlanId.value = plan.planId;
  } catch {
    errorMessage.value = '无法生成变更计划，请刷新后重试。';
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
  } catch {
    errorMessage.value = '启用未完成，原有活动版本保持不变。';
  } finally {
    isApplying.value = false;
  }
}

function selectRuntime(runtime: RuntimeRecord) {
  selectedRuntimeId.value = runtime.id;
  if (activeView.value === 'overview') activeView.value = 'runtimes';
}

function formatPlatform() {
  return snapshot.value?.platformLabel ?? 'Windows 11 · 当前用户';
}

onMounted(refresh);
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
              <button type="button" class="text-button" @click="activeView = 'runtimes'">全部查看 <span aria-hidden="true">→</span></button>
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
          <div class="panel-heading"><div><h2 id="all-runtime-title">本机运行时</h2><span class="panel-subtitle">{{ runtimes.length }} 个候选 · 最后检查 {{ snapshot?.lastScanAt ?? '—' }}</span></div><button type="button" class="button secondary" :disabled="isScanning" @click="scanEnvironment">{{ isScanning ? '扫描中…' : '重新扫描' }}</button></div>
          <div class="filter-bar large" role="group" aria-label="运行时筛选"><button type="button" class="filter-button" :class="{ selected: filter === 'all' }" @click="filter = 'all'">全部 <span>{{ runtimes.length }}</span></button><button type="button" class="filter-button" :class="{ selected: filter === 'php' }" @click="filter = 'php'">PHP</button><button type="button" class="filter-button" :class="{ selected: filter === 'other' }" @click="filter = 'other'">其他</button></div>
          <div class="runtime-list full" role="list"><RuntimeRow v-for="runtime in filteredRuntimes" :key="runtime.id" :runtime="runtime" :selected="selectedRuntime?.id === runtime.id" @select="selectedRuntimeId = runtime.id" @enable="requestEnable(runtime)" /></div>
        </section>
      </template>

      <template v-else-if="activeView === 'terminal'">
        <section class="page-intro"><p class="eyebrow">SESSION ENVIRONMENT</p><h2>新终端</h2><p>每个会话在启动时绑定当前版本。切换运行时后，请打开新的终端窗口。</p></section>
        <section class="terminal-layout"><div class="terminal-card"><div class="terminal-toolbar"><div class="window-dots"><span /><span /><span /></div><span>PowerShell · menv session</span><StatusPill label="待启动" tone="neutral" compact /></div><div class="terminal-screen" role="img" aria-label="终端预览"><p><span class="terminal-muted">MEnv session · Windows 11</span></p><p><span class="terminal-prompt">PS C:\\Projects\demo&gt;</span> <span class="terminal-command">php -v</span></p><p class="terminal-output">PHP 8.3.14 (cli) (built: Nov 19 2024 10:21:12)<br />Copyright (c) The PHP Group</p><p><span class="terminal-prompt">PS C:\\Projects\demo&gt;</span><span class="terminal-caret" aria-hidden="true" /></p></div><div class="terminal-actions"><button type="button" class="button primary" disabled>启动终端（即将支持）</button><span>当前环境：{{ activeRuntime ? `${familyLabels[activeRuntime.family]} ${activeRuntime.version}` : '未选择运行时' }}</span></div></div><aside class="panel terminal-info"><p class="eyebrow">SESSION RULES</p><h2>安全边界</h2><ul><li>只使用绝对 executable 与受控工作目录</li><li>新会话读取最新 active 快照</li><li>已有进程不会因切换被终止</li><li>输出与环境变量不会写入活动日志</li></ul></aside></section>
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
</template>
