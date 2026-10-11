import { describe, expect, it } from 'vitest';
import { runCli, type CliCorePort } from '../src/index.js';

type Call = { method: string; input?: unknown };

function createFakeCore(overrides: Partial<CliCorePort> = {}) {
  const calls: Call[] = [];
  const core: CliCorePort = {
    getEnvironmentState: async () => {
      calls.push({ method: 'getEnvironmentState' });
      return { runtimes: [], active: null };
    },
    scanInstalledRuntimes: async (input?: unknown) => {
      calls.push(input === undefined ? { method: 'scanInstalledRuntimes' } : { method: 'scanInstalledRuntimes', input });
      return { runtimes: [{ id: 'php-8.3', activation: 'disabled' }] };
    },
    listRuntimeCatalog: async (kind?: string) => {
      calls.push({ method: 'listRuntimeCatalog', input: kind });
      return { kind: kind ?? 'all', entries: [] };
    },
    verifyRuntimePath: async (path: string, _family?: string) => {
      calls.push({ method: 'verifyRuntimePath', input: path });
      return { path, valid: true };
    },
    planEnvironmentChange: async (input: unknown) => {
      calls.push({ method: 'planEnvironmentChange', input });
      return { planId: 'plan-1', planHash: 'hash-1', changes: input };
    },
    confirmPlan: async (planId: string, actor?: string) => {
      calls.push({ method: 'confirmPlan', input: { planId, actor } });
      return { planId, confirmationToken: 'token-1' };
    },
    applyEnvironmentChange: async (input: unknown) => {
      calls.push({ method: 'applyEnvironmentChange', input });
      return { operationId: 'op-1', status: 'queued' };
    },
    getOperationStatus: async (operationId: string) => {
      calls.push({ method: 'getOperationStatus', input: operationId });
      return { operationId, status: 'succeeded' };
    },
    cancelOperation: async (operationId: string) => {
      calls.push({ method: 'cancelOperation', input: operationId });
      return { operationId, status: 'cancelled' };
    },
    ...overrides,
  };
  return { core, calls };
}

function runJson(argv: string[], core: CliCorePort) {
  const output: string[] = [];
  return runCli(argv, {
    core,
    requestId: () => 'request-1',
    write: (line) => output.push(line),
  }).then((exitCode) => ({ exitCode, output: JSON.parse(output.join('')) as Record<string, unknown> }));
}

