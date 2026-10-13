import type { EnvironmentSnapshot } from '../types/runtime';

/** 首次启动只在没有任何历史扫描记录时触发一次只读扫描。 */
export function shouldScanOnStartup(snapshot: EnvironmentSnapshot): boolean {
  return snapshot.runtimes.length === 0 && snapshot.lastScanAt === null;
}
