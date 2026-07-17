// Typed wrappers around the settings command surface.
// The RCON password is write-only from the frontend's perspective;
// `hasRconPassword` tells the UI whether one is stored.

import { invoke } from '@tauri-apps/api/core';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ManagerSettings {
  scumServerLogsDir: string | null;
  rconHost: string | null;
  rconPort: number | null;
  discordWebhook: string | null;
  hasRconPassword: boolean;
  /** False until the first-run wizard commits. */
  wizardCompleted: boolean;
}

export interface SettingsPatch {
  scumServerLogsDir?: string | null;
  rconHost?: string | null;
  rconPort?: number | null;
  /** Empty string = clear the keychain entry. Omit = no change. */
  rconPassword?: string;
  discordWebhook?: string | null;
  /** Pass true to mark the wizard as done; false to re-show it. */
  wizardCompleted?: boolean;
}

export interface NodeDetectResult {
  found: boolean;
  version: string | null;
}

export interface LogsDirTestResult {
  exists: boolean;
  hasLogFiles: boolean;
}

export interface RconTestResult {
  ok: boolean;
  error: string | null;
}

// ---------------------------------------------------------------------------
// Command wrappers
// ---------------------------------------------------------------------------

export function settingsGetAll(): Promise<ManagerSettings> {
  return invoke<ManagerSettings>('settings_get_all');
}

export function settingsSetPartial(patch: SettingsPatch): Promise<ManagerSettings> {
  return invoke<ManagerSettings>('settings_set_partial', { patch });
}

export function settingsTestLogsDir(path: string): Promise<LogsDirTestResult> {
  return invoke<LogsDirTestResult>('settings_test_logs_dir', { path });
}

export function settingsTestRcon(
  host: string,
  port: number,
  password: string,
): Promise<RconTestResult> {
  return invoke<RconTestResult>('settings_test_rcon', { host, port, password });
}

export function managerDetectNode(): Promise<NodeDetectResult> {
  return invoke<NodeDetectResult>('manager_detect_node');
}