// Tauri wrappers for install-snapshot commands.

import { invoke } from '@tauri-apps/api/core';

export interface SnapshotResult {
  target: string;
  scumBuild: string;
  source: string;
  destination: string;
  durationMs: number;
  ok: boolean;
  robocopyExitCode: number;
  summary: string;
}

export interface SnapshotEntry {
  target: string;
  scumBuild: string;
  path: string;
  sizeBytes: number;
  createdAtIso: string;
}

export function snapshotInstall(target: 'server' | 'client'): Promise<SnapshotResult> {
  return invoke<SnapshotResult>('snapshot_install', { target });
}

export function snapshotList(target?: 'server' | 'client'): Promise<SnapshotEntry[]> {
  return invoke<SnapshotEntry[]>('snapshot_list', { target });
}
