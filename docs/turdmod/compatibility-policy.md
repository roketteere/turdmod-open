# TurdMOD — compatibility policy

Where each mod-delivery strategy is allowed to run, with the technical gating
the loader / manager UI enforces. This doc is the single source of truth that
the manifest validator and the loader's runtime checks read from.

For the BE-detection technical design that backs Strategy C, see
`docs/turdmod/battleye-safety.md`.

---

## Compatibility matrix

| Environment                 | Strategy A: pak content | Strategy B: server-side | Strategy C: offline scripting | Strategy D: external tool |
|---|:-:|:-:|:-:|:-:|
| Solo / offline              | ✅ | n/a | ✅ | ✅ |
| Private server, BE off      | ✅ | ✅  | ✅ | ✅ |
| Private server, BE on       | ✅ | ✅  | ❌ | ✅ |
| Official Gamepires server   | ❌ | n/a | ❌ | ✅ |

**Strategy A (pak content)** runs on every private server regardless of BE
state — both cosmetic and gameplay paks. Private-server admins manage their
own server's policy via `~mods/` access and player bans. Three-warning UI
keeps players from accidentally connecting to an official server with paks
loaded (see `battleye-safety.md` § "Private-server-only policy").

**Strategy B (server-side)** runs in the dedicated server process; BE doesn't
participate. Available to any private server.

**Strategy C (offline scripting)** is the only strategy that requires the
detection-and-refusal layer. Loader fails closed if it can't determine BE
state — see `battleye-safety.md` for the runtime check sequence.

**Strategy D (external tool)** runs as a separate process and never touches
the game. Available everywhere; same vector as OBS / Discord overlay /
streamer tooling.

## Manifest enforcement

Every mod manifest declares one `mode`. The loader rejects a mod whose mode
isn't allowed in the current environment. Example for the manifest spec
(KTask #138):

```yaml
id: my-loot-tweak
name: "Better TEC1 Loot"
version: 1.0.0
author: "TechyRican"
mode: server-side          # pak-content | server-side | offline-only | external-tool
min_build: "23128448"
max_build: ""              # empty = no upper bound
description: |
  Increases drop rates of Memory Modules in TEC1 abandoned bunker chests.
capabilities:
  - filesystem: read
  - network: localhost
entrypoint: scripts/main.lua
```

## How the loader detects environment

| Detection question | Source |
|---|---|
| "Is BattlEye running?" | Process enumeration (`BEService.exe`, `BEClient_x64.exe`, `BEDaisy.sys`) |
| "Is SCUM in single-player mode?" | Game launch arguments (`-singleplayer`, `-NoBattlEye`) |
| "Is the player connected to an official server?" | Server name / metadata via overlay capture or loopback log feed |
| "Is the active server BE-required?" | Server browser metadata if accessible, else conservative default = yes |

Detection runs once at loader startup and again on every server connection
event. Watchdog re-runs every 30s while injected (Strategy C only).

## Three-warning UI for pak activation (Strategy A)

Pak files mount at game startup, not per-server. To prevent a player loading
a modded pak and then connecting to an official server, the manager surfaces
warnings at three points:

1. **Before activation** — explicit consent dialog: "This pak modifies your
   SCUM client. Use on private servers and solo only. Do NOT connect to an
   official Gamepires server with this mod active."
2. **At launch** — Vanilla vs Modded launcher modes. Vanilla mode moves the
   contents of `~mods/` aside before starting the game.
3. **Post-connect** — top-of-screen warning if an official server is detected
   with `~mods/` non-empty.

The warning is informational; TurdMOD never force-disconnects the player.

## Where this is enforced

- **Manifest validator** (KTask #138, will live in the registry backend
  #151 + the `turdmod` CLI #146): rejects manifests with invalid `mode`,
  out-of-range `min_build`/`max_build`, or unknown capabilities.
- **Loader runtime** (KTask #135, #136): re-checks the matrix at every
  injection / pak mount / server connection event.
- **Manager UI** (KTask #150): surfaces the three-warning flow + Vanilla/
  Modded launcher modes + post-connect warning.
