import { describe, expect, it } from 'vitest';
import { runMcpServe, type McpServeRunner, type McpServeDependencies } from '../src/mcp-entry.js';
import type { CliCorePort } from '../src/index.js';

function fakeCore(): CliCorePort {
  return {
    getEnvironmentState: async () => ({ runtimes: [] }),
    scanInstalledRuntimes: async () => ({ runtimes: [] }),
    listRuntimeCatalog: async () => ({ entries: [] }),
    verifyRuntimePath: async (path: string) => ({ path, valid: true }),
    planEnvironmentChange: async (input: unknown) => ({ input }),
    confirmPlan: async (planId: string) => ({ planId, confirmationToken: 'token' }),
    applyEnvironmentChange: async (input: { idempotencyKey: string }) => ({ operationId: input.idempotencyKey }),
    getOperationStatus: async (operationId: string) => ({ operationId, status: 'succeeded' }),
    cancelOperation: async (operationId: string) => ({ operationId, status: 'cancelled' }),
  };
}

describe('MCP CLI entry', () => {
  it('starts the local stdio MCP runner without adding a shell escape hatch', async () => {
    const calls: unknown[] = [];
    const runServer: McpServeRunner = async (options) => {
      calls.push(options.service);
    };

    const code = await runMcpServe(
      ['mcp', 'serve', '--transport', 'stdio'],
      { core: fakeCore(), runServer },
    );

    expect(code).toBe(0);
    expect(calls).toHaveLength(1);
  });

  it('rejects non-stdio transports and unknown flags before starting MCP', async () => {
    const calls: unknown[] = [];
    const logs: string[] = [];
    const dependencies: McpServeDependencies = {
      core: fakeCore(),
      runServer: async () => {
        calls.push('started');
      },
      logger: (message) => logs.push(message),
    };

    await expect(runMcpServe(['mcp', 'serve', '--transport', 'http'], dependencies)).resolves.toBe(2);
    await expect(runMcpServe(['mcp', 'serve', '--script', 'whoami'], dependencies)).resolves.toBe(2);
    await expect(runMcpServe([], dependencies)).resolves.toBe(2);
    expect(calls).toHaveLength(0);
    expect(logs.every((message) => message.includes('stdio') || message.includes('Unknown') || message.includes('Expected'))).toBe(true);
  });
});
