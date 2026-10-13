import { describe, expect, it } from 'vitest';
import { createMockRuntimeService } from './mockRuntimeService';

describe('mock runtime service', () => {
  it('keeps a scan read-only with respect to activation', async () => {
    const service = createMockRuntimeService();
    const before = await service.getSnapshot();
    const after = await service.scan();

    expect(after.runtimes.map((runtime) => runtime.activation)).toEqual(
      before.runtimes.map((runtime) => runtime.activation),
    );
    expect(after.activities[0]?.title).toBe('环境扫描完成');
  });

  it('applies a plan to the requested runtime and preserves the family boundary', async () => {
    const service = createMockRuntimeService();
    const before = await service.getSnapshot();
    const python = before.runtimes.find((runtime) => runtime.family === 'python');

    expect(python).toBeDefined();
    const plan = await service.planEnable(python!.id);
    const after = await service.applyEnable(plan.planId);

    expect(after.runtimes.find((runtime) => runtime.id === python!.id)?.activation).toBe('active');
    expect(after.runtimes.find((runtime) => runtime.id === 'php-8.3-x64-nts')?.activation).toBe('active');
  });

  it('lists PHP catalog entries and keeps a downloaded version disabled', async () => {
    const service = createMockRuntimeService();
    const catalog = await service.listPhpCatalog();
    const artifact = catalog.find((candidate) => candidate.version === '8.4.26');
    const legacy = catalog.find((candidate) => candidate.version === '7.4.33');

    expect(artifact).toMatchObject({ family: 'php', available: true, installed: false });
    expect(legacy).toMatchObject({ family: 'php', lifecycle: 'eol', legacy: true, available: true });
    const plan = await service.planPhpInstall(artifact!.artifactId);
    const after = await service.applyPhpInstall(plan.planId);
    const runtime = after.runtimes.find((candidate) => candidate.version === '8.4.26');

    expect(runtime).toMatchObject({ availability: 'managed', activation: 'disabled', family: 'php' });
    expect(after.runtimes.find((candidate) => candidate.activation === 'active')?.version).toBe('8.3.14');
    expect((await service.listPhpCatalog()).find((candidate) => candidate.version === '8.4.26')?.installed).toBe(true);
  });

  it('cancels a pending install idempotently without changing the environment', async () => {
    const service = createMockRuntimeService();
    const before = await service.getSnapshot();
    const plan = await service.planPhpInstall('php-8.4.26-nts-x64');
    expect(await service.listPhpInstallPlans()).toEqual([plan]);

    await expect(service.cancelPhpInstall(plan.planId)).resolves.toBeUndefined();
    await expect(service.cancelPhpInstall(plan.planId)).resolves.toBeUndefined();
    await expect(service.listPhpInstallPlans()).resolves.toEqual([]);
    await expect(service.applyPhpInstall(plan.planId)).rejects.toThrow('PLAN_CANCELLED');
    await expect(service.getSnapshot()).resolves.toEqual(before);
    expect((await service.listPhpCatalog()).find((artifact) => artifact.version === '8.4.26')?.installed).toBe(false);

    const replacement = await service.planPhpInstall('php-8.4.26-nts-x64');
    expect(replacement.planId).not.toBe(plan.planId);
    await expect(service.applyPhpInstall(replacement.planId)).resolves.toMatchObject({
      runtimes: expect.arrayContaining([expect.objectContaining({ version: '8.4.26', activation: 'disabled' })]),
    });
  });

  it('rejects cancellation of completed or missing install plans', async () => {
    const service = createMockRuntimeService();
    const plan = await service.planPhpInstall('php-8.4.26-nts-x64');
    await service.applyPhpInstall(plan.planId);

    await expect(service.cancelPhpInstall(plan.planId)).rejects.toThrow('OPERATION_NOT_CANCELLABLE');
    await expect(service.cancelPhpInstall('missing-plan')).rejects.toThrow('PLAN_REQUIRED');
  });
});
