<script setup lang="ts">
import {
  activationLabels,
  availabilityLabels,
  familyLabels,
  healthLabels,
  type RuntimeRecord,
} from '../types/runtime';
import StatusPill from './StatusPill.vue';

defineProps<{ runtime: RuntimeRecord; selected: boolean }>();
const emit = defineEmits<{ select: []; enable: [] }>();

const activationTone = (runtime: RuntimeRecord) => {
  if (runtime.activation === 'active') return 'active';
  if (runtime.activation === 'failed') return 'error';
  if (runtime.activation === 'enabling' || runtime.activation === 'disabling') return 'warning';
  return 'neutral';
};

const healthTone = (runtime: RuntimeRecord) => {
  if (runtime.health === 'healthy') return 'success';
  if (runtime.health === 'error') return 'error';
  if (runtime.health === 'unknown') return 'neutral';
  return 'warning';
};
</script>

<template>
  <article class="runtime-row" :class="{ selected }" role="listitem">
    <button class="runtime-main" type="button" @click="emit('select')">
      <span class="runtime-logo" :class="`family-${runtime.family}`" aria-hidden="true">
        {{ runtime.family === 'php' ? 'php' : runtime.family === 'python' ? 'Py' : runtime.family.slice(0, 2).toUpperCase() }}
      </span>
      <span class="runtime-title">
        <span class="runtime-name-line">
          <strong>{{ familyLabels[runtime.family] }} {{ runtime.version }}</strong>
          <span v-if="runtime.eol" class="legacy-label">legacy</span>
        </span>
        <span class="runtime-meta">{{ runtime.architecture }} · {{ runtime.detail }}</span>
      </span>
    </button>

    <div class="runtime-statuses" aria-label="运行时状态">
      <StatusPill :label="availabilityLabels[runtime.availability]" :tone="runtime.availability === 'managed' ? 'success' : 'warning'" compact />
      <StatusPill :label="activationLabels[runtime.activation]" :tone="activationTone(runtime)" compact />
      <StatusPill :label="healthLabels[runtime.health]" :tone="healthTone(runtime)" compact />
    </div>

    <div class="runtime-action">
      <button
        v-if="runtime.activation !== 'active'"
        type="button"
        class="text-button"
        :disabled="runtime.availability === 'invalid-path' || runtime.availability === 'unsupported'"
        @click="emit('enable')"
      >
        启用
      </button>
      <span v-else class="active-caption">当前版本</span>
    </div>
  </article>
</template>
