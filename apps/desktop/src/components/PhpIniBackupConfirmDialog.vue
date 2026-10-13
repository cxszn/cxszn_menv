<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import type { RuntimeRecord } from '../types/runtime';

const props = defineProps<{ open: boolean; runtime: RuntimeRecord | null; sourcePath: string; sourceSha256: string; busy?: boolean }>();
const emit = defineEmits<{ cancel: []; confirm: [] }>();
const cancelButton = ref<HTMLButtonElement | null>(null);

watch(() => props.open, async (open) => {
  if (open) {
    await nextTick();
    cancelButton.value?.focus();
  }
});
</script>

<template>
  <div v-if="open && runtime" class="dialog-layer" role="presentation" @keydown.esc="emit('cancel')">
    <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="ini-backup-title" aria-describedby="ini-backup-copy">
      <div class="dialog-icon" aria-hidden="true">▣</div>
      <p class="eyebrow">备份 PHP 配置</p>
      <h2 id="ini-backup-title">备份 PHP {{ runtime.version }} 的 php.ini？</h2>
      <p id="ini-backup-copy" class="dialog-copy">
        MEnv 会把当前配置复制到应用自己的备份目录。不会修改、覆盖或删除原始 php.ini，也不会自动打开外部编辑器。
      </p>
      <div class="dialog-plan">
        <div><span>原配置文件</span><strong>{{ sourcePath }}</strong></div>
        <div><span>原文件 SHA-256</span><strong>{{ sourceSha256 }}</strong></div>
      </div>
      <p class="dialog-copy">计划生成后文件若有变化，会要求重新确认。单个备份文件上限为 4 MiB。</p>
      <div class="dialog-actions">
        <button ref="cancelButton" type="button" class="button secondary" :disabled="busy" @click="emit('cancel')">取消</button>
        <button type="button" class="button primary" :disabled="busy || !sourcePath || !sourceSha256" @click="emit('confirm')">{{ busy ? '备份中…' : '确认备份' }}</button>
      </div>
    </section>
  </div>
</template>
