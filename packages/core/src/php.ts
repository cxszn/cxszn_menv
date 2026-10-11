import { win32 as winPath } from 'node:path';
import { CoreError } from './errors.js';
import { normalizeWindowsPath, sha256 } from './hash.js';
import type {
  FileSystemProbe,
  ProcessProbe,
  RuntimeArchitecture,
  RuntimeHealth,
  RuntimeRecord,
  PhpThreadSafety,
  RuntimeIniInfo,
  RuntimeAvailability,
} from './types.js';

export interface PhpProbeResult extends RuntimeRecord {
  family: 'php';
}

export interface ValidatedExecutablePath {
  path: string;
  normalizedPath: string;
}

function isLocalWindowsAbsolutePath(value: string): boolean {
  return /^[a-zA-Z]:[\\/]/.test(value) && !value.startsWith('\\\\');
}

function isPhpCliName(value: string): boolean {
  const name = winPath.basename(value).toLowerCase();
  return /^php(?:[-_.][a-z0-9_.-]+)?\.exe$/.test(name);
}

/**
 * Validate a user-provided runtime executable without touching the executable.
 * The adapter intentionally rejects UNC/device paths and reparse points so
 * callers cannot silently register a path outside the selected local tree.
 */
export function validateWindowsExecutablePath(path: string, fileSystem: FileSystemProbe): ValidatedExecutablePath {
  if (typeof path !== 'string' || path.trim() === '') {
    throw new CoreError('INVALID_INPUT', 'A runtime executable path is required.');
  }

  const trimmed = path.trim();
  if (!isLocalWindowsAbsolutePath(trimmed) || trimmed.startsWith('\\\\?\\')) {
    throw new CoreError('PATH_NOT_ALLOWED', 'Only absolute local Windows paths are allowed.', {
      nextActions: ['Choose a local drive path such as C:\\Tools\\PHP\\php.exe.'],
    });
  }
  if (!isPhpCliName(trimmed)) {
    throw new CoreError('INVALID_RUNTIME_PATH', 'The selected file is not a PHP CLI executable.');
  }

  const normalizedPath = normalizeWindowsPath(fileSystem.realPath(trimmed));
  if (!isLocalWindowsAbsolutePath(normalizedPath) || normalizedPath.startsWith('\\\\')) {
    throw new CoreError('PATH_NOT_ALLOWED', 'The resolved runtime path is not a local Windows path.');
  }
  if (fileSystem.isReparsePoint(trimmed) || fileSystem.isReparsePoint(normalizedPath)) {
    throw new CoreError('PATH_NOT_ALLOWED', 'Reparse points are not accepted for runtime executables.');
  }
  // Check every parent directory as well as the executable. A junction/symlink
  // in a parent can otherwise redirect a seemingly local path to an untrusted tree.
  const driveRoot = normalizedPath.slice(0, 3);
  const parentParts = normalizedPath.slice(3).split('\\').filter(Boolean).slice(0, -1);
  let parent = driveRoot;
  for (const part of parentParts) {
    parent = `${parent.replace(/[\\/]$/, '')}\\${part}`;
    if (fileSystem.isReparsePoint(parent)) {
      throw new CoreError('PATH_NOT_ALLOWED', 'A parent directory is a reparse point.');
    }
  }
  if (!fileSystem.exists(normalizedPath)) {
    throw new CoreError('PATH_NOT_FOUND', 'The selected runtime executable does not exist.');
  }
  if (!fileSystem.isFile(normalizedPath)) {
    throw new CoreError('INVALID_RUNTIME_PATH', 'The selected runtime path is not a file.');
  }

  return { path: trimmed, normalizedPath };
}

function parseArchitecture(output: string): RuntimeArchitecture {
  const lower = output.toLocaleLowerCase('en-US');
  if (/(^|[^a-z0-9])arm64([^a-z0-9]|$)/.test(lower)) return 'arm64';
  if (/(^|[^a-z0-9])x64([^a-z0-9]|$)/.test(lower) || /amd64/.test(lower)) return 'x64';
  if (/(^|[^a-z0-9])x86([^a-z0-9]|$)/.test(lower) || /win32/.test(lower)) return 'x86';
  return 'unknown';
}

