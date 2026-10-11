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
});
