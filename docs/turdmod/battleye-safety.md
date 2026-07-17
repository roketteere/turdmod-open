# TurdMOD — BattlEye-safe architecture

**Status:** draft, 2026-05-08. Pre-permission-scope (KTask #134); revise once
that task lands.

This document is the technical foundation for shipping TurdMOD without putting any
SCUM player at risk of a BattlEye ban. It is the answer to the question "how do
we build a mod engine for a BattlEye-protected game without poking the bear?"

**TL;DR**: We never inject into a process while BattlEye is active. We pick four
mod-delivery strategies (pak content, server-side, offline scripting, external
tool) and gate each one to environments where BattlEye is provably not running.
A startup detection-and-refusal layer makes this enforceable in code, not just
policy.

---

## What BattlEye actually does (so we know what to avoid)

BattlEye is a kernel + user-mode anti-cheat. The relevant detection vectors for
a mod loader:

1. **Module / DLL injection scan**. BE walks the loaded modules in the protected
   process and flags anything not signed / not whitelisted. `LoadLibrary`,
   `CreateRemoteThread`, manual mappers — all noticed.
2. **Code-patch detection in protected modules**. Hooks into `SCUM-Win64-Shipping.exe`
   or known UE engine modules (jmps, IAT detours, vtable swaps) get caught.
3. **Memory read/write on the game process**. `ReadProcessMemory` /
   `WriteProcessMemory` from a foreign process is one of the loudest signals.
4. **Hidden / suspicious processes**. Drivers from unsigned kernel sources;
   processes that hide their main window or evade enumeration.
5. **Debugger attach**. `IsDebuggerPresent` style checks; debugger attach during
   a protected session is a flag.
6. **File-integrity checks** on certain game files (executables, configs).
   BattlEye historically does NOT scan `Content/Paks/*.pak` files — paks are
   game data, and UE supports modular pak loading. But this is the one boundary
   we want to verify directly with Gamepires (see permission-scope task).

What BattlEye does **NOT** do (industry consensus, not just our claim):

- Block screen capture (DXGI/GDI/Windows.Graphics.Capture) — every streamer on
  earth runs OBS while BE is active. Confirmed in our existing memory at
  `feedback_no_battleye_risk.md` and the Auto-PIN feature already shipped uses
  this vector.
- Block external HID input (SendInput, SendKeys) — all helper utilities,
  AutoHotkey, Steam's keybinder, etc., use this.
- Inspect or block other applications running on the same machine — anti-cheat
  is scoped to the protected process.
- Scan or block .pak files in `Content/Paks/~mods/` (unconfirmed but consistent
  with UE 4.27 behavior). To be confirmed via Gamepires.

## The four delivery strategies

Each is BattlEye-safe by construction. A given mod chooses one in its manifest.

### Strategy A — Pak content mods

- Mod ships as a `.pak` file dropped into
  `<game>/SCUM/Content/Paks/~mods/<modid>.pak`
- UE 4.27 native: paks in `~mods/` are automatically merged at startup.
- Use cases: replace textures, models, audio; override DataTable rows
  (item stats, loot weights, crafting recipes); inject locres entries; new
  Blueprints (decorative).
- **Process touch:** zero. The game loads the pak through its normal asset
  pipeline.
- **BE risk:** zero, *unless Gamepires considers paks in `~mods/` a TOS
  violation*. Gating on the permission grant.
- Limitation: no scripting, no behavior changes that require code.

### Strategy B — Server-side mods

- Mod runs **on the dedicated server**, not the client.
- The dedicated server (`SCUMServer.exe`) does not run BattlEye — BE is a
  client-side anti-cheat protecting players, not servers.
- We can do anything: spawn rate tweaks, custom drop tables, weather control,
  custom NPCs, faction logic, scripted events, economy rules, custom RCON
  commands.
- Players connect with **vanilla clients** and see the modded behavior. Their
  client never sees a single byte of mod code.
- **Process touch:** server only.
- **BE risk:** zero. BE is not in the loop.
- Best target for the most-requested categories: server economy, custom missions,
  loot table changes, persistence systems, anti-griefing rules, admin tooling.

### Strategy C — Offline / single-player scripting

- For solo mode and private servers with `-NoBattlEye`, we ship a UE4SS-style
  DLL loader + LuaJIT runtime that mounts at game startup.
- Loader pre-flight (see "Detection layer" below) guarantees BE is not active
  before injecting.
- Can hook UFunctions, mutate Blueprint state, drive custom UI, react to game
  events.
- **Process touch:** game client, but only when BE is provably absent.
- **BE risk:** zero, conditional on the detection layer being correct. We treat
  the detection layer as load-bearing security code: tested, fuzz-checked,
  fail-closed.

### Strategy D — External tool

- Companion apps that run as separate processes alongside the game.
- Public state via screen capture (DXGI/Windows.Graphics.Capture). Already
  used in our overlay (Auto-PIN F8).
- Action via `SendInput` (simulating keypresses).
- Can be done from any language (.NET, Rust, Tauri).
- **Process touch:** none on the game.
- **BE risk:** zero. Industry-standard helper-app pattern.
- Use cases: tactical map, build tracker, recipe lookup, kill feed, voice
  commands.

## The detection-and-refusal layer

Strategy C is the only delivery mode that touches the game process. Before any
injection, the loader runs this gate:

```
1. Enumerate processes:
   - if any of (BEService.exe, BEClient_x64.exe, BEDaisy.sys driver loaded,
     BattlEye Launcher, BattlEye Service Window) is present
     -> REFUSE. Log to local audit. Show user-facing notice.
2. Inspect SCUM launch arguments (via process command line):
   - if "-NoBattlEye" not present AND running in multiplayer mode
     -> REFUSE.
   - if "-singleplayer" / offline mode flag present -> ALLOW.
3. After injection, monitor connection state:
   - if the game client begins connecting to an official server (server list
     metadata says BE-required)
     -> auto-unload, terminate the runtime, log.
4. Watchdog thread:
   - re-runs steps 1+3 every 30 seconds while injected.
   - any change to BE-active -> auto-unload.
5. Audit log:
   - every refusal and every successful inject is written to
     <appdata>/TurdMOD/loader.log with timestamp, reason, mode.
   - never phones home; this is local-only diagnostics.
6. User-facing UI:
   - the manager UI surfaces the loader state in plain English:
     "TurdMOD scripting is OFF — you are connected to a BattlEye-protected
     server. TurdMOD content paks are unaffected."
```

The loader **fails closed**: if it can't determine BE state, it refuses. Better
to leave a feature off than to ban a player.

## Mod manifest — `mode` field

Every mod declares its delivery mode and the loader/manager enforces compatibility:

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

Compatibility matrix the loader enforces at install time:

| Environment                       | pak-content | server-side | offline-only | external-tool |
|---|:-:|:-:|:-:|:-:|
| Single-player (offline)           | ✅ | n/a | ✅ | ✅ |
| Private server, BE off            | ✅ | ✅ (server)  | ✅ | ✅ |
| Private server, BE on             | ✅ | ✅ (server)  | ❌ | ✅ |
| Official server                   | ❌ | n/a | ❌ | ✅ |

**Policy decision (Joel, 2026-05-08):** allow `pak-content` on private servers
regardless of BE state — both cosmetic and gameplay paks. Risk is the private
server admin's to manage; they can disable `~mods/` or ban offenders themselves
if they don't want mods on their server. The hard line stays at official
servers, where Gamepires/BE has authority over bans.

## Private-server-only policy — how the warning lands

Pak files mount at **game startup**, not per-server. Once a modded pak is
loaded, it's active for the whole game session. The manager UI surfaces this
at three points:

1. **Before activation** — explicit consent dialog:

   > "This pak modifies your SCUM client. **Use on private servers and solo
   > only.** Do NOT connect to an official Gamepires server with this mod
   > active. TurdMOD cannot prevent that connection — it's your responsibility.
   > [✓] I understand."

   Activation is blocked until the box is checked. Choice persists per-mod;
   first-time install always re-prompts.

2. **At launch** — the launcher offers two modes:

   - **Launch Vanilla** — moves the contents of `~mods/` to `~mods.disabled/`
     before starting SCUM. Guarantees a clean session for official-server play.
   - **Launch Modded** — runs the game with `~mods/` populated. Default for
     anyone who has at least one pak activated.

   The launcher icon / banner colour switches between the two modes so the
   player can never confuse them at a glance.

3. **Post-connect detection** — once in-game, the manager UI watches the
   active server (via overlay screen capture of the server name / via the
   loopback log feed if available) and shows a top-of-screen warning if it
   sees an official Gamepires server while `~mods/` is non-empty:

   > "⚠️ You have TurdMOD paks active on what looks like an official server.
   > Disconnect now and relaunch in Vanilla mode."

   The warning is informational; TurdMOD never force-disconnects the player.

## What goes in `docs/turdmod/permission-scope.md` (KTask #134)

Open questions to resolve with the permission grantor before any code ships:

1. **Who granted permission, when, and what is the durable record** (email,
   contract, DM, public statement)? Public statement is best.
2. **Strategy A scope** — are pak content mods in `~mods/` allowed on:
   - solo / offline?
   - private servers (BE off)?
   - private servers (BE on)?
   - official servers?
3. **Strategy B scope** — are server-side mods allowed on:
   - private servers operated by anyone?
   - official Gamepires servers?
4. **Strategy C scope** — is DLL injection + scripting in solo / no-BE
   environments explicitly allowed (we're presuming yes given Joel's "we have
   permission")?
5. **Strategy D scope** — explicit blessing for external tools using
   screen capture + SendInput (this is industry-standard but worth a paper
   trail).
6. **Distribution & branding** — is "TurdMOD" usable as a brand? Required to
   credit Gamepires? Required to disclose mod-related risks in our manager UI?
7. **BE Cooperation Mode** — does the permission include any chance of a
   whitelisted, signed TurdMOD loader running while BE is active? (Long shot, but
   if yes, it changes the matrix.)
8. **Revocation path** — under what conditions does the permission revoke?
   (E.g., a TurdMOD-distributed mod that includes a cheat → permission terminates.)
9. **Code signing** — should the TurdMOD loader DLL be code-signed by us
   (Azure Trusted Signing) and is signed-only enforcement required?
10. **Public-credit** — does Gamepires want public credit / a co-marketing
    moment when TurdMOD launches?

Once those answers are written into `docs/turdmod/permission-scope.md`, the
compatibility matrix above is locked in and we can start building.

## Phase plan (BattlEye-aware)

**Phase 1 — pak-only loader.** Strategy A. Lowest risk, highest immediate value
(textures, sounds, datatable overrides, recipe tweaks). No injection. Ships
first. Allowed on solo + every private server (regardless of BE state); blocked
for official servers via the launcher's Vanilla mode + post-connect warning
(see "Private-server-only policy" above).

**Phase 2 — server-side runtime.** Strategy B. Lua VM hosted in the dedicated
server process. No client-side anything. Targets the most-requested mod
categories (drop rates, custom missions, economy). Works on every private
server.

**Phase 3 — offline scripting.** Strategy C. The DLL loader + Lua VM, gated by
the detection layer, for solo and BE-off private servers. Enables UI mods,
custom HUDs, behavior tweaks the server can't do alone.

**Phase 4 — external tool framework.** Strategy D. SDK for companion apps
(screen capture + SendInput + a documented IPC channel between mod and our
overlay). This formalizes what the existing overlay's Auto-PIN already does.

**Phase 5 — distribution.** Mod registry, web browser, Discord `/mods`, all
mode-aware: a player on official servers sees only Strategy A and D mods in
their feed; private server owners see B; solo players see all.

## What this gives us that no other SCUM mod tool offers

- **Safety as a product feature.** The detection layer + manifest mode + UI
  surfacing is a unique-to-TurdMOD trust signal. No other UE mod loader treats
  BE-safety as a first-class concern.
- **Server-side-first.** The biggest mod use cases (drop rates, custom
  missions, economy) are solved with zero client risk. We ship value to
  private-server admins on day one without touching any player's BE session.
- **Permission as a wedge.** With a real permission grant from Gamepires we
  can do things the existing UE-modding scene cannot, like distribute pak mods
  through an official-feeling marketplace.

## Open technical questions

- **Pak hot-mount at runtime** vs. startup-only? `~mods/` is loaded at
  startup; runtime mounting needs `FCoreDelegates::OnMountPak` access, which
  is engine-API and likely needs the loader. Trade-off: startup-only = pure
  BE-safe; runtime = better DX but requires Strategy C boundaries.
- **Server-side Lua sandbox attack surface.** A malicious server-side mod can
  trash the server. We need capability flags + signing for any mod whose
  manifest declares `mode: server-side`.
- **External tool IPC.** A documented channel (named pipe / loopback HTTP)
  for tools to talk to our overlay or to other tools. Specify in Phase 4.
- **DataTable-row override conflicts.** Two pak mods that both edit
  `ILTN_Depository.MeM.MemoryModule_Level4` — last-loaded wins, but the
  manager UI must show the conflict.

## Footnotes

- See `MEMORY/feedback_no_battleye_risk.md` for the standing rule that no
  TurdMOD code can ship a BE-risk vector.
- See `MEMORY/feedback_log_game_build_diffs.md` for the build-diff system
  that flags mod compatibility breakage on patches.
- See `IDEAS.md` "Overlay screen-capture sensor — game-state via pixels
  (BattlEye-safe)" for the precedent for Strategy D.

---

## 2026-05-30 — what shipped: launcher↔loader handshake + our-servers gate

The DayZ-style launcher (`apps/turdmod-launcher`) and the safety wiring landed
on branch `feat/client-modded-launcher`. This section reflects the *current*
code; the strategy sections above remain the design rationale.

**Two launch paths, never a system-wide toggle.** Steam "Play" = vanilla + BE,
official servers fair game, install untouched. The launcher = `SCUM.exe` with
`turdmod_loader.dll` injected, connecting only to our BE-off servers. The choice
of which client to run *is* the toggle.

**Two on-disk contracts** (launcher → loader, under `%LOCALAPPDATA%/TurdMOD/`):

- **`launch-mode.json`** — the launcher writes
  `{ "mode": "private-be-off"|"solo"|"private-be-on", "server": {...} }` *before*
  it resumes the suspended SCUM process. The loader's `detect.rs::infer_mode()`
  reads it at DllMain time. **Fail-closed override wins:** a BattlEye module
  loaded in-process forces `PrivateBeOn` (loader stays inert) regardless of the
  flag — a stale or forged file can never enable modding while BE watches.
- **`mods/enabled.json`** — `{ "enabled": ["id", ...] }`. The launcher writes it
  on mod toggle; `runtime.rs::discover_and_load()` honors it. **Absent ⇒ load
  all** (back-compat with pre-launcher installs).

**Our-servers gate (the invariant).** The launcher only offers servers from
`GET /api/servers` (apps/turdmod-web) with `battlEye === false`, and
`turdmod_launcher_core::launch` refuses to launch against any server whose
`battle_eye` is true — the gate is in the shared injection lib, not just the UI,
so the CLI launcher inherits it. The `+connect` arg is built from the selected
allowlisted server, never free-typed.

**Residual risk — NOT closed by this MVP.** The launch-path gate only covers the
server the launcher connects *into*. A player who alt-tabs to SCUM's in-game
server browser and joins an official BE server is outside this gate's view. The
expected backstop is that a BE server rejects a BE-off client at handshake — but
that is **not proven here**, so do **not** claim the client "cannot be banned."
Closing this hole requires the RE spike (runtime server-IP gate in the loader)
tracked in IDEAS.md.
