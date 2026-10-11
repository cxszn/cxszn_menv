<script setup lang="ts">
import type { ActivityEntry } from '../types/runtime';

defineProps<{ activities: ActivityEntry[] }>();
</script>

<template>
  <ol class="activity-list" aria-label="最近活动">
    <li v-for="activity in activities.slice(0, 4)" :key="activity.id" class="activity-item">
      <span class="activity-marker" :class="`marker-${activity.tone}`" aria-hidden="true" />
      <div class="activity-copy">
        <div class="activity-title-line">
          <strong>{{ activity.title }}</strong>
          <time>{{ activity.occurredAt }}</time>
        </div>
        <p>{{ activity.detail }}</p>
      </div>
    </li>
  </ol>
</template>
