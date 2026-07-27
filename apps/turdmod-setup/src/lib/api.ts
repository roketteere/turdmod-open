// Typed bindings for the Rust commands. The AI assistant calls these same
// functions the UI buttons do — one path, no drift.

import { invoke } from "@tauri-apps/api/core";

export type HostKind = "local" | "own-vps" | "rented-ftp" | "unknown";
export type Support = "yes" | "no" | "maybe";

export interface DetectedInstalls {
  game: string | null;
  server: string | null;
  searched: string[];
}

export interface Capability {
  id: string;
  label: string;
  support: Support;
  reason: string;
}

export interface CapabilityReport {
  host_kind: HostKind;
  capabilities: Capability[];
  verdict: string;
  engine_supported: boolean;
}

export type ServiceState = "missing" | "stopped" | "running";

export interface PreparedConfig {
  token: string;
  port: number;
  config: Record<string, unknown>;
  artifacts_dir: string | null;
  /** TurdMOD is already here — this run updates rather than installs fresh. */
  is_update: boolean;
  /** The existing access key was reused, so dashboards keep working. */
  token_preserved: boolean;
  service_state: ServiceState;
}

export interface StepResult {
  step: string;
  ok: boolean;
  detail: string;
}

export interface Check {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  fix: string;
}

export interface VerifyReport {
  checks: Check[];
  all_ok: boolean;
  summary: string;
}

export interface UninstallPlan {
  steps: string[];
  has_manifest: boolean;
  service_state: ServiceState;
  files_to_restore: number;
  files_to_remove: number;
  /** Non-empty when we can't fully reverse — show it before they start. */
  warning: string;
}

export const api = {
  detectInstalls: () => invoke<DetectedInstalls>("detect_installs"),

  validatePath: (path: string) => invoke<boolean>("validate_path", { path }),

  findServerExe: (root: string) => invoke<string | null>("find_server_exe", { root }),

  capabilityReport: (hostKind: HostKind, canExecute: boolean) =>
    invoke<CapabilityReport>("capability_report", { hostKind, canExecute }),

  prepareConfig: (serverRoot: string, port?: number) =>
    invoke<PreparedConfig>("prepare_config", { serverRoot, port }),

  installLocal: (serverRoot: string, config: Record<string, unknown>, artifactsDir?: string | null) =>
    invoke<StepResult[]>("install_local_full", { serverRoot, config, artifactsDir }),

  verify: (port: number, token: string, serverRoot?: string) =>
    invoke<VerifyReport>("verify_install", { port, token, serverRoot }),

  uninstallPlan: () => invoke<UninstallPlan>("uninstall_plan"),
  uninstallRun: (removeSettings?: boolean) =>
    invoke<StepResult[]>("uninstall_run", { removeSettings }),

  // Assistant helpers
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  tailLog: (path: string, lines?: number) => invoke<string>("tail_log", { path, lines }),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  writeTextFile: (path: string, contents: string) =>
    invoke<void>("write_text_file", { path, contents }),
};
