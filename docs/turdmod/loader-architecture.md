# TurdMOD loader architecture (Strategy C)

How the offline / single-player scripting layer is built, ordered by what
ships first.

For policy + when this strategy is allowed at all, see
`docs/turdmod/compatibility-policy.md`. For why we never run when BE is active,
see `docs/turdmod/battleye-safety.md`.

---

## Layer 0 — Detection-and-refusal (SHIPPED)

Lives in `apps/turdmod-cli/src/detect.ts`, wired to `turdmod scripting check` /
`turdmod scripting inject`.

This is the load-bearing safety code — every layer above it checks here first
and refuses on any uncertain result.

What it does:

1. **Process scan** via `tasklist /FO CSV /NH` — fast, no admin needed.
   Looks for: `BEService.exe`, `BEService_x64.exe`, `BEClient_x64.exe`,
   `BattlEyeLauncher.exe`. If any match, refuse.
2. **SCUM cmdline** via `Get-CimInstance Win32_Process` — looks at the
   running SCUM process's command line for `-singleplayer`, `-offline`, or
   `-NoBattlEye` flags.
3. **Mode inference**:
   - `solo` — `-singleplayer` / `-offline` present
   - `private-be-off` — `-NoBattlEye` present
   - `private-be-on` — neither flag present (default; BE assumed active)
   - `unknown` — SCUM not running OR cmdline unreadable
4. **Decision tree (fail-closed)**:
   - BE active → REFUSE
   - SCUM not running → REFUSE
   - mode is `solo` or `private-be-off` → ALLOW
   - mode is anything else → REFUSE
5. **Audit-friendly result** — every check returns `{ ts, beActive,
   beProcessesFound, scumRunning, scumPid, scumCmdline, mode,
   allowedToInject, reason }`. The reason field reads as a one-line audit
   log entry.

Future layers MUST call `detectScumEnvironment()` before doing anything
risky. CLI `turdmod scripting inject` already enforces this and exits 1 with
the reason if refused.

**What's not in Layer 0 and needs follow-up**:

- Watchdog thread that re-runs detection every 30s while injected (we don't
  inject yet, so nothing to watch).
- Server-list detection that can tell `private-be-on` from `official` —
  currently both fall under the same refusal bucket.
- Persistent local audit log under `<appdata>/TurdMOD/loader.log`.

---

## Layer 1 — Loader DLL (NOT YET BUILT)

A native DLL that ships alongside the game and gets loaded at SCUM startup.
Written in Rust.

Two viable injection vectors, **both compatible with the detection layer**:

### 1A. Proxy DLL (DLL hijacking)

Drop `<turdmod>.dll` next to `SCUM-Win64-Shipping.exe` with a name that the
game's import table resolves first — e.g. `dxgi.dll`, `version.dll`,
`winmm.dll`. Windows loader picks our DLL up before the system one. Our DLL
forwards the legitimate exports to the system DLL (so the game still works)
and, in `DllMain`, kicks off our runtime.

- Pros: No external launcher needed. Works with any way the player launches
  SCUM (Steam, shortcut, etc.).
- Cons: Loaded for every launch unless the player manually removes it. The
  detection layer in `DllMain` is the gate — if BE is detected, immediately
  return without doing anything else.

### 1B. Launcher injection

A separate `turdmod-launcher.exe` that:

1. Runs `detectScumEnvironment()` — refuses if not allowed.
2. Spawns SCUM with the appropriate flags (`-singleplayer` or whatever the
   mode requires).
3. Waits for the main window to appear, then `CreateRemoteThread` +
   `LoadLibrary` to inject the DLL.
4. Hands control back to the game.

- Pros: Player explicitly chooses to launch with mods (matches the Vanilla
  vs Modded launcher mode in `compatibility-policy.md` § three-warning UI).
- Cons: A second binary to ship; player has to use it.

**Decision for v1: ship both.** The launcher path is the default flow we
recommend; the proxy DLL is a power-user opt-in. Both gate on Layer 0.

### Rust crate layout (planned)

```
apps/turdmod-loader/
  Cargo.toml                 (workspace)
  crates/
    turdmod-loader-shared/      common types (DetectionResult, ManifestRef)
    turdmod-loader-dll/         the injectable DLL (cdylib, "dxgi" facade)
    turdmod-loader-launcher/    standalone .exe (wraps 'turdmod scripting check' + injects)
```

