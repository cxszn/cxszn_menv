<script setup lang="ts">
import { version as appVersion } from '../../package.json';

defineProps<{ activeView: string }>();
const emit = defineEmits<{ select: [view: string] }>();

const navItems = [
  { id: 'overview', label: '总览', icon: '⌂' },
  { id: 'runtimes', label: '运行时', icon: '◈' },
  { id: 'terminal', label: '终端', icon: '>_' },
  { id: 'activity', label: '活动日志', icon: '◷' },
];
</script>

<template>
  <aside class="sidebar" aria-label="主导航">
    <div class="brand-block">
      <div class="brand-mark" aria-hidden="true">M</div>
      <div>
        <p class="brand-name">MEnv</p>
        <p class="brand-caption">Developer runtimes</p>
      </div>
    </div>

    <nav class="nav-list" aria-label="工作区">
      <button
        v-for="item in navItems"
        :key="item.id"
        type="button"
        class="nav-item"
        :class="{ active: activeView === item.id }"
        :aria-current="activeView === item.id ? 'page' : undefined"
        @click="emit('select', item.id)"
      >
        <span class="nav-icon" aria-hidden="true">{{ item.icon }}</span>
        <span>{{ item.label }}</span>
      </button>
    </nav>

    <div class="sidebar-spacer" />

    <div class="sidebar-footnote">
      <span class="status-dot" aria-hidden="true" />
      <div>
        <span class="footnote-label">本地服务</span>
        <span class="footnote-meta">已连接 · v{{ appVersion }} preview</span>
      </div>
    </div>
  </aside>
</template>
