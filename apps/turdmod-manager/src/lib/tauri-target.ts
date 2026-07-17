// Typed wrappers for the target switcher.
// 'game' = SCUM client (appid 513710), 'server' = SCUM Dedicated Server (appid 1077840).

import { invoke } from '@tauri-apps/api/core';

export type InstallTarget = 'game' | 'server';

export interface DetectedInstalls {
  game: string | null;
  server: string | null;
}

export function listDetectedInstalls(): Promise<DetectedInstalls> {
  return invoke<DetectedInstalls>('manager_list_detected_installs');
}

export function getActiveTarget(): Promise<InstallTarget> {
  return invoke<InstallTarget>('manager_get_active_target');
}

export function setActiveTarget(target: InstallTarget): Promise<void> {
  return invoke<void>('manager_set_active_target', { target });
}
