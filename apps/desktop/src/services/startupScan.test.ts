import { describe, expect, it } from 'vitest';
import { shouldScanOnStartup } from './startupScan';
import type { RuntimeRecord } from '../types/runtime';

const baseSnapshot = {
  platform: 'windows' as const,
  platformLabel: 'Windows 11 · 当前用户',
  pathLease: 'inactive' as const,
  runtimes: [],
  activities: [],
};

const knownRuntime: RuntimeRecord = {
  id: 'php-8.5',
  family: 'php',
  name: 'PHP',
  version: '8.5.11',
  availability: 'detected-unmanaged',
  activation: 'disabled',
  health: 'healthy',
  operation: 'idle',
  architecture: 'x64',
  path: 'R:\\App\\Env\\php\\php8.5.11\\php.exe',
  source: 'detected-unmanaged',
  lastCheckedAt: 'now',
  detail: 'NTS',
};

describe('startup scan policy', () => {
  it('scans an untouched empty snapshot once', () => {
    expect(shouldScanOnStartup({ ...baseSnapshot, lastScanAt: null })).toBe(true);
  });

  it('does not rescan an intentionally empty environment after a prior scan', () => {
    expect(shouldScanOnStartup({ ...baseSnapshot, lastScanAt: '2026-10-13T00:00:00Z' })).toBe(false);
  });

  it('does not scan again when runtimes are already known', () => {
    expect(shouldScanOnStartup({
      ...baseSnapshot,
      lastScanAt: null,
      runtimes: [knownRuntime],
    })).toBe(false);
  });
});
