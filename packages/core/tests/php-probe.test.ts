import { describe, expect, it } from 'vitest';
import {
  CoreError,
  type FileSystemProbe,
  type ProcessProbe,
  probePhpExecutable,
  validateWindowsExecutablePath,
} from '../src/index.js';

function createProbe(overrides: Partial<FileSystemProbe> = {}): FileSystemProbe {
  return {
    exists: () => true,
    isFile: () => true,
    realPath: (value) => value,
    isReparsePoint: () => false,
    ...overrides,
  };
}

function createRunner(stdout: string, overrides: Partial<ProcessProbe> = {}): ProcessProbe {
  return {
    run: async () => ({ exitCode: 0, stdout, stderr: '' }),
    ...overrides,
  };
}

describe('validateWindowsExecutablePath', () => {
  it('accepts an absolute local php.exe path and returns a normalized path', () => {
    const result = validateWindowsExecutablePath('C:\\Tools\\PHP\\php.exe', createProbe());

    expect(result).toEqual({
      path: 'C:\\Tools\\PHP\\php.exe',
      normalizedPath: 'C:\\Tools\\PHP\\php.exe',
    });
  });

  it('rejects relative, UNC and non-php executable paths', () => {
    expect(() => validateWindowsExecutablePath('php.exe', createProbe())).toThrowError(CoreError);
    expect(() => validateWindowsExecutablePath('\\\\server\\share\\php.exe', createProbe())).toThrowError(
      expect.objectContaining({ code: 'PATH_NOT_ALLOWED' }),
    );
    expect(() => validateWindowsExecutablePath('C:\\Tools\\PHP\\php.cmd', createProbe())).toThrowError(
      expect.objectContaining({ code: 'INVALID_RUNTIME_PATH' }),
    );
  });

  it('rejects paths that resolve through a reparse point', () => {
    expect(() =>
      validateWindowsExecutablePath(
        'C:\\Tools\\PHP\\php.exe',
        createProbe({ isReparsePoint: () => true }),
      ),
    ).toThrowError(expect.objectContaining({ code: 'PATH_NOT_ALLOWED' }));
  });
});

describe('probePhpExecutable', () => {
  it('parses PHP version, architecture, thread safety and ini source', async () => {
    const runner = createRunner(
      'PHP 8.3.12 (cli) (built: Sep 25 2026 12:00:00) (NTS Visual C++ 2022 x64)\nCopyright (c) The PHP Group\n',
    );
    const result = await probePhpExecutable(
      'C:\\Tools\\PHP\\php.exe',
      createProbe(),
      runner,
    );

    expect(result).toMatchObject({
      family: 'php',
      version: '8.3.12',
      architecture: 'x64',
      threadSafety: 'nts',
      executablePath: 'C:\\Tools\\PHP\\php.exe',
      health: 'healthy',
      ini: expect.objectContaining({ source: 'unknown' }),
    });
  });

  it('uses php -n -v and does not load project extensions during a health probe', async () => {
    const calls: string[][] = [];
    const runner: ProcessProbe = {
      run: async (_executable, args) => {
        calls.push(args);
        return { exitCode: 0, stdout: 'PHP 5.6.40 (cli) (NTS x86)\n', stderr: '' };
      },
    };

    await probePhpExecutable('C:\\Legacy\\php.exe', createProbe(), runner);

    expect(calls[0]).toEqual(['-n', '-v']);
    expect(calls[1]).toEqual(['--ini']);
  });

  it('reports a failed health result when the probe exits non-zero', async () => {
    const result = await probePhpExecutable(
      'C:\\Broken\\php.exe',
      createProbe(),
      createRunner('', { run: async () => ({ exitCode: 1, stdout: '', stderr: 'missing DLL' }) }),
    );

    expect(result).toMatchObject({
      health: 'error',
      version: null,
      diagnostics: expect.arrayContaining([expect.stringContaining('missing DLL')]),
    });
  });

  it('marks PHP 6 and non-existent 5.7 lines unsupported instead of treating them as supported', async () => {
    const php6 = await probePhpExecutable(
      'C:\\Legacy\\php.exe',
      createProbe(),
      createRunner('PHP 6.0.0 (cli) (NTS x64)\\n'),
    );
    const php57 = await probePhpExecutable(
      'C:\\Legacy\\php-57.exe',
      createProbe(),
      createRunner('PHP 5.7.0 (cli) (NTS x64)\\n'),
    );

    expect(php6).toMatchObject({ availability: 'unsupported', health: 'warning' });
    expect(php57).toMatchObject({ availability: 'unsupported', health: 'warning' });
  });
});
