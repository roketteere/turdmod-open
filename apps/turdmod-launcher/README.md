# turdmod-launcher

DayZ-style desktop launcher for the **modded TurdMOD client**. Players pick a
TurdMOD server (BattlEye **off**), toggle which mods to run, and hit Play — the
launcher spawns `SCUM.exe` with the loader injected and connects straight to the
chosen server.

**Your Steam install is never modified.** For official servers, launch SCUM
normally from Steam — that stays vanilla + BattlEye on. This is two launch
*paths*, not a system-wide BattlEye toggle.

## Safety model (read this)

- The launcher only offers servers from `GET /api/servers` where
  `battlEye === false`. It **refuses** to launch against a BattlEye server —
  joining one with the BE-off modded client is an instant ban. That refusal
  lives in the shared injection core (`turdmod_launcher_core::launch`), not just
  the UI.
- Before resuming SCUM, the launcher writes
  `%LOCALAPPDATA%\TurdMOD\launch-mode.json`, which the loader's `detect.rs`
  reads. If BattlEye is ever detected in-process, the loader stays inert
  regardless of that flag (fail-closed).
- **Known gap:** the gate covers the server the launcher connects into, not a
  later in-game-browser join. See `docs/turdmod/battleye-safety.md` for the full
  residual-risk note. Don't claim "unbannable."

## Run locally (dev)

From this directory:

```
pnpm install            # once, from the repo root or here
pnpm --filter @turdmod/turdmod-launcher tauri:dev
```

`tauri:dev` starts the Vite dev server (port 5180) and the Rust backend with
hot reload. **Do not** use `pnpm tauri build` for testing — that produces
distribution installers.

## Build (distribution installer)

```
pnpm --filter @turdmod/turdmod-launcher tauri:build
```

Output: `src-tauri/target/release/bundle/`.

## Test / check

```
pnpm --filter @turdmod/turdmod-launcher build      # tsc + vite frontend
cargo check --manifest-path src-tauri/Cargo.toml   # Rust backend
```

## Preconditions

- Windows (DLL injection + SCUM are Windows-only).
- `turdmod_loader.dll` must be resolvable: set `TURDMOD_LOADER_DLL` to its path,
  or place it next to the launcher binary. Built from `apps/turdmod-loader`.
- SCUM installed (auto-discovered under Steam, or set `SCUM_EXE`).
- Server allowlist source: `TURDMOD_API_BASE` (default `https://turdmod.com`),
  endpoint `GET /api/servers`. Last-good list is cached to
  `%LOCALAPPDATA%\TurdMOD\servers-cache.json` for offline resilience.

## How it fits

- **Backend** (`src-tauri/src/commands.rs`): `launcher_list_servers`,
  `launcher_list_mods`, `launcher_set_enabled_mods`, `launcher_launch_modded`.
- **Injection core**: reused from `apps/turdmod-loader/launcher` (the crate
  `turdmod_launcher_core`) — the exact same spawn-suspended + CreateRemoteThread
  path the CLI `turdmod-launcher.exe` uses. One copy of the unsafe code.
- **Loader contracts**: `launch-mode.json` (mode handshake) and
  `mods/enabled.json` (enable/disable) under `%LOCALAPPDATA%\TurdMOD\`.