function parseThreadSafety(output: string): PhpThreadSafety {
  const match = output.match(/\((?:[^\n)]*\b)?(NTS|TS)(?:\b[^\n)]*)?\)/i);
  if (!match?.[1]) return 'unknown';
  return match[1].toLocaleLowerCase('en-US') as PhpThreadSafety;
}

function parseVersion(output: string): string | null {
  return output.match(/\bPHP\s+(\d+\.\d+\.\d+)\b/i)?.[1] ?? null;
}

function parseIniInfo(output: string): RuntimeIniInfo {
  const match = output.match(/Loaded Configuration File\s*=>\s*(.+)/i);
  if (!match?.[1]) return { source: 'unknown', path: null };
  const value = match[1].trim();
  if (!value || value.toLocaleLowerCase('en-US') === '(none)') return { source: 'none', path: null };
  return { source: 'explicit', path: normalizeWindowsPath(value) };
}

function supportedPhp(version: string | null): boolean {
  if (!version) return false;
  const [majorText, minorText] = version.split('.');
  const major = Number(majorText);
  const minor = Number(minorText);
  return (major === 5 && minor === 6) || (major === 7 && minor <= 4) || (major === 8 && minor <= 5);
}

function runtimeId(path: string): string {
  return `php_${sha256(normalizeWindowsPath(path).toLocaleLowerCase('en-US')).slice(0, 24)}`;
}

function nowIso(): string {
  return new Date().toISOString();
}

export async function probePhpExecutable(
  executablePath: string,
  fileSystem: FileSystemProbe,
  processProbe: ProcessProbe,
  now: () => Date = () => new Date(),
): Promise<PhpProbeResult> {
  const validated = validateWindowsExecutablePath(executablePath, fileSystem);
  const checkedAt = now().toISOString();
  const rootPath = winPath.dirname(validated.normalizedPath);
  const base: PhpProbeResult = {
    id: runtimeId(validated.normalizedPath),
    family: 'php',
    version: null,
    executablePath: validated.normalizedPath,
    rootPath,
    architecture: 'unknown',
    threadSafety: 'unknown',
    availability: 'detected-unmanaged',
    ownership: 'detected-unmanaged',
    activation: 'disabled',
    health: 'error',
    ini: { source: 'unknown', path: null },
    legacy: false,
    diagnostics: [],
    firstSeenAt: checkedAt,
    lastCheckedAt: checkedAt,
  };

  let probeOutput;
  try {
    // -n is deliberate: health checks must not load arbitrary project extensions.
    probeOutput = await processProbe.run(validated.normalizedPath, ['-n', '-v'], { timeoutMs: 5_000 });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { ...base, diagnostics: [`Probe failed: ${message}`] };
  }

  const combined = `${probeOutput.stdout}\n${probeOutput.stderr}`.trim();
  const version = parseVersion(probeOutput.stdout);
  const availability: RuntimeAvailability = version && supportedPhp(version) ? 'detected-unmanaged' : 'unsupported';
  const diagnostics = [] as string[];
  let ini: RuntimeIniInfo = base.ini;
  if (probeOutput.stderr.trim()) diagnostics.push(probeOutput.stderr.trim());
  if (probeOutput.exitCode !== 0) {
    diagnostics.push(`PHP probe exited with code ${probeOutput.exitCode}.`);
  } else if (!version) {
    diagnostics.push('PHP version could not be parsed from probe output.');
  } else if (availability === 'unsupported') {
    diagnostics.push('This PHP version is outside the supported 5.6–8.5 range.');
  }

  if (probeOutput.exitCode === 0 && version) {
    try {
      const iniProbe = await processProbe.run(validated.normalizedPath, ['--ini'], { timeoutMs: 5_000 });
      ini = parseIniInfo(`${iniProbe.stdout}\n${iniProbe.stderr}`);
      if (iniProbe.exitCode !== 0) {
        diagnostics.push(`PHP ini probe exited with code ${iniProbe.exitCode}.`);
      }
    } catch (error) {
      diagnostics.push(`PHP ini probe failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  const health: RuntimeHealth = probeOutput.exitCode === 0 && version ? (availability === 'unsupported' ? 'warning' : 'healthy') : 'error';
  return {
    ...base,
    version,
    architecture: parseArchitecture(combined),
    threadSafety: parseThreadSafety(combined),
    availability,
    health,
    ini,
    legacy: Boolean(version?.startsWith('5.6.')),
    diagnostics,
  };
}
