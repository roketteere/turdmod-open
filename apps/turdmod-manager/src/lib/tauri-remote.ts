// Typed wrappers for remote service API commands.
// These talk to turdmod-service running on OVH via the Tauri backend.

import { invoke } from '@tauri-apps/api/core';

export interface RemoteConfig {
  host: string;
  port: number;
  token: string;
  enabled: boolean;
}

export interface RemoteHealth {
  service: string;
  version: string;
  uptime_secs: number;
}

export interface RemoteStatus {
  server_running: boolean;
  pid: number | null;
  uptime_secs: number;
  restarts: number;
}

export function remoteGetConfig(): Promise<RemoteConfig | null> {
  return invoke<RemoteConfig | null>('remote_get_config');
}

export function remoteSetConfig(cfg: RemoteConfig): Promise<void> {
  return invoke('remote_set_config', { ...cfg });
}

export function remoteHealth(): Promise<RemoteHealth> {
  return invoke<RemoteHealth>('remote_health');
}

export function remoteStatus(): Promise<RemoteStatus> {
  return invoke<RemoteStatus>('remote_status');
}

// Server lifecycle — target-aware ("local" or "remote"/OVH), matching the
// Admin Map host toggle. countdown (secs) on restart fires a player-warning banner.
export interface ServerStatus {
  server_running: boolean;
  pid?: number | null;
  uptime_secs?: number;
  restarts?: number;
  build?: string;
  /** Effective-next-launch BattlEye state (true = secure / no -NoBattlEye). */
  battleye?: boolean;
}

export function remoteServerStart(target: string): Promise<{ pid: number }> {
  return invoke<{ pid: number }>('remote_server_start', { target });
}

export function remoteServerStop(target: string): Promise<{ ok: boolean }> {
  return invoke<{ ok: boolean }>('remote_server_stop', { target });
}

export function remoteServerRestart(target: string, countdown?: number): Promise<{ pid: number }> {
  return invoke<{ pid: number }>('remote_server_restart', { target, countdown: countdown ?? null });
}

export function remoteServerStatus(target: string): Promise<ServerStatus> {
  return invoke<ServerStatus>('remote_server_status', { target });
}

// Stage BattlEye on/off (applies on the next start/restart).
export function remoteServerBattleye(
  target: string,
  enabled: boolean,
): Promise<{ ok: boolean; battleye: boolean; args: string[]; pendingRestart: boolean }> {
  return invoke('remote_server_battleye', { target, enabled });
}

// Admin config files (AdminUsers.ini / ServerSettingsAdminUsers.ini) on the target host.
export interface AdminFileResult {
  ok: boolean;
  name: string;
  contents?: string;
  pendingRestart?: boolean;
  error?: string;
}

export function remoteAdminFileGet(target: string, name: string): Promise<AdminFileResult> {
  return invoke<AdminFileResult>('remote_admin_file_get', { target, name });
}

export function remoteAdminFileSet(target: string, name: string, contents: string): Promise<AdminFileResult> {
  return invoke<AdminFileResult>('remote_admin_file_set', { target, name, contents });
}

// TurdMOD data JSON files (safe_zones / vehicle_ownership / repo_history / vehicle_durability)
// via the service /data/file endpoint. Backs the Vehicle Manager + Safe Zones tabs.
export function remoteDataFileGet(target: string, name: string): Promise<AdminFileResult> {
  return invoke<AdminFileResult>('remote_data_file_get', { target, name });
}
export function remoteDataFileSet(target: string, name: string, contents: string): Promise<AdminFileResult> {
  return invoke<AdminFileResult>('remote_data_file_set', { target, name, contents });
}

// All server vehicles (live scum.db) + manual repossession.
export interface ServerVehicle {
  id: number; class: string; x?: number; y?: number; z?: number;
  sector: string; owner?: string | null; locked: boolean; parts: string[];
}
export function remoteVehiclesDetail(target: string): Promise<{ count: number; vehicles: ServerVehicle[] }> {
  return invoke('remote_vehicles_detail', { target });
}
export function remoteVehiclesRepo(
  target: string,
  vehicles: { id: number; vehicle?: string; owner?: string }[],
): Promise<{ ok: boolean; destroyed: number; failed: number }> {
  return invoke('remote_vehicles_repo', { target, vehicles });
}

// Permissions — the live tier ladder + per-player tiers + per-command overrides.
// Reads/writes the SAME state the in-game `!perm` mutates (takes effect live).
export interface PlayerPerms {
  name: string;
  tier: string; // one of PermissionsState.tiers
  mod_overrides: Record<string, boolean>; // command/mod -> allow(true)/deny(false)
}
export interface ModConfig {
  enabled: boolean;
  required_tier: string;
}
export interface PermissionsState {
  ok?: boolean;
  players: Record<string, PlayerPerms>; // steamId -> perms (key may be "pending_<name>")
  mods: Record<string, ModConfig>; // command/feature -> config (the full catalog)
  tiers: string[]; // ladder low->high, e.g. ["Free","Bronze",...,"Admin"]
}
export function remotePermissionsGet(target: string): Promise<PermissionsState> {
  return invoke('remote_permissions_get', { target });
}
export function remotePermissionsSet(
  target: string,
  state: { players: Record<string, PlayerPerms>; mods: Record<string, ModConfig> },
): Promise<{ ok: boolean }> {
  return invoke('remote_permissions_set', { target, state });
}

