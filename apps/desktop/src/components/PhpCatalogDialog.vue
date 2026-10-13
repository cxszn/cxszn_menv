<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import { activationLabels, type RuntimeCatalogArtifact } from '../types/runtime';

const props = defineProps<{
  open: boolean;
  artifacts: RuntimeCatalogArtifact[];
  loading?: boolean;
  busyArtifactId?: string | null;
  error?: string;
}>();

const emit = defineEmits<{
  cancel: [];
  refresh: [];
  install: [artifact: RuntimeCatalogArtifact];
}>();

const closeButton = ref<HTMLButtonElement | null>(null);

watch(
  () => props.open,
  async (open) => {
    if (open) {
      await nextTick();
      closeButton.value?.focus();
    }
  },
);

function lifecycleLabel(artifact: RuntimeCatalogArtifact): string {
  if (artifact.lifecycle === 'eol') return 'EOL · 归档风险';
  if (artifact.lifecycle === 'archival') return '归档版本';
  if (artifact.lifecycle === 'revoked') return '已撤销';
  return '支持中';
}

function lifecycleTone(artifact: RuntimeCatalogArtifact): 'success' | 'warning' | 'danger' {
  if (artifact.lifecycle === 'supported') return 'success';
  if (artifact.lifecycle === 'revoked') return 'danger';
  return 'warning';
}

function formatSize(sizeBytes?: number): string {
  if (!sizeBytes) return '大小由安装器校验';
  return `${(sizeBytes / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <div v-if="open" class="dialog-layer" role="presentation" @keydown.esc="emit('cancel')">
    <section class="catalog-dialog" role="dialog" aria-modal="true" aria-labelledby="php-catalog-title" aria-describedby="php-catalog-copy">
      <div class="dialog-header">
        <div>
          <p class="eyebrow">VERIFIED RUNTIME CATALOG</p>
          <h2 id="php-catalog-title">下载 PHP 版本</h2>
        </div>
        <button ref="closeButton" type="button" class="icon-button subtle" aria-label="关闭 PHP 版本目录" @click="emit('cancel')">×</button>
      </div>
      <p id="php-catalog-copy" class="dialog-copy">
        版本来自已验证的固定来源清单。下载完成后会校验大小和 SHA-256，并登记为“已托管 · 未启用”；不会自动修改 PATH。
      </p>

      <div v-if="error" class="catalog-error" role="alert">
        <span aria-hidden="true">!</span>
        <span>{{ error }}</span>
      </div>
      <div v-if="loading" class="catalog-loading" role="status" aria-live="polite">
        正在读取已验证版本清单…
      </div>
      <div v-else-if="!artifacts.length" class="catalog-empty" role="status">
        暂无可用的 PHP 版本。请稍后重新读取清单。
      </div>
      <ul v-else class="catalog-list" aria-label="可下载的 PHP 版本">
        <li v-for="artifact in artifacts" :key="artifact.artifactId" class="catalog-item">
          <div class="catalog-item-main">
            <div class="catalog-item-title">
              <strong>PHP {{ artifact.version }}</strong>
              <span v-if="artifact.legacy" class="legacy-label">legacy</span>
            </div>
            <span class="catalog-item-meta">
              {{ artifact.architecture }} · {{ artifact.threadSafety.toUpperCase() }} · {{ formatSize(artifact.sizeBytes) }}
            </span>
            <span class="catalog-item-source">{{ artifact.fileName ?? '固定来源 artifact' }}</span>
          </div>
          <div class="catalog-item-side">
            <span class="catalog-lifecycle" :class="`catalog-${lifecycleTone(artifact)}`">{{ lifecycleLabel(artifact) }}</span>
            <span v-if="artifact.installed" class="catalog-installed">已登记</span>
            <button
              v-else
              type="button"
              class="button secondary catalog-download"
              :disabled="!artifact.available || artifact.lifecycle === 'revoked' || Boolean(busyArtifactId)"
              @click="emit('install', artifact)"
            >
              <span v-if="busyArtifactId === artifact.artifactId" class="button-spinner" aria-hidden="true" />
              {{ busyArtifactId === artifact.artifactId ? '准备中…' : artifact.available ? '下载并安装' : '不可用' }}
            </button>
          </div>
        </li>
      </ul>

      <div class="catalog-footer">
        <span>所有下载都需要确认安装计划。</span>
        <button type="button" class="button ghost" :disabled="loading" @click="emit('refresh')">重新读取</button>
      </div>
    </section>
  </div>
</template>
