// Typed wrappers for soft-RCON admin commands.

import { invoke } from '@tauri-apps/api/core';

export type AdminFilename =
  | 'BannedUsers.ini'
  | 'AdminUsers.ini'
  | 'WhitelistedUsers.ini'
  | 'SilencedUsers.ini'
  | 'ExclusiveUsers.ini'
  | 'ServerSettings.ini'
  | 'ServerSettingsAdminUsers.ini';

export interface ServerSettingsForm {
  serverName: string | null;
  serverDescription: string | null;
  serverPassword: string | null;
  maxPlayers: string | null;
  serverPlaystyle: string | null;
  messageOfTheDay: string | null;
  enableWhitelist: string | null;
  enableBattlEye: string | null;
  allowFirstPerson: string | null;
  allowThirdPerson: string | null;
  allowCrosshair: string | null;
  enableNewPlayerProtection: string | null;
  newPlayerProtectionDuration: string | null;
  allowVoting: string | null;
  dayCycleSpeedMultiplier: string | null;
  nighttimeSpeedMultiplier: string | null;
  economyMultiplier: string | null;
  respawnTime: string | null;
  xpMultiplier: string | null;
  rawIni: string;
}

export type SettingsPatch = Partial<
  Record<keyof Omit<ServerSettingsForm, 'rawIni'>, string>
>;

export function readAdminFile(serverId: string, filename: AdminFilename): Promise<string> {
  return invoke<string>('manager_server_read_admin_file', { serverId, filename });
}

export function writeAdminFile(
  serverId: string,
  filename: AdminFilename,
  contents: string,
): Promise<void> {
  return invoke<void>('manager_server_write_admin_file', { serverId, filename, contents });
}

export function parseServerSettings(serverId: string): Promise<ServerSettingsForm> {
  return invoke<ServerSettingsForm>('manager_server_parse_server_settings', { serverId });
}

export function saveServerSettingsPartial(
  serverId: string,
  patch: SettingsPatch,
): Promise<void> {
  return invoke<void>('manager_server_save_server_settings_partial', { serverId, patch });
}

export function parseUserList(raw: string): string[] {
  return raw
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith(';') && !l.startsWith('#'));
}

export function serializeUserList(ids: string[]): string {
  return ids.join('\n') + '\n';
}

export const STEAM64_RE = /^7656119\d{10}$/;

export function isValidSteam64(id: string): boolean {
  return STEAM64_RE.test(id);
}

// true = SCUM hot-reloads the file at runtime; false = restart required.
export const FILE_HOT_RELOAD: Record<AdminFilename, boolean> = {
  'BannedUsers.ini': true,
  'AdminUsers.ini': true,
  'WhitelistedUsers.ini': true,
  'SilencedUsers.ini': true,
  'ExclusiveUsers.ini': true,
  'ServerSettings.ini': false,
  'ServerSettingsAdminUsers.ini': true,
};