// Save-edit a player's skills (level/xp). The service stops SCUM, backs up scum.db, writes,
// then restarts — #SetSkillLevel is dev-gated so the save is the only reliable path.
export interface SkillEdit { name: string; level?: number | null; xp?: number | null }
export function scumdbSetSkill(
  target: string,
  steam: string,
  skills: SkillEdit[],
): Promise<{ ok: boolean; updated?: number; backup?: string | null; restarted?: boolean; error?: string }> {
  return invoke('scumdb_set_skill', { target, steam, skills });
}

// Save-edit base attributes (Strength/Constitution/Dexterity/Intelligence) in body_simulation.
export interface AttrEdit { name: string; value: number }
export function scumdbSetAttribute(
  target: string,
  steam: string,
  attributes: AttrEdit[],
): Promise<{ ok: boolean; updated?: number; backup?: string | null; restarted?: boolean; error?: string }> {
  return invoke('scumdb_set_attribute', { target, steam, attributes });
}

// Recurring restart schedule. Epochs (seconds); render local clocks client-side.
export interface WarnBanner {
  at_secs: number; // fire this banner when this many seconds remain before restart
  color: string; // "R-G-B" center-banner color
  text: string;
}
export interface RestartScheduleInfo {
  enabled: boolean;
  interval_hours: number;
  warn_secs: number;
  last_restart_at?: number | null;
  next_restart_at?: number | null;
  now: number;
  messages: WarnBanner[];
}

export function remoteServerScheduleGet(target: string): Promise<RestartScheduleInfo> {
  return invoke<RestartScheduleInfo>('remote_server_schedule_get', { target });
}

export function remoteServerScheduleSet(
  target: string,
  body: { enabled?: boolean; interval_hours?: number; warn_secs?: number; messages?: WarnBanner[] },
): Promise<RestartScheduleInfo> {
  return invoke<RestartScheduleInfo>('remote_server_schedule_set', { target, body });
}

// Elevated (dev-command) users — SCUM.db `elevated_users`, turdmod-managed + queued.
export interface ElevatedInfo {
  ok: boolean;
  effective: string[]; // active in SCUM.db now
  managed: string[]; // turdmod-managed desired set
  pending_add: string[]; // will activate on next restart
  pending_remove: string[];
}

export function remoteElevatedGet(target: string): Promise<ElevatedInfo> {
  return invoke<ElevatedInfo>('remote_elevated_get', { target });
}

export function remoteElevatedSet(
  target: string,
  body: { add?: string[]; remove?: string[] },
): Promise<ElevatedInfo> {
  return invoke<ElevatedInfo>('remote_elevated_set', { target, body });
}

// Loot customization files (list / read Default|Override / write Override).
export interface LootFileResult { ok: boolean; path?: string; contents?: string; files?: string[]; error?: string }

export function remoteLootFiles(target: string): Promise<LootFileResult> {
  return invoke<LootFileResult>('remote_loot_files', { target });
}
export function remoteLootFileGet(target: string, path: string): Promise<LootFileResult> {
  return invoke<LootFileResult>('remote_loot_file_get', { target, path });
}
export function remoteLootFileSet(target: string, path: string, contents: string): Promise<LootFileResult> {
  return invoke<LootFileResult>('remote_loot_file_set', { target, body: { path, contents } });
}

export function remoteServerLogs(lines?: number): Promise<{ lines: string[] }> {
  return invoke<{ lines: string[] }>('remote_server_logs', { lines: lines ?? 100 });
}

export function remoteEngineRpc<T = unknown>(
  method: string,
  params?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>('remote_engine_rpc', { method, params: params ?? null });
}

// scumpilot (AI admin) control — proxied by the service at /scumpilot/*.
export interface ScumpilotStatus {
  ok: boolean;
  allPaused: boolean;
  agents: Record<string, boolean>;
  ownerOnline: boolean;
}

export function remoteScumpilotStatus(): Promise<ScumpilotStatus> {
  return invoke<ScumpilotStatus>('remote_scumpilot_status');
}

export function remoteScumpilotPause(): Promise<{ ok: boolean; paused: boolean }> {
  return invoke<{ ok: boolean; paused: boolean }>('remote_scumpilot_pause');
}

export function remoteScumpilotResume(): Promise<{ ok: boolean; paused: boolean }> {
  return invoke<{ ok: boolean; paused: boolean }>('remote_scumpilot_resume');
}
