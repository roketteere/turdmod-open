// Tools the assistant can call. Each maps onto the SAME Tauri command a UI
// button uses — the assistant can't do anything you couldn't do by clicking.
//
// @inv: every tool marked destructive:true must show a confirm card before it
//       runs. Adding a tool that mutates the machine without that flag is the
//       one way this app could surprise someone.

import { api } from "../lib/api";
import type { HostKind } from "../lib/api";
import type { SetupStore } from "../lib/setup-state";
import type { ToolDef } from "./providers";

export interface ToolSpec {
  def: ToolDef;
  destructive: boolean;
  /** Plain-language one-liner for the confirm card. */
  summarize: (args: Record<string, unknown>) => string;
  run: (args: Record<string, unknown>) => Promise<unknown>;
}

const str = (v: unknown, fallback = ""): string => (typeof v === "string" ? v : fallback);
const num = (v: unknown, fallback: number): number => (typeof v === "number" ? v : fallback);

export function buildTools(store: SetupStore): ToolSpec[] {
  const { state, set, go } = store;

  return [
    {
      def: {
        name: "detect_installs",
        description:
          "Scan this PC for SCUM installs (game and dedicated server) via the Steam registry and library folders. Returns paths found and where it looked.",
        parameters: { type: "object", properties: {} },
      },
      destructive: false,
      summarize: () => "Scan this PC for SCUM installs",
      run: async () => {
        const d = await api.detectInstalls();
        set({ detected: d, ...(d.server && !state.serverRoot ? { serverRoot: d.server } : {}) });
        return d;
      },
    },

    {
      def: {
        name: "set_server_root",
        description:
          "Set the SCUM dedicated server folder that TurdMOD will be installed into. Validate it first with path_exists if unsure.",
        parameters: {
          type: "object",
          properties: { path: { type: "string", description: "Absolute path to the SCUM server folder" } },
          required: ["path"],
        },
      },
      destructive: false,
      summarize: (a) => `Use ${str(a.path)} as the server folder`,
      run: async (a) => {
        const path = str(a.path);
        const ok = await api.validatePath(path);
        if (!ok) return { ok: false, error: "That folder doesn't look like a SCUM server install." };
        set({ serverRoot: path });
        return { ok: true, serverRoot: path };
      },
    },

    {
      def: {
        name: "set_host_kind",
        description:
          "Record where the user's server lives: 'local' (this PC), 'own-vps' (their own box with SSH/RDP), 'rented-ftp' (a game host with only FTP and a web panel), or 'unknown'.",
        parameters: {
          type: "object",
          properties: {
            host_kind: { type: "string", enum: ["local", "own-vps", "rented-ftp", "unknown"] },
          },
          required: ["host_kind"],
        },
      },
      destructive: false,
      summarize: (a) => `Set host type to ${str(a.host_kind)}`,
      run: async (a) => {
        const hostKind = str(a.host_kind, "unknown") as HostKind;
        set({ hostKind });
        return { hostKind };
      },
    },

    {
      def: {
        name: "capability_report",
        description:
          "Get the honest list of what this hosting situation supports (engine mods, pak mods, config tuning, dashboard) with a reason for anything unsupported. Call this before promising the user anything.",
        parameters: {
          type: "object",
          properties: {
            host_kind: { type: "string", enum: ["local", "own-vps", "rented-ftp", "unknown"] },
            can_execute: {
              type: "boolean",
              description: "True only if it's confirmed the user can run their own programs on that box.",
            },
          },
          required: ["host_kind", "can_execute"],
        },
      },
      destructive: false,
      summarize: () => "Check what this hosting setup supports",
      run: async (a) => {
        const rep = await api.capabilityReport(
          str(a.host_kind, "unknown") as HostKind,
          a.can_execute === true,
        );
        set({ capability: rep });
        return rep;
      },
    },

    {
      def: {
        name: "prepare_config",
        description:
          "Generate the service configuration (fills in paths, locates the Server Pack artifacts). Safe — writes nothing to disk. If TurdMOD is already installed this detects it and REUSES the existing access key and settings; the result tells you via is_update / token_preserved / service_state.",
        parameters: {
          type: "object",
          properties: {
            server_root: { type: "string" },
            port: { type: "number", description: "Optional. Defaults to 9090." },
          },
          required: ["server_root"],
        },
      },
      destructive: false,
      summarize: () => "Generate the service settings",
      run: async (a) => {
        const cfg = await api.prepareConfig(str(a.server_root, state.serverRoot), num(a.port, state.port));
        set({
          token: cfg.token,
          port: cfg.port,
          config: cfg.config,
          artifactsDir: cfg.artifacts_dir,
          isUpdate: cfg.is_update,
          tokenPreserved: cfg.token_preserved,
          serviceState: cfg.service_state,
          serverRoot: str(a.server_root, state.serverRoot),
        });
        // Never echo the token back to the model — it's a live credential.
        return { ...cfg, token: "(hidden)" };
      },
    },

    {
      def: {
        name: "install_local",
        description:
          "Perform the install on THIS PC: copy the bridge and loader files into the server folder, write the service config, and install + start the Windows Service. Requires prepare_config first. Only valid when the game server runs on this same machine — it cannot install to a remote box.",
        parameters: { type: "object", properties: {} },
      },
      destructive: true,
      summarize: () =>
        state.isUpdate
          ? `Update TurdMOD in ${state.serverRoot || "the server folder"}${state.serviceState === "running" ? " — this stops the running server" : ""}`
          : `Copy TurdMOD files into ${state.serverRoot || "the server folder"} and install the Windows Service`,
      run: async () => {
        if (!state.config) return { ok: false, error: "Call prepare_config first." };
        const results = await api.installLocal(state.serverRoot, state.config, state.artifactsDir);
        const failed = results.filter((r) => !r.ok);
        set({
          installResults: results,
          lastError: failed.map((r) => `${r.step}: ${r.detail}`).join("; "),
        });
        go("install");
        return results;
      },
    },

    {
      def: {
        name: "verify_install",
        description:
          "Check that the install actually works: service health, game server running, engine bridge responding. Each failing check comes back with a specific fix.",
        parameters: { type: "object", properties: {} },
      },
      destructive: false,
      summarize: () => "Check whether TurdMOD is running",
      run: async () => {
        const rep = await api.verify(state.port, state.token, state.serverRoot);
        set({
          verifyReport: rep,
          lastError: rep.all_ok ? "" : rep.checks.filter((c) => !c.ok).map((c) => c.detail).join("; "),
        });
        go("verify");
        return rep;
      },
    },

    {
      def: {
        name: "check_for_update",
        description:
          "Ask turdmod.com whether a newer TurdMOD build is published, and compare it to what's installed here. Returns state 'current', 'available', or 'unknown'. 'unknown' means we genuinely couldn't tell — never tell the user they're up to date on an 'unknown'.",
        parameters: { type: "object", properties: {} },
      },
      destructive: false,
      summarize: () => "Check whether a newer TurdMOD is available",
      run: async () => api.checkForUpdate(),
    },

    {
      def: {
        name: "client_plan",
        description:
          "For the modded client: measure the user's SCUM game install and list every drive with free space, showing what a modded copy would cost on each. A drive on the same volume as the game can share the read-only game content, so it costs ~1 GB and takes seconds instead of ~89 GB. Call this before offering to build a copy.",
        parameters: {
          type: "object",
          properties: { source: { type: "string", description: "The SCUM game (client) folder." } },
          required: ["source"],
        },
      },
      destructive: false,
      summarize: () => "Check what a modded game copy would cost",
      run: async (a) => api.clientPlan(str(a.source, state.detected?.game ?? "")),
    },

    {
      def: {
        name: "client_create_copy",
        description:
          "Build the isolated modded copy of the game at `dest`. Never modifies the Steam install — that's what keeps official-server play safe. Refuses a destination that already exists and isn't empty. Also tells the Launcher where the copy lives. Use client_plan first and pick a drive that fits.",
        parameters: {
          type: "object",
          properties: {
            source: { type: "string" },
            dest: { type: "string", description: "e.g. C:\\SCUM-Modded" },
          },
          required: ["source", "dest"],
        },
      },
      destructive: true,
      summarize: (a) => `Build a modded copy of the game at ${str(a.dest)} (your Steam install is not touched)`,
      run: async (a) => {
        const results = await api.clientCreateCopy(
          str(a.source, state.detected?.game ?? ""),
          str(a.dest),
        );
        set({
          installResults: results,
          lastError: results.filter((r) => !r.ok).map((r) => `${r.step}: ${r.detail}`).join("; "),
        });
        return results;
      },
    },

    {
      def: {
        name: "uninstall_plan",
        description:
          "See exactly what removing TurdMOD would do, without doing it. Tells you which files get restored from backup, which get deleted, and whether an install record exists. Always call this before uninstall_run so you can tell the user what's about to happen.",
        parameters: { type: "object", properties: {} },
      },
      destructive: false,
      summarize: () => "Check what removing TurdMOD would do",
      run: async () => api.uninstallPlan(),
    },

    {
      def: {
        name: "uninstall_run",
        description:
          "Remove TurdMOD: stop and unregister the service, restore every file we replaced from backup, delete every file we added, and take the bridge out of UE4SS's mods.txt. Keeps service.json by default so a reinstall remembers their settings — pass remove_settings true only if they explicitly want a clean slate. This stops their game server.",
        parameters: {
          type: "object",
          properties: {
            remove_settings: {
              type: "boolean",
              description: "Also delete service.json (token, ports, tuning). Default false.",
            },
          },
        },
      },
      destructive: true,
      summarize: (a) =>
        `Remove TurdMOD from this PC${a.remove_settings === true ? ", including your saved settings" : " (keeping your settings)"}${state.serviceState === "running" ? " — this stops the running server" : ""}`,
      run: async (a) => {
        const results = await api.uninstallRun(a.remove_settings === true);
        const failed = results.filter((r) => !r.ok);
        set({
          installResults: results,
          lastError: failed.map((r) => `${r.step}: ${r.detail}`).join("; "),
        });
        return results;
      },
    },

    {
      def: {
        name: "path_exists",
        description: "Check whether a file or folder exists.",
        parameters: {
          type: "object",
          properties: { path: { type: "string" } },
          required: ["path"],
        },
      },
      destructive: false,
      summarize: (a) => `Check whether ${str(a.path)} exists`,
      run: async (a) => ({ exists: await api.pathExists(str(a.path)) }),
    },

    {
      def: {
        name: "read_text_file",
        description:
          "Read a text file (config, INI, JSON). Truncated to 20,000 characters. Use tail_log for logs instead.",
        parameters: {
          type: "object",
          properties: { path: { type: "string" } },
          required: ["path"],
        },
      },
      destructive: false,
      summarize: (a) => `Read ${str(a.path)}`,
      run: async (a) => api.readTextFile(str(a.path)),
    },

    {
      def: {
        name: "tail_log",
        description:
          "Read the last lines of a log file — the fastest way to diagnose a failed install. Useful paths: the service log next to turdmod-service.exe, and UE4SS.log in the server folder.",
        parameters: {
          type: "object",
          properties: {
            path: { type: "string" },
            lines: { type: "number", description: "Default 80, max 500." },
          },
          required: ["path"],
        },
      },
      destructive: false,
      summarize: (a) => `Read the end of ${str(a.path)}`,
      run: async (a) => api.tailLog(str(a.path), num(a.lines, 80)),
    },

    {
      def: {
        name: "write_text_file",
        description:
          "Write a text file — use to fix a broken config. Read it first so you don't clobber settings you didn't mean to change.",
        parameters: {
          type: "object",
          properties: { path: { type: "string" }, contents: { type: "string" } },
          required: ["path", "contents"],
        },
      },
      destructive: true,
      summarize: (a) => `Overwrite ${str(a.path)}`,
      run: async (a) => {
        await api.writeTextFile(str(a.path), str(a.contents));
        return { ok: true };
      },
    },

    {
      def: {
        name: "go_to_step",
        description:
          "Move the wizard to a step so the user can see what you're doing. Steps: welcome, detect, capability, configure, install, verify.",
        parameters: {
          type: "object",
          properties: {
            step: {
              type: "string",
              enum: ["welcome", "detect", "capability", "configure", "install", "verify"],
            },
          },
          required: ["step"],
        },
      },
      destructive: false,
      summarize: (a) => `Move to the ${str(a.step)} step`,
      run: async (a) => {
        go(str(a.step, "welcome") as never);
        return { step: a.step };
      },
    },
  ];
}
