<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import type { RuntimeCatalogArtifact } from '../types/runtime';

const props = defineProps<{
  open: boolean;
  artifact: RuntimeCatalogArtifact | null;
  busy?: boolean;
  cancelling?: boolean;
  error?: string;
}>();

const emit = defineEmits<{ cancel: []; confirm: []; dismiss: [] }>();
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
  <div v-if="open && artifact" class="dialog-layer" role="presentation" @keydown.esc="emit('cancel')">
    <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="install-confirm-title" aria-describedby="install-confirm-copy">
      <div class="dialog-icon" aria-hidden="true">↓</div>
      <p class="eyebrow">确认下载与安装</p>
      <h2 id="install-confirm-title">安装 PHP {{ artifact.version }}？</h2>
      <p id="install-confirm-copy" class="dialog-copy">
        MEnv 会从已验证的固定来源下载 artifact，校验大小与 SHA-256 后原子发布到受管版本目录。安装完成后保持未启用，不会写入 PATH，也不会覆盖手动登记的 PHP。
      </p>
      <div class="dialog-plan">
        <div><span>架构</span><strong>{{ artifact.architecture }} · {{ artifact.threadSafety.toUpperCase() }}</strong></div>
        <div><span>来源</span><strong>{{ artifact.fileName ?? '签名 catalog artifact' }}</strong></div>
        <div><span>状态</span><strong>下载后 · 已托管 · 未启用</strong></div>
      </div>
      <div v-if="error" class="catalog-error" role="alert"><span aria-hidden="true">!</span><span>{{ error }}</span></div>
      <div class="dialog-actions">
        <button v-if="error" type="button" class="button ghost" :disabled="busy" @click="emit('dismiss')">关闭（保留计划）</button>
        <button ref="cancelButton" type="button" class="button secondary" :disabled="busy" @click="emit('cancel')">{{ cancelling ? '正在取消…' : '取消' }}</button>
        <button type="button" class="button primary" :disabled="busy" @click="emit('confirm')">
          {{ busy && !cancelling ? '正在下载并校验…' : '确认下载' }}
        </button>
      </div>
    </section>
  </div>
</template>