describe('CLI JSON protocol', () => {
  it('returns the environment state through doctor without duplicating core logic', async () => {
    const { core, calls } = createFakeCore();

    const result = await runJson(['doctor', '--json'], core);

    expect(result.exitCode).toBe(0);
    expect(result.output).toEqual({
      schema_version: '1',
      request_id: 'request-1',
      ok: true,
      result: { runtimes: [], active: null },
    });
    expect(calls).toEqual([{ method: 'getEnvironmentState' }]);
  });

  it('routes scan, catalog and path verification to the core port', async () => {
    const { core, calls } = createFakeCore();

    const scan = await runJson(['runtime', 'scan', '--json'], core);
    const catalog = await runJson(['catalog', 'list', '--kind', 'php', '--json'], core);
    const verify = await runJson(['runtime', 'verify', '--path', 'C:\\php\\php.exe', '--json'], core);

    expect(scan.output.ok).toBe(true);
    expect(catalog.output.result).toEqual({ kind: 'php', entries: [] });
    expect(verify.output.result).toEqual({ path: 'C:\\php\\php.exe', valid: true });
    expect(calls.map((call) => call.method)).toEqual([
      'scanInstalledRuntimes',
      'listRuntimeCatalog',
      'verifyRuntimePath',
    ]);
  });

  it('accepts family as the canonical alias for runtime filters', async () => {
    const { core, calls } = createFakeCore();

    await runJson(['runtime', 'scan', '--family', 'php', '--json'], core);
    await runJson(['catalog', 'list', '--family', 'php', '--json'], core);
    await runJson(['runtime', 'verify', '--path', 'C:\\php\\php.exe', '--family', 'php', '--json'], core);

    expect(calls).toEqual([
      { method: 'scanInstalledRuntimes', input: { runtimeFamily: 'php' } },
      { method: 'listRuntimeCatalog', input: 'php' },
      { method: 'verifyRuntimePath', input: 'C:\\php\\php.exe' },
    ]);
  });

  it('creates a plan for runtime add and never applies a write implicitly', async () => {
    const { core, calls } = createFakeCore();

    const result = await runJson(['runtime', 'add', '--path', 'C:\\php\\php.exe', '--json'], core);

    expect(result.exitCode).toBe(0);
    expect(result.output.ok).toBe(true);
    expect(result.output.result).toEqual({
      planId: 'plan-1',
      planHash: 'hash-1',
      changes: { action: 'runtime.add', path: 'C:\\php\\php.exe' },
    });
    expect(calls.map((call) => call.method)).toEqual(['planEnvironmentChange']);
  });

  it('uses core enable/disable actions for runtime activation commands', async () => {
    const planned: unknown[] = [];
    const { core } = createFakeCore({
      planEnvironmentChange: async (input) => {
        planned.push(input);
        return { planId: 'plan-1', planHash: 'hash-1', changes: input };
      },
    });

    await runJson(['runtime', 'enable', '--runtime-id', 'php-8.3', '--json'], core);
    await runJson(['runtime', 'disable', '--runtime-id', 'php-8.3', '--json'], core);

    expect(planned).toEqual([
      { action: 'enable', runtimeId: 'php-8.3' },
      { action: 'disable', runtimeId: 'php-8.3' },
    ]);
  });

  it('does not let activation commands register a path implicitly', async () => {
    const { core, calls } = createFakeCore();
    const result = await runJson(['runtime', 'enable', '--path', 'C:\\php\\php.exe', '--json'], core);

    expect(result.exitCode).toBe(2);
    expect(result.output).toMatchObject({ ok: false, error: { code: 'INVALID_ARGUMENT' } });
    expect(calls).toEqual([]);
  });

  it('passes target runtime and actor fields through change planning', async () => {
    const planned: unknown[] = [];
    const { core } = createFakeCore({
      planEnvironmentChange: async (input) => {
        planned.push(input);
        return { planId: 'plan-1', planHash: 'hash-1', changes: input };
      },
    });

    await runJson([
      'change',
      'plan',
      '--action',
      'switch',
      '--runtime-id',
      'php-8.2',
      '--target-runtime-id',
      'php-8.3',
      '--actor',
      'codex',
      '--json',
    ], core);

    expect(planned).toEqual([
      { action: 'switch', runtimeId: 'php-8.2', targetRuntimeId: 'php-8.3', actor: 'codex' },
    ]);
  });

  it('requires plan hash, confirmation token and idempotency key before apply', async () => {
    const { core, calls } = createFakeCore();

    const result = await runJson(['change', 'apply', '--plan-id', 'plan-1', '--json'], core);

    expect(result.exitCode).toBe(3);
    expect(result.output.ok).toBe(false);
    expect(result.output.error).toMatchObject({
      code: 'CONFIRMATION_REQUIRED',
      retryable: false,
    });
    expect(calls).toEqual([]);
  });

  it('applies an already confirmed plan with a caller-supplied idempotency key', async () => {
    const { core, calls } = createFakeCore();

    const result = await runJson([
      'change',
      'apply',
      '--plan-id',
      'plan-1',
      '--plan-hash',
      'hash-1',
      '--confirmation-token',
      'token-1',
      '--idempotency-key',
      'retry-1',
      '--actor',
      'codex',
      '--json',
    ], core);

    expect(result.exitCode).toBe(0);
    expect(result.output.result).toEqual({ operationId: 'op-1', status: 'queued' });
    expect(calls).toEqual([
      {
        method: 'applyEnvironmentChange',
        input: {
          planId: 'plan-1',
          planHash: 'hash-1',
          confirmationToken: 'token-1',
          idempotencyKey: 'retry-1',
          actor: 'codex',
        },
      },
    ]);
  });

  it('accepts --plan as an alias for --plan-id', async () => {
    const { core, calls } = createFakeCore();

    const result = await runJson([
      'change',
      'apply',
      '--plan',
      'plan-1',
      '--confirmation-token',
      'token-1',
      '--idempotency-key',
      'retry-1',
      '--json',
    ], core);

    expect(result.exitCode).toBe(0);
    expect(calls[0]).toMatchObject({ method: 'applyEnvironmentChange', input: { planId: 'plan-1' } });
  });

  it('routes operation status and cancellation without exposing a shell escape hatch', async () => {
    const { core, calls } = createFakeCore();

    const status = await runJson(['operation', 'get', '--id', 'op-1', '--json'], core);
    const cancelled = await runJson(['operation', 'cancel', '--id', 'op-1', '--json'], core);
    const rejected = await runJson(['run-shell', 'whoami', '--json'], core);

    expect(status.output.result).toEqual({ operationId: 'op-1', status: 'succeeded' });
    expect(cancelled.output.result).toEqual({ operationId: 'op-1', status: 'cancelled' });
    expect(rejected.exitCode).toBe(2);
    expect(rejected.output.error).toMatchObject({ code: 'UNKNOWN_COMMAND' });
    expect(calls.map((call) => call.method)).toEqual(['getOperationStatus', 'cancelOperation']);
  });

  it('returns a stable error envelope for missing arguments', async () => {
    const { core } = createFakeCore();

    const result = await runJson(['runtime', 'verify', '--json'], core);

    expect(result.exitCode).toBe(2);
    expect(result.output).toMatchObject({
      schema_version: '1',
      request_id: 'request-1',
      ok: false,
      error: {
        code: 'INVALID_ARGUMENT',
        retryable: false,
      },
    });
  });
});
