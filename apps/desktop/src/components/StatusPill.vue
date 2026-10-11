<script setup lang="ts">
withDefaults(
  defineProps<{
    label: string;
    tone?: 'neutral' | 'success' | 'warning' | 'error' | 'active';
    compact?: boolean;
  }>(),
  { tone: 'neutral', compact: false },
);
</script>

<template>
  <span class="status-pill" :class="[`tone-${tone}`, { compact }]">
    <span class="pill-dot" aria-hidden="true" />
    {{ label }}
  </span>
</template>
