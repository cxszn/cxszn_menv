import { describe, expect, it } from 'vitest';
import {
  CoreError,
  CoreService,
  InMemoryPlanStore,
  InMemoryPathLeaseStore,
  InMemoryStateStore,
  createPathLeaseState,
  type FileSystemProbe,
  type PathLeaseState,
  type PathLeaseStore,
  type ProcessProbe,
} from '../src/index.js';

const executablePath = 'C:\\Tools\\PHP\\php-8.3\\php.exe';

function fsProbe(): FileSystemProbe {
  return {
    exists: () => true,
    isFile: () => true,
    realPath: (value) => value,
    isReparsePoint: () => false,
  };
}

function processProbe(): ProcessProbe {
  return {
    run: async () => ({ exitCode: 0, stdout: 'PHP 8.3.12 (cli) (NTS Visual C++ 2022 x64)\n', stderr: '' }),
  };
}

class BlockingPathLeaseStore implements PathLeaseStore {
  private state: PathLeaseState = createPathLeaseState(null, [], false, null, null);
  private entered: Promise<void>;
  private released: Promise<void>;
  private resolveEntered!: () => void;
  private resolveReleased!: () => void;
  private blockNextWrite = true;

  public constructor() {
    this.entered = new Promise<void>((resolve) => { this.resolveEntered = resolve; });
    this.released = new Promise<void>((resolve) => { this.resolveReleased = resolve; });
  }

  public async read(): Promise<PathLeaseState> {
    return structuredClone(this.state);
  }

  public async write(next: PathLeaseState, expectedHash: string): Promise<PathLeaseState> {
    if (this.state.hash !== expectedHash) throw new CoreError('PATH_CONFLICT', 'path changed');
    if (this.blockNextWrite) {
      this.blockNextWrite = false;
      this.resolveEntered();
      await this.released;
    }
    this.state = structuredClone(next);
    return structuredClone(this.state);
  }

  public waitUntilWriteStarts(): Promise<void> {
    return this.entered;
  }

  public release(): void {
    this.resolveReleased();
  }
}

function service() {
  return new CoreService({
    stateStore: new InMemoryStateStore(),
    pathLeaseStore: new InMemoryPathLeaseStore(),
    fileSystem: fsProbe(),
    processProbe: processProbe(),
    scannerPaths: [executablePath],
    appBinPath: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin',
  });
}

