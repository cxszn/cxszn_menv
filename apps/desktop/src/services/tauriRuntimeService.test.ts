import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { mapEnvironmentSnapshot, createTauriRuntimeService, TauriRuntimeError } from './tauriRuntimeService';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

describe('Tauri runtime mapping', () => {
  beforeEach(() => invokeMock.mockReset());

  it('keeps discovered PHP disabled and exposes its executable path', () => {
    const snapshot = mapEnvironmentSnapshot({
      runtimes: [{
        id: 'php:win:r:\\app\\env\\php\\php8.5.11\\php.exe',
        family: 'php',
        version: '8.5.11',
        executablePath: 'R:\\App\\Env\\php\\php8.5.11\\php.exe',
        rootPath: 'R:\\App\\Env\\php\\php8.5.11',
        architecture: 'x64',
        threadSafety: 'nts',
        ownership: 'detected-unmanaged',
        availability: 'detected-unmanaged',
        activation: 'disabled',
        health: 'healthy',
        ini: { source: 'none', path: null },
        legacy: false,
        diagnostics: [],
        lastCheckedAt: 'now',
      }],
      pathLease: { enabled: false, runtimeId: null },
      operations: [],
      updatedAt: 'now',
    });

    expect(snapshot.runtimes[0]).toMatchObject({
      version: '8.5.11',
      path: 'R:\\App\\Env\\php\\php8.5.11\\php.exe',
      availability: 'detected-unmanaged',
      activation: 'disabled',
      source: 'detected-unmanaged',
    });
  });

  it('maps Node discovery without presenting PHP TS/NTS details', () => {
    const snapshot = mapEnvironmentSnapshot({
      runtimes: [{
        id: 'node:win:c:\\nodejs\\node.exe',
        family: 'node',
        version: '22.15.0',
        executablePath: 'C:\\nodejs\\node.exe',
        rootPath: 'C:\\nodejs',
        architecture: 'x64',
        threadSafety: 'unknown',
        ownership: 'detected-unmanaged',
        availability: 'detected-unmanaged',
        activation: 'disabled',
        health: 'healthy',
        ini: { source: 'unknown', path: null },
        legacy: false,
        diagnostics: [],
        lastCheckedAt: 'now',
      }],
      pathLease: { enabled: false, runtimeId: null },
      operations: [],
      updatedAt: 'now',
    });

    expect(snapshot.runtimes[0]).toMatchObject({
      family: 'node',
      version: '22.15.0',
      detail: 'x64',
    });
  });

  it('preserves native error code and recovery hints for failed enable operations', async () => {
    invokeMock.mockResolvedValue({
      schema_version: '1',
      request_id: 'request-1',
      ok: false,
      error: {
        code: 'PLATFORM_ERROR',
        message: 'create PHP shim hard link (target and bin must share a volume)',
        retryable: false,
        next_actions: ['Retry after changing the shim volume.'],
      },
    });

    await expect(createTauriRuntimeService().getSnapshot()).rejects.toMatchObject({
      name: 'TauriRuntimeError',
      code: 'PLATFORM_ERROR',
      retryable: false,
      nextActions: ['Retry after changing the shim volume.'],
    } satisfies Partial<TauriRuntimeError>);
  });

  it('maps the verified PHP catalog without treating artifacts as installed', async () => {
    invokeMock.mockResolvedValueOnce({
      schema_version: '1',
      request_id: 'catalog-1',
      ok: true,
      result: {
        artifacts: [{
          artifact_id: 'php-8.4.26-nts-x64',
          runtime_family: 'php',
          version: '8.4.26',
          arch: 'x86_64',
          thread_safety: 'nts',
          lifecycle: 'supported',
          file_name: 'php-8.4.26.zip',
          size_bytes: 123,
        }],
      },
    }).mockResolvedValueOnce({
      schema_version: '1',
      request_id: 'state-1',
      ok: true,
      result: {
        runtimes: [],
        pathLease: { enabled: false, runtimeId: null },
        operations: [],
        updatedAt: 'now',
      },
    });

    await expect(createTauriRuntimeService().listPhpCatalog()).resolves.toEqual([{
      artifactId: 'php-8.4.26-nts-x64',
      family: 'php',
      version: '8.4.26',
      architecture: 'x64',
      threadSafety: 'nts',
      lifecycle: 'supported',
      legacy: false,
      archival: false,
      available: true,
      installed: false,
      fileName: 'php-8.4.26.zip',
      sizeBytes: 123,
    }]);
  });

  it('maps the native Rust catalog summary shape returned as an array', async () => {
    invokeMock.mockResolvedValueOnce({
      schema_version: '1',
      request_id: 'catalog-native-1',
      ok: true,
      result: [{
        family: 'php',
        artifactId: 'php-8.5.11-nts-x64',
        version: '8.5.11',
        architecture: 'x86_64',
        threadSafety: 'nts',
        lifecycle: 'supported',
        legacy: false,
        archival: false,
        available: true,
        installed: false,
        fileName: 'php-8.5.11-nts-Win32-vs17-x64.zip',
        sizeBytes: 36048897,
      }],
    }).mockResolvedValueOnce({
      schema_version: '1',
      request_id: 'state-native-1',
      ok: true,
      result: {
        runtimes: [],
        pathLease: { enabled: false, runtimeId: null },
        operations: [],
        updatedAt: 'now',
      },
    });

    await expect(createTauriRuntimeService().listPhpCatalog()).resolves.toMatchObject([{
      artifactId: 'php-8.5.11-nts-x64',
      family: 'php',
      version: '8.5.11',
      architecture: 'x64',
      threadSafety: 'nts',
      installed: false,
    }]);
  });

  it.each([
    ['ts', false],
    ['nts', true],
    ['unknown', false],
  ] as const)('matches the catalog NTS build against local %s without merging variants', async (threadSafety, installed) => {
    invokeMock.mockResolvedValueOnce({
      schema_version: '1', request_id: 'catalog-variant', ok: true,
      result: [{
        artifactId: 'php-8.5.11-nts-x64', family: 'php', version: '8.5.11',
        architecture: 'x64', threadSafety: 'nts', lifecycle: 'supported',
      }],
    }).mockResolvedValueOnce({
      schema_version: '1', request_id: 'state-variant', ok: true,
      result: {
        runtimes: [{
          id: 'php:win:r:\\php\\php.exe', family: 'php', version: '8.5.11',
          executablePath: 'R:\\php\\php.exe', rootPath: 'R:\\php',
          architecture: 'x64', threadSafety, ownership: 'detected-unmanaged',
          availability: 'detected-unmanaged', activation: 'disabled', health: 'healthy',
          ini: { source: 'none', path: null }, legacy: false, diagnostics: [], lastCheckedAt: 'now',
        }],
        pathLease: { enabled: false, runtimeId: null }, operations: [], updatedAt: 'now',
      },
    });

    const artifacts = await createTauriRuntimeService().listPhpCatalog();
    expect(artifacts).toHaveLength(1);
    expect(artifacts).toMatchObject([{ installed }]);
  });

  it('exposes the approved ini source and preserves its binding through confirmation', async () => {
    const runtime = {
      id: 'php-ini-runtime', family: 'php', version: '8.5.11', executablePath: 'R:\\PHP\\php.exe', rootPath: 'R:\\PHP',
      architecture: 'x64', threadSafety: 'ts', ownership: 'manual', availability: 'manual', activation: 'disabled', health: 'healthy',
      ini: { source: 'explicit', path: 'R:\\Config\\php.ini' }, legacy: false, diagnostics: [], lastCheckedAt: 'now',
    };
    const binding = { path: 'R:\\Config\\php.ini', sha256: 'a'.repeat(64) };
    const plan = { planId: 'ini-plan', planHash: 'bound-hash', request: { actor: 'desktop-ui' }, expiresAtEpochSeconds: 9999999999, requiresConfirmation: true, backupSource: binding };
    invokeMock.mockResolvedValueOnce({ ok: true, result: { runtimes: [runtime], pathLease: { enabled: false }, operations: [], updatedAt: 'now' } })
      .mockResolvedValueOnce({ ok: true, result: plan })
      .mockResolvedValueOnce({ ok: true, result: { value: 'test-token', planId: plan.planId, planHash: plan.planHash, actor: plan.request.actor, expiresAtEpochSeconds: plan.expiresAtEpochSeconds } })
      .mockResolvedValueOnce({ ok: true, result: { runtimeId: runtime.id, sourcePath: binding.path, backupPath: 'C:\\MEnv\\backup.ini' } })
      .mockResolvedValueOnce({ ok: true, result: { runtimes: [runtime], pathLease: { enabled: false }, operations: [], updatedAt: 'now' } });
    const service = createTauriRuntimeService();
    await expect(service.planPhpIniBackup(runtime.id)).resolves.toMatchObject({ sourcePath: binding.path, sourceSha256: binding.sha256 });
    await service.applyPhpIniBackup(plan.planId);
    expect(invokeMock.mock.calls.find(([command]) => command === 'apply_php_ini_backup')?.[1]).toMatchObject({ plan: { backupSource: binding } });
  });

  it('recovers an unexpired persisted PHP install plan for explicit confirmation', async () => {
    const plan = {
      planId: 'recovered-install-plan',
      planHash: 'recovered-hash',
      request: { actor: 'desktop-ui' },
      expiresAtEpochSeconds: 9999999999,
      requiresConfirmation: true,
    };
    invokeMock.mockResolvedValueOnce({
      schema_version: '1',
      request_id: 'recovered-plan',
      ok: true,
      result: [{
        plan,
        artifact: {
          family: 'php',
          artifactId: 'php-8.5.11-nts-x64',
          version: '8.5.11',
          architecture: 'x86_64',
          threadSafety: 'nts',
          lifecycle: 'supported',
          legacy: false,
          archival: false,
          installed: false,
          available: true,
          fileName: 'php-8.5.11-nts-Win32-vs17-x64.zip',
          sizeBytes: 36048897,
        },
      }],
    });

    await expect(createTauriRuntimeService().listPhpInstallPlans()).resolves.toEqual([{
      planId: 'recovered-install-plan',
      artifact: {
        artifactId: 'php-8.5.11-nts-x64',
        family: 'php',
        version: '8.5.11',
        architecture: 'x64',
        threadSafety: 'nts',
        lifecycle: 'supported',
        legacy: false,
        archival: false,
        installed: false,
        available: true,
        fileName: 'php-8.5.11-nts-Win32-vs17-x64.zip',
        sizeBytes: 36048897,
      },
    }]);
    expect(invokeMock).toHaveBeenCalledWith('list_php_install_plans', undefined);
  });

  it('preserves catalog install error codes and keeps the plan retryable', async () => {
    const artifact = {
      artifact_id: 'php-8.5.11-nts-x64',
      runtime_family: 'php',
      version: '8.5.11',
      arch: 'x86_64',
      thread_safety: 'nts',
      lifecycle: 'supported',
      file_name: 'php-8.5.11-nts-Win32-vs17-x64.zip',
      size_bytes: 36048897,
    };
    const plan = {
      planId: 'install-plan-1',
      planHash: 'hash-1',
      request: { actor: 'desktop-ui' },
      expiresAtEpochSeconds: 9999999999,
      requiresConfirmation: true,
    };
    invokeMock
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'catalog', ok: true, result: [artifact] })
      .mockResolvedValueOnce({
        schema_version: '1', request_id: 'state', ok: true,
        result: { runtimes: [], pathLease: { enabled: false, runtimeId: null }, operations: [], updatedAt: 'now' },
      })
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'plan', ok: true, result: plan })
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'token', ok: true, result: { value: 'token', ...plan } })
      .mockResolvedValueOnce({
        schema_version: '1', request_id: 'apply', ok: false,
        error: {
          code: 'HASH_MISMATCH',
          message: 'download SHA-256 does not match the signed catalog',
          retryable: false,
          next_actions: [],
        },
      })
      .mockResolvedValueOnce({
        schema_version: '1', request_id: 'apply-retry', ok: false,
        error: {
          code: 'HASH_MISMATCH',
          message: 'download SHA-256 does not match the signed catalog',
          retryable: false,
          next_actions: [],
        },
      });

    const service = createTauriRuntimeService();
    await service.planPhpInstall('php-8.5.11-nts-x64');
    await expect(service.applyPhpInstall('install-plan-1')).rejects.toMatchObject({
      name: 'TauriRuntimeError',
      code: 'HASH_MISMATCH',
    } satisfies Partial<TauriRuntimeError>);
    await expect(service.applyPhpInstall('install-plan-1')).rejects.toMatchObject({
      code: 'HASH_MISMATCH',
    } satisfies Partial<TauriRuntimeError>);
  });

  it('cancels a recovered install plan and repeats cancellation through the native service', async () => {
    const plan = {
      planId: 'cancel-install-plan', planHash: 'hash', request: { actor: 'desktop-ui' },
      expiresAtEpochSeconds: 9999999999, requiresConfirmation: true,
    };
    invokeMock.mockResolvedValueOnce({ ok: true, result: [{
      plan,
      artifact: { artifactId: 'php-8.5.11-nts-x64', family: 'php', version: '8.5.11', architecture: 'x64', threadSafety: 'nts', lifecycle: 'supported' },
    }] }).mockResolvedValue({ ok: true, result: null });
    const service = createTauriRuntimeService();
    await service.listPhpInstallPlans();

    await expect(service.cancelPhpInstall(plan.planId)).resolves.toBeUndefined();
    await expect(service.applyPhpInstall(plan.planId)).rejects.toThrow('PLAN_REQUIRED');
    await expect(service.cancelPhpInstall(plan.planId)).resolves.toBeUndefined();

    expect(invokeMock.mock.calls).toEqual([
      ['list_php_install_plans', undefined],
      ['cancel_php_install', { planId: plan.planId }],
      ['cancel_php_install', { planId: plan.planId }],
    ]);
  });

  it('retains an install plan and native error details when cancellation fails', async () => {
    const plan = {
      planId: 'retained-install-plan', planHash: 'hash', request: { actor: 'desktop-ui' },
      expiresAtEpochSeconds: 9999999999, requiresConfirmation: true,
    };
    invokeMock.mockResolvedValueOnce({ ok: true, result: [{
      plan,
      artifact: { artifactId: 'php-8.5.11-nts-x64', family: 'php', version: '8.5.11', architecture: 'x64', threadSafety: 'nts', lifecycle: 'supported' },
    }] }).mockResolvedValueOnce({ ok: false, error: {
      code: 'OPERATION_BUSY', message: 'another operation is running', retryable: true, next_actions: [],
    } }).mockResolvedValueOnce({ ok: true, result: { value: 'token', ...plan } })
      .mockResolvedValueOnce({ ok: true, result: null })
      .mockResolvedValueOnce({ ok: true, result: {
        runtimes: [], pathLease: { enabled: false, runtimeId: null }, operations: [], updatedAt: 'now',
      } });
    const service = createTauriRuntimeService();
    await service.listPhpInstallPlans();

    await expect(service.cancelPhpInstall(plan.planId)).rejects.toMatchObject({ code: 'OPERATION_BUSY', retryable: true });
    await expect(service.applyPhpInstall(plan.planId)).resolves.toMatchObject({ runtimes: [] });
    expect(invokeMock).toHaveBeenCalledWith('apply_php_install', { plan, confirmationToken: { value: 'token', ...plan } });
  });

  it('lets the native service decide cancellation for an uncached plan', async () => {
    invokeMock.mockResolvedValue({ ok: false, error: {
      code: 'OPERATION_NOT_CANCELLABLE', message: 'install has started', retryable: false, next_actions: [],
    } });
    await expect(createTauriRuntimeService().cancelPhpInstall('uncached-plan')).rejects.toMatchObject({ code: 'OPERATION_NOT_CANCELLABLE' });
    expect(invokeMock).toHaveBeenCalledWith('cancel_php_install', { planId: 'uncached-plan' });
  });

  it('uses the fixed terminal command contract for session lifecycle', async () => {
    invokeMock
      .mockResolvedValueOnce({
        schema_version: '1', request_id: 'terminal-start', ok: true,
        result: { sessionId: 'session-1', cwd: 'C:\\Projects\\demo' },
      })
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'terminal-input', ok: true, result: null })
      .mockResolvedValueOnce({
        schema_version: '1', request_id: 'terminal-poll', ok: true,
        result: [{ kind: 'output', text: 'PHP 8.5.11\r\n' }],
      })
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'terminal-close', ok: true, result: null });

    const service = createTauriRuntimeService();
    await expect(service.startTerminal('C:\\Projects\\demo')).resolves.toEqual({
      sessionId: 'session-1', cwd: 'C:\\Projects\\demo',
    });
    await service.sendTerminalInput('session-1', 'php -v\r\n');
    await expect(service.pollTerminal('session-1')).resolves.toEqual([{ kind: 'output', text: 'PHP 8.5.11\r\n' }]);
    await service.closeTerminal('session-1');

    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'terminal_start', 'terminal_send_input', 'terminal_poll', 'terminal_close',
    ]);
  });

  it('plans and applies a confirmed manual PHP path registration', async () => {
    const plan = {
      planId: 'register-plan-1',
      planHash: 'hash-1',
      request: { actor: 'desktop-ui' },
      expiresAtEpochSeconds: 9999999999,
      requiresConfirmation: true,
    };
    const runtime = {
      id: 'php:win:d:\\tools\\php.exe',
      family: 'php',
      version: '8.5.11',
      executablePath: 'D:\\tools\\php.exe',
      rootPath: 'D:\\tools',
      architecture: 'x64',
      threadSafety: 'nts',
      ownership: 'manual',
      availability: 'manual',
      activation: 'disabled',
      health: 'healthy',
      ini: { source: 'none', path: null },
      legacy: false,
      diagnostics: [],
      lastCheckedAt: 'now',
    } as const;
    invokeMock
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'register-plan', ok: true, result: { plan, runtime } })
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'token', ok: true, result: { value: 'token', ...plan } })
      .mockResolvedValueOnce({ schema_version: '1', request_id: 'register-apply', ok: true, result: {} })
      .mockResolvedValueOnce({
        schema_version: '1', request_id: 'state', ok: true,
        result: { runtimes: [runtime], pathLease: { enabled: false, runtimeId: null }, operations: [], updatedAt: 'now' },
      });

    const service = createTauriRuntimeService();
    await expect(service.planPhpRegister('D:\\tools\\php.exe')).resolves.toMatchObject({
      planId: 'register-plan-1',
      runtime: { path: 'D:\\tools\\php.exe', source: 'manual', activation: 'disabled' },
    });
    await expect(service.applyPhpRegister('register-plan-1')).resolves.toMatchObject({
      runtimes: [{ path: 'D:\\tools\\php.exe', source: 'manual', activation: 'disabled' }],
    });
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'plan_php_path_registration', 'issue_confirmation_token', 'apply_php_path_registration', 'get_environment_state',
    ]);
  });
});