Outstanding decisions:
- DLL name: `dxgi.dll` (most compatible) vs `version.dll` (smaller footprint).
- Code signing: we already have Azure Trusted Signing wired up
  (`MEMORY/reference_azure_signing.md`); both the launcher and the DLL get
  signed.
- Process-capability checks: SCUM runs as user, not admin; we need to
  inject without UAC prompts.

---

## Layer 2 — Lua runtime (NOT YET BUILT)

Embedded LuaJIT (or PUC-Lua 5.4 if LuaJIT is too restrictive). Sandbox
defaults match the manifest's `capabilities` declaration:

- `filesystem: read` — opens via a path-allowlist that maps mod-relative
  paths into a read-only mount.
- `filesystem: readwrite` — only writes into `<appdata>/TurdMOD/<mod-id>/`.
- `network: localhost` — sockets restricted to 127.0.0.1.
- `network: any` — full sockets, only enabled if the mode is server-side.

LuaJIT picked over Lua 5.4 because:

- UE4SS uses LuaJIT, so existing UE-modder muscle memory transfers.
- Smaller binary, faster cold-start.
- FFI cdef → lets us bridge to UE objects without writing tons of glue.

The sandbox is enforced before the mod's main script runs. We override
`os`, `io`, `package` with allowlist-aware shims. The capability check is
pure Rust — Lua can't reach around it.

Hot reload: file watcher on the mod's script directory, on change tear
down the Lua state and rebuild from scratch (re-run the mod's `init`
hook). Mods are expected to be idempotent on init; we document this in
the SDK guide (#147).

---

## Layer 3 — UE4 hook injection (NOT YET BUILT)

This is the part that's actually hard. We need to:

1. Find a stable hook point for "frame tick" so mods can register
   per-frame callbacks.
2. Find or build a UFunction call → Lua bridge so mods can call
   `Player:GetHealth()` and similar.
3. Resolve UClass / UProperty by name through the UE reflection system.

UE4SS already does this. Two paths:

- **Take UE4SS as a dep / hard fork.** It's MIT-licensed UE 4 + 5
  scripting framework, well-tested. We bundle it inside the loader DLL
  and expose a TurdMOD-flavored API on top. Pro: months of work saved.
  Con: we own a fork going forward.
- **Build our own.** Read the same papers (`Hook UE4 UFunction Call`)
  and re-implement. Pro: full control + smaller binary. Con: months of
  work and bug-for-bug compatibility issues with future SCUM patches.

**Likely v1 choice: depend on UE4SS** — fork it into `vendor/ue4ss/` and
build it as part of the loader DLL. Document the relationship in the
README. Strip the UE4SS branding per the no-third-party-credit rule
(`MEMORY/feedback_no_third_party_devs.md`) — the TurdMOD UI never mentions
UE4SS by name.

---

## Layer 4 — TurdMOD Lua API (NOT YET BUILT)

The actual surface authors write against. Mirrors the API surface filed in
the `TurdMOD #6..#11` epics:

- `turdmod.content` — DataTable / Blueprint / locres overrides
- `turdmod.player` — spawn / death / inventory / stats / damage hooks
- `turdmod.world` — spawn / despawn / weather / time / custom POIs
- `turdmod.ui` — HUD widgets, custom menus
- `turdmod.persistence` — per-mod savefile slot (under `<appdata>/TurdMOD/<mod-id>/`)
- `turdmod.network` — server-authoritative event dispatch (server-side mode only)

API stability promise: every public function carries a `@since X.Y.Z` tag
in the auto-generated reference (#148) and we maintain backward compat
within v1.

---

## What changes when?

| Trigger | Layer affected |
|---|---|
| BattlEye config update | Layer 0 detection might need new process names |
| SCUM patches the executable | Layer 3 hook offsets need re-finding (UE4SS handles via signature scan) |
| SCUM updates UE engine | Layer 3 + Layer 4 (UFunction signatures may change) |
| New mode in Layer 1 | Layer 0 mode inference + manifest spec (Layer 0 of #138) |

The build-diff system (`MEMORY/feedback_log_game_build_diffs.md`) catches
changes that affect Layer 4 mods automatically. Layer 1–3 changes need a
manual re-test on each patch.

---

## Sub-tasks filed

Layer 1 (Rust loader DLL + launcher), Layer 2 (LuaJIT runtime + sandbox),
Layer 3 (UE4 hook injection), and Layer 4 (Lua API surfaces) each get
their own KTask. See the turdmod-loader-* tasks filed alongside #136.
