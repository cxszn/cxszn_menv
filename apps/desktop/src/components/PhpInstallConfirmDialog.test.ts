import { createSSRApp } from 'vue';
import { renderToString } from 'vue/server-renderer';
import { describe, expect, it } from 'vitest';
import PhpInstallConfirmDialog from './PhpInstallConfirmDialog.vue';
import type { RuntimeCatalogArtifact } from '../types/runtime';

const artifact: RuntimeCatalogArtifact = {
  artifactId: 'php-8.5.11-nts-x64', family: 'php', version: '8.5.11',
  architecture: 'x64', threadSafety: 'nts', lifecycle: 'supported',
  legacy: false, archival: false, installed: false, available: true,
};

describe('PHP install confirmation recovery', () => {
  it('offers a distinct way to close a failed installation while preserving its plan', async () => {
    const html = await renderToString(createSSRApp(PhpInstallConfirmDialog, {
      open: true, artifact, error: 'OPERATION_NOT_CANCELLABLE',
    }));
    expect(html).toContain('关闭（保留计划）');
    expect(html).toContain('OPERATION_NOT_CANCELLABLE');
  });

  it('does not offer dismiss before an error and disables it during an operation', async () => {
    const ready = await renderToString(createSSRApp(PhpInstallConfirmDialog, { open: true, artifact }));
    expect(ready).not.toContain('关闭（保留计划）');
    const busy = await renderToString(createSSRApp(PhpInstallConfirmDialog, {
      open: true, artifact, error: 'Failed', busy: true,
    }));
    expect(busy).toMatch(/<button[^>]*disabled[^>]*>关闭（保留计划）<\/button>/);
  });
});
