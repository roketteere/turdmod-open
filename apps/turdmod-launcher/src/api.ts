// Tauri command wrappers + DTOs. The Rust backend names use snake_case;
// @tauri-apps invoke() uses the literal Rust function name (see memory
// reference_tauri_command_naming).
import { invoke } from "@tauri-apps/api/core";

export type ServerDto = {
  id: string;
  name: string;
  ip: string;
  port: number;
  battlEye: boolean;
  region?: string | null;
  description?: string | null;
};

export type ModDto = {
  id: string;
  name: string;
  version?: string | null;
  author?: string | null;
  description?: string | null;
  enabled: boolean;
};

export type LaunchResult = {
  pid: number;
  log: string[];
};

export const listServers = () => invoke<ServerDto[]>("launcher_list_servers");

export const listMods = () => invoke<ModDto[]>("launcher_list_mods");

export const setEnabledMods = (ids: string[]) =>
  invoke<void>("launcher_set_enabled_mods", { ids });

export const launchModded = (serverId: string) =>
  invoke<LaunchResult>("launcher_launch_modded", { serverId });

// True while the launched SCUM process is still running. UI polls this to
// close the launcher when the GAME exits (vs. just disconnecting a server).
export const pidAlive = (pid: number) =>
  invoke<boolean>("launcher_pid_alive", { pid });

export type JoinProgress = {
  pct: number;
  label: string;
  done: boolean;
  error?: string | null;
};

// Real join progress from SCUM's client log (boot → connect → in world).
// Drives the loading beam; not a fake timer.
export const joinProgress = () =>
  invoke<JoinProgress>("launcher_join_progress");
