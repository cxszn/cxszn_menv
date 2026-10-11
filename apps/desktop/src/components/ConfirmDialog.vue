<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import type { RuntimeRecord } from '../types/runtime';

const props = defineProps<{ open: boolean; runtime: RuntimeRecord | null; busy?: boolean }>();
const emit = defineEmits<{ cancel: []; confirm: [] }>();
const cancelButton = ref<HTMLButtonElement | null>(null);

watch(
  () => props.open,
  async (open) => {
    if (open) {
      await nextTick();
      cancelButton.value?.focus();
    }
  },
);
</script>

<template>
  <div v-if="open && runtime" class="dialog-layer" role="presentation" @keydown.esc="emit('cancel')">
    <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-copy">
      <div class="dialog-icon" aria-hidden="true">↗</div>
      <p class="eyebrow">确认环境变更</p>
      <h2 id="confirm-title">启用 {{ runtime.name }} {{ runtime.version }}？</h2>
      <p id="confirm-copy" class="dialog-copy">
        MEnv 会更新应用自己的用户级 shim。已有终端不会热切换，请新开终端后使用；外部安装目录和系统 PATH 其他条目不会被修改。
      </p>
      <div class="dialog-plan">
        <div><span>作用域</span><strong>当前用户 · 新终端</strong></div>
        <div><span>目标</span><strong>{{ runtime.path ?? '由核心服务解析' }}</strong></div>
      </div>
      <div class="dialog-actions">
        <button ref="cancelButton" type="button" class="button secondary" :disabled="busy" @click="emit('cancel')">取消</button>
        <button type="button" class="button primary" :disabled="busy" @click="emit('confirm')">
          {{ busy ? '正在应用…' : '确认启用' }}
        </button>
      </div>
    </section>
  </div>
</template>