describe('CoreService runtime lifecycle', () => {
  it('discovers PHP as disabled and preserves ownership boundaries', async () => {
    const core = service();
    const state = await core.scanInstalledRuntimes();

    expect(state.runtimes).toHaveLength(1);
    expect(state.runtimes[0]).toMatchObject({
      family: 'php',
      availability: 'detected-unmanaged',
      ownership: 'detected-unmanaged',
      activation: 'disabled',
    });
  });

  it('requires a confirmation token before applying a planned activation', async () => {
    const core = service();
    const state = await core.scanInstalledRuntimes();
    const runtime = state.runtimes[0];
    if (!runtime) throw new Error('expected runtime');

    const plan = await core.planEnvironmentChange({
      action: 'enable',
      runtimeId: runtime.id,
      scope: 'user',
    });

    await expect(
      core.applyEnvironmentChange({ planId: plan.planId, idempotencyKey: 'activate-1' }),
    ).rejects.toMatchObject({ code: 'CONFIRMATION_REQUIRED' });
  });

  it('applies a confirmed user lease without touching real registry state', async () => {
    const core = service();
    const state = await core.scanInstalledRuntimes();
    const runtime = state.runtimes[0];
    if (!runtime) throw new Error('expected runtime');

    const plan = await core.planEnvironmentChange({ action: 'enable', runtimeId: runtime.id, scope: 'user' });
    const token = await core.confirmPlan(plan.planId, 'test-user');
    const operation = await core.applyEnvironmentChange({
      planId: plan.planId,
      confirmationToken: token,
      idempotencyKey: 'activate-2',
      actor: 'test-user',
    });

    expect(operation).toMatchObject({ status: 'succeeded', operationId: expect.any(String) });
    const nextState = await core.getEnvironmentState();
    expect(nextState.runtimes[0]).toMatchObject({ activation: 'active' });
    expect(nextState.pathLease).toMatchObject({ enabled: true, path: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin' });
  });

  it('is idempotent when the same key is retried', async () => {
    const core = service();
    const state = await core.scanInstalledRuntimes();
    const runtime = state.runtimes[0];
    if (!runtime) throw new Error('expected runtime');
    const plan = await core.planEnvironmentChange({ action: 'enable', runtimeId: runtime.id, scope: 'user' });
    const token = await core.confirmPlan(plan.planId, 'test-user');
    const input = { planId: plan.planId, confirmationToken: token, idempotencyKey: 'activate-3', actor: 'test-user' };

    const first = await core.applyEnvironmentChange(input);
    const second = await core.applyEnvironmentChange(input);

    expect(second).toEqual(first);
    expect((await core.getEnvironmentState()).operations).toHaveLength(1);
  });

  it('disabling removes only the app-owned lease and keeps runtime files intact', async () => {
    const core = service();
    const state = await core.scanInstalledRuntimes();
    const runtime = state.runtimes[0];
    if (!runtime) throw new Error('expected runtime');
    const enable = await core.planEnvironmentChange({ action: 'enable', runtimeId: runtime.id, scope: 'user' });
    const token = await core.confirmPlan(enable.planId, 'test-user');
    await core.applyEnvironmentChange({ planId: enable.planId, confirmationToken: token, idempotencyKey: 'activate-4' });

    const disable = await core.planEnvironmentChange({ action: 'disable', runtimeId: runtime.id, scope: 'user' });
    const disableToken = await core.confirmPlan(disable.planId, 'test-user');
    await core.applyEnvironmentChange({ planId: disable.planId, confirmationToken: disableToken, idempotencyKey: 'disable-1' });

    expect((await core.getEnvironmentState()).runtimes[0]).toMatchObject({ activation: 'disabled' });
    expect((await core.getEnvironmentState()).pathLease).toMatchObject({ enabled: false });
  });

  it('rejects an unknown runtime and a stale plan hash', async () => {
    const core = service();
    await expect(core.planEnvironmentChange({ action: 'enable', runtimeId: 'missing', scope: 'user' })).rejects.toMatchObject({
      code: 'RUNTIME_NOT_FOUND',
    });

    const state = await core.scanInstalledRuntimes();
    const runtime = state.runtimes[0];
    if (!runtime) throw new Error('expected runtime');
    const plan = await core.planEnvironmentChange({ action: 'enable', runtimeId: runtime.id, scope: 'user' });
    const token = await core.confirmPlan(plan.planId, 'test-user');
    await expect(
      core.applyEnvironmentChange({ planId: plan.planId, planHash: 'stale', confirmationToken: token, idempotencyKey: 'stale-1' }),
    ).rejects.toMatchObject({ code: 'PLAN_STALE' });
  });

  it('fails closed when a non-PHP family is requested for path verification', async () => {
    const core = service();
    await expect(core.verifyRuntimePath(executablePath, 'python')).rejects.toMatchObject({
      code: 'UNSUPPORTED_RUNTIME',
    });
  });

  it('plans and applies manual registration without copying or enabling the external runtime', async () => {
    const core = service();
    const plan = await core.planEnvironmentChange({ action: 'runtime.add', path: executablePath });

    expect(plan).toMatchObject({
      action: 'runtime.add',
      runtimePath: executablePath,
      runtimeSnapshot: expect.objectContaining({ ownership: 'manual', activation: 'disabled' }),
      requiresConfirmation: true,
    });
    const token = await core.confirmPlan(plan.planId, 'test-user');
    await core.applyEnvironmentChange({ planId: plan.planId, confirmationToken: token, idempotencyKey: 'manual-1' });

    expect((await core.getEnvironmentState()).runtimes[0]).toMatchObject({ ownership: 'manual', availability: 'manual', activation: 'disabled' });
  });

  it('can confirm and apply a plan from another service instance sharing stores', async () => {
    const stateStore = new InMemoryStateStore();
    const planStore = new InMemoryPlanStore();
    const first = new CoreService({ stateStore, planStore, pathLeaseStore: new InMemoryPathLeaseStore(), fileSystem: fsProbe(), processProbe: processProbe(), scannerPaths: [executablePath], appBinPath: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin' });
    await first.scanInstalledRuntimes();
    const runtime = (await first.getEnvironmentState()).runtimes[0];
    if (!runtime) throw new Error('expected runtime');
    const plan = await first.planEnvironmentChange({ action: 'enable', runtimeId: runtime.id });
    const token = await first.confirmPlan(plan.planId, 'test-user');

    const second = new CoreService({ stateStore, planStore, pathLeaseStore: new InMemoryPathLeaseStore(), fileSystem: fsProbe(), processProbe: processProbe(), scannerPaths: [executablePath], appBinPath: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin' });
    const operation = await second.applyEnvironmentChange({ planId: plan.planId, confirmationToken: token, idempotencyKey: 'cross-process-1' });

    expect(operation.status).toBe('succeeded');
  });

  it('persists running state and rolls PATH back when a concurrent operation is cancelled', async () => {
    const pathLeaseStore = new BlockingPathLeaseStore();
    const core = new CoreService({
      stateStore: new InMemoryStateStore(),
      pathLeaseStore,
      fileSystem: fsProbe(),
      processProbe: processProbe(),
      scannerPaths: [executablePath],
      appBinPath: 'C:\\Users\\demo\\AppData\\Local\\cxszn-menv\\bin',
    });
    const runtime = (await core.scanInstalledRuntimes()).runtimes[0];
    if (!runtime) throw new Error('expected runtime');
    const plan = await core.planEnvironmentChange({ action: 'enable', runtimeId: runtime.id, scope: 'user' });
    const token = await core.confirmPlan(plan.planId, 'test-user');

    const applying = core.applyEnvironmentChange({
      planId: plan.planId,
      confirmationToken: token,
      idempotencyKey: 'cancel-running-1',
    });
    await pathLeaseStore.waitUntilWriteStarts();
    const running = (await core.getEnvironmentState()).operations[0];
    if (!running) throw new Error('expected running operation');
    expect(running.status).toBe('running');
    await expect(core.cancelOperation(running.operationId)).resolves.toMatchObject({ status: 'cancelled' });
    pathLeaseStore.release();
    await expect(applying).rejects.toMatchObject({ code: 'OPERATION_CANCELLED' });
    expect((await core.getEnvironmentState()).operations[0]?.status).toBe('cancelled');
    expect((await core.getEnvironmentState()).pathLease.enabled).toBe(false);
  });
});
