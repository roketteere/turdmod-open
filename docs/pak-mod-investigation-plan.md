# Pak-mod investigation — concrete plan

**Goal:** answer whether server-side Blueprint pak mods are a viable path for TurdMOD's Lite tier on managed hosts (G-Portal in particular). Written 2026-05-19.

Pak modding is UE4's documented moddability path: you build a `.pak` file containing Blueprint classes that derive from the game's own classes, drop it into the server's `Paks/` directory, UE4's pak loader mounts it at startup, and the new BP classes get registered into `GUObjectArray` next to the native ones. We've already proven (in `dumpAdminCommands`) that BP subclasses of `AdminCommand_*` get auto-discovered by SCUM's admin system the same way native classes do — that's the foothold this plan exploits.

This document is the spec for the investigation, not a feature design. The output of the investigation is **a go/no-go on building a real pak-mod toolchain**.

---

## TL;DR — the decision tree

```
                ┌─ Q1: Does G-Portal allow uploads to Paks/?
                │
        YES ────┼─ Q2: Does SCUMServer mount unsigned _P.paks?
                │
        YES ────┼─ Q3: How much of the gameplay surface is reachable from BP?
                │
        ENOUGH  ┴─ ✅ Build the pak-mod toolchain. Lite tier gets in-process
                                                    mods on managed hosts.
        
        NO at any step → fall back to:
          - FTP/RCON-only Lite (current state)
          - OR TurdMOD Hosted (we run the VPS, customers pay us)
```

Cheapest information first: **Q2 is testable on Joel's local SCUM Server today** — costs nothing, no G-Portal account needed, and if it fails the whole pak path is dead regardless of what G-Portal allows. Run Q2 first.

---

## The three unknowns

### Q1 — Does G-Portal mount custom paks for SCUM tenants?

**Hypothesis:** G-Portal exposes some directories via FTP/SFTP but typically locks down game-asset directories on shared hosts. SCUM specifically might allow `Paks/` writes if their mod policy is permissive; might block if they treat paks as game-integrity-critical.

**Verification plan** (in order of cost):

1. **Community intel (free, ~30 min).** Search G-Portal community forums + SCUM modding Discord + Reddit r/SCUMgame for posts like "g-portal custom paks", "g-portal mods scum", "pak upload denied". Look for tenant reports of trying this. Capture every concrete report (date, G-Portal SCUM version, outcome) in a findings table at the bottom of this doc.
2. **G-Portal panel reconnaissance (free, ~15 min).** G-Portal often publishes screenshots/docs of their per-game control panels. Confirm whether SCUM's panel has a "Mods" tab, a custom-pak upload field, or any moddability surface beyond INI editing. The presence/absence of such a tab is itself evidence.
3. **Tenant probe (cheap, ~1 week + $10).** Worst-case, spin up the cheapest G-Portal SCUM slot (typically 1-week / 4-slot tier). FTP into the install. Attempt to write a 64KB no-op file to `SCUM/Content/Paks/`. Outcomes:
   - Write succeeds + persists across restart → **Q1 is YES.**
   - Write succeeds but the file vanishes after restart → G-Portal's restart hook wipes user-added paks. **Q1 is NO** unless we find a side-channel.
   - Write rejected by FTP server → **Q1 is NO** at the access layer.
   - Path doesn't exist (paks live somewhere else than we expect) → reconnaissance needed before we conclude.

**Owner:** Joel (he has the G-Portal account if any; I can't open one).

**Done when:** we have a definitive yes/no plus a recorded log of the test (FTP transcript or screenshot saved into the findings table).

---

### Q2 — Does GameServer.exe mount unsigned `_P.paks`?

**Hypothesis:** SCUM's shipped content paks are AES-encrypted (we have the key per memory `scum_aes_key`), but most UE4 games that encrypt their own content still allow unencrypted, unsigned user paks to mount side-by-side via the standard `_P.pak` priority suffix. SCUM-specific policy is unknown — they may have set `bRequireEncryptedSignatures = true` or wired `OnPakSignatureCheckFailure` to reject.

**Verification plan:**

1. **Build a minimal no-op `_P.pak`** (~30 min once tooling exists per §"Tooling" below). Content: a single `UTexture2D` or `UDataAsset` with a unique name like `TurdMODProbe_v1`. No Blueprints yet — we're testing the mount, not the logic surface.
2. **Drop into local SCUM Server's pak directory** at `C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks\TurdMODProbe_P.pak`.
3. **Start the server.** No INI edit needed — confirmed 2026-05-19: SCUM already logs pak mount lines at default `Display` verbosity. Existing log lines we're already getting today:
   ```
   LogPakFile: Display: Found Pak file ../../../SCUM/Content/Paks/pakchunk9-WindowsServer.pak attempting to mount.
   LogPakFile: Display: Mounting pak file ../../../SCUM/Content/Paks/pakchunk9-WindowsServer.pak.
   LogPakFile: OnPakFileMounted2Time == 0.000486
   ```
   So our probe pak will leave a matching pair of lines (or won't) — that's the signal.
4. **Grep `SCUM/Saved/Logs/SCUM.log` for `TurdMODProbe` after restart. Three outcomes:**
   - Both `Found Pak file ...TurdMODProbe_P.pak` AND `Mounting pak file ...TurdMODProbe_P.pak` present → **Q2 is YES.**
   - `Found Pak file ...TurdMODProbe_P.pak` present but no `Mounting` line, plus a signature/encryption error line → **Q2 is NO** at the signature layer. The pak path is dead unless we find a way to sign as Gamepires (we won't).
   - No log line at all (silently ignored) → middle ground; SCUM may have a custom mount filter. Need to dig into how SCUMServer enumerates the Paks/ directory at boot.
   - Baseline note: as of 2026-05-19, the live `SCUM.log` shows no `Signature mismatch` lines for SCUM's own signed paks, so a failure mode for ours would be obvious in contrast.
5. **Verify the asset is actually loadable** by firing a bridge RPC: `dumpClasses` with grep `TurdMODProbe`. If the class appears in `GUObjectArray`, mount succeeded AND the asset registered. If it doesn't appear, mount may have happened but asset registry didn't pick it up (cookable vs raw issue).

**Owner:** me (build the pak), Joel (start the server, send me the log tail).

**Done when:** we have either the success log line OR a failure mode documented with the exact log message.

---

### Q3 — How much of the gameplay surface is reachable from Blueprints?

**Hypothesis:** Anything SCUM exposes as `BlueprintCallable` / `BlueprintNativeEvent` / `BlueprintReadWrite` is reachable. Static UFunctions in `MiscStatics`-style classes can't be overridden but can be called. Native C++ logic that doesn't go through Blueprint event graphs (chat broadcast paths, replication lifecycle, raw `ProcessEvent` hooks) is out of reach.

**The realistic surface for Lite-tier mods, in priority order:**

| Capability | Plausibility | Test |
|---|---|---|
| **Define new admin commands** | High — already proven via `_C` discovery. | Build `BP_AdminCommand_TurdModTest_C` with `_verb=TurdModTest`, override `Execute()` to call `BroadcastChatLine`. Fire `#TurdModTest` in-game. |
| **Override existing admin commands** | Medium — depends on whether the original was Blueprintable. SCUM's admin BPs use `_C` subclasses, so we have a precedent. | Subclass `AdminCommand_Announce_C`, override `Execute()` to prepend a tag. Replace via pak priority (`_P` suffix wins). |
| **Listen to in-game events** | Medium — depends on which classes broadcast Blueprint events. | Hook `OnPlayerLogin` / `OnPlayerLogout` if SCUM's GameMode emits them as BP delegates; otherwise we need a Tick poll. |
| **Add new replicated player state** | Medium — only if `SCUMPlayerState` is marked `Blueprintable`. Many UE4 games leave it native-only. | Subclass `SCUMPlayerState`, add a `Replicated` property, swap in via GameMode override. |
| **Override loot tables** | Low-Medium — SCUM's loot tables look like data assets (per our extraction work). Override-by-name should work. | Replace `BP_LootTable_*` asset in a `_P.pak` and check if loot generation picks it up. |
| **Modify replicated UMG widgets** | **Zero.** Memory `server-side-cdo-does-not-propagate` already settled this. Pak mods don't change that. | n/a |
| **Hook arbitrary C++ functions** | **Zero.** Requires DLL injection. | n/a |

**Verification plan:**

Phase Q3 into three milestones with go/no-go gates:

- **Q3.a — "Hello, world" admin command.** Build a pak containing one BP class that derives from a known-good `AdminCommand_*` base. The verb prints to admin chat. If this works, we have a real Lite-tier mod surface. If it doesn't, every richer capability below is also dead.
- **Q3.b — Override an existing verb.** Pak ships a `BP_AdminCommand_Announce_C` override that prepends `[MODDED]` to the message. Fire `#Announce hello` and confirm the prefix appears in chat. This proves we can replace, not just add.
- **Q3.c — Read/write authoritative state.** Pak ships a new admin command that reads a player's `_currency` or `_famePoints` property and writes a delta. Confirms BP reflection has authoritative write power.

If Q3.a passes we can ship something. If Q3.a-c all pass, the pak-mod tier is competitive with the bridge for most admin workflows.

**Owner:** mostly me (pak authoring + BP scripting), Joel (in-game testing + screenshots).

---

## Tooling we'd need

### Custom pak builder

UE4 ships `UnrealPak.exe` as part of any engine install. Three sourcing options:

1. **Epic Games Launcher → UE4 4.27.2 install (~30 GB).** Heaviest but canonical. `UnrealPak.exe` lives at `Engine/Binaries/Win64/UnrealPak.exe`. Comes with UE4 Editor (which we'll need for BP authoring anyway), so this is one decision, not two.
2. **UnrealPakTool (community fork, ~50 MB).** Standalone CLI that wraps the same pak format. Works for assembling already-cooked assets. Doesn't help with BP authoring.
3. **CUE4Parse-based packer.** We already use CUE4Parse for the `scumdump` extraction pipeline; the writer side exists in the same library. Pure C# / .NET. Smallest footprint but most code to write ourselves.

**Recommendation:** install UE4 4.27.2 once (Epic launcher → "Custom Engine Version" → 4.27.2 → uncheck Android/iOS targets to save ~10 GB). Use the bundled `UnrealPak.exe` for pak assembly. Use UE4 Editor for BP authoring against a stub project that imitates SCUM's class layout.

### BP authoring shim project

UE4 Editor needs to "see" SCUM's classes to let you subclass them. Two routes:

1. **Header reconstruction (the harder route).** Use the existing `tools/ue4ss-headers-gen` work + UEPseudo reconstruction (we have a working snapshot per memory `ue4ss_header_generator_status`) to produce `.h` files defining SCUM's classes. Build a stub UE4 C++ project against those headers. UE4 Editor recognizes them and lets you subclass.
2. **Asset-borrowing (the easier route).** Use Dumper-7 output (per memory `dumper7_setup_gotchas`) to extract SCUM's BP base classes as `.uasset`s. Drop into a stub UE4 project's `Content/SCUM/` folder. The editor sees them as available parents. Cleaner: no C++ build required.

**Recommendation:** start with route 2. If we hit an "asset references missing dependency" wall we don't want to chase, fall back to route 1.

### Mount-detection

Two layers:

1. **At build/test time.** Our existing log scraper (the FTP/log tail we'd build for the Lite tier anyway, per the FTP/RCON conversation) watches `SCUMServer.log` for `LogPakFile: Mounted pak file '...TurdMOD*'`. Boolean signal: is our pak mounted right now?
2. **At runtime, from inside the pak.** Author a sentinel admin verb like `#TurdModPing` that prints the running version. If `#TurdModPing` resolves, the mod is live. This is the runtime equivalent of the loader's `ping` method and serves the same diagnostic purpose for Lite.

### Version pinning per SCUM build

Pak-mod fragility is real. When SCUM updates a base class's property layout, BP-derived subclasses can break in ways that compile fine but crash at runtime. We need:

1. **Per-build BP manifest.** When we author a pak against SCUM build `23128915`, we record in the pak's metadata which classes we subclassed, which properties we touched, and what their offsets were at build time.
2. **Pre-flight check at deploy.** Before the Manager uploads a pak to a server, it queries the server's SCUM build via RCON (or the log header), looks up the matching `scumdump` snapshot, and verifies the classes/properties we depend on still exist with the same shape. If not, refuse to deploy.
3. **Per-build pak variants.** Ultimately, a TurdMOD mod ships as a *set* of paks (one per supported SCUM build), and the Manager picks the right one. Same shape as how Steam Workshop mods handle compatibility tags.

Out of scope for the investigation phase. Mention here so we don't build the toolchain without thinking about it.

---

## Phase order

Lowest-cost-to-decision first:

| Phase | Cost | Information unlocked |
|---|---|---|
| **0. Local pak mount probe (Q2)** | 30 min build + 5 min restart | If Q2 fails, the whole path is dead. Do this BEFORE anything else. |
| **1. Local "Hello world" BP admin verb (Q3.a)** | 1-2 days (UE4 install + BP shim project + author + cook) | If Q3.a works, we have a viable Lite-tier mod surface, even before knowing G-Portal's answer. |
| **2. G-Portal community intel (Q1, free pass)** | 30 min | Cheapest possible signal on Q1. If multiple community reports say "G-Portal locks paks", we save the trial-account cost. |
| **3. Q3.b + Q3.c expand the surface** | 1-2 days | Locks in how much we can actually ship. |
| **4. G-Portal tenant probe (Q1, paid)** | $10 + 1 week | Only if phases 0-3 all green AND community intel was inconclusive. |
| **5. Toolchain hardening + version pinning** | 1-2 weeks | Only invest if 0-4 are all go. |

Phase 0 should happen this week. Everything else gates on it.

---

## Honest gotchas

- **BP authoring is slow.** Even for a Hello World verb, the UE4 install + shim project + cook + pak loop is multi-hour the first time. Budget accordingly; don't promise a same-day demo.
- **Signed-pak rejection is hard to recover from.** If Q2 says SCUM requires Gamepires signatures, the pak-mod path is OVER. No workaround that doesn't involve patching the executable, which is the line. Have the Hosted-tier plan ready as the fallback.
- **G-Portal could change policy.** Even if Q1 is YES today, G-Portal can decide tomorrow that user paks are off-limits. Anything we ship for Lite needs to degrade gracefully to FTP/RCON-only.
- **Anti-cheat re-entry risk.** Memory `battleye_always_off` covers Joel's servers, but a Lite-tier customer might not have BattlEye disabled on their G-Portal box. Document this as a hard prerequisite for any pak-mod Lite feature: "your server must have BattlEye off."
- **SCUM versioning is fast.** Pak mods will break on roughly every SCUM patch. The version-pinning toolchain isn't optional; it's load-bearing for shipping this to anyone but Joel.

---

## Concrete next-step session plan

Optimized for "answer Q2 in one sitting":

1. Joel installs UE4 4.27.2 from the Epic Games Launcher (~30 GB; this is the long pole, kick it off first). Uncheck Android/iOS to save space.
2. While installing: I sketch the no-op probe pak structure (one `UDataAsset` subclass with `TurdMODProbe_v1` as the name).
3. UE4 install done → open Editor, create a blank C++ project, add the probe data asset, cook for `WindowsServer` target.
4. Use the bundled `UnrealPak.exe` to wrap the cooked asset as `TurdMODProbe_P.pak`. Filename suffix `_P` matters — it sets mount priority.
5. Copy to `C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks\TurdMODProbe_P.pak`.
6. Stop server, restart it. (No INI edit needed — pak mount logging is already on at default verbosity.)
7. I tail `SCUM/Saved/Logs/SCUM.log` via the Manager's existing Console / EngineConsolePage. Grep for `TurdMODProbe`. Look for the `Found Pak file` + `Mounting pak file` pair.
8. **Decision point.** If mounted → schedule Q3.a (Hello World admin verb). If rejected → record the failure mode, evaluate fallback to Hosted tier or Lite-via-FTP/RCON only.

The whole sequence is one session of focused work, gated on the UE4 install completing.

---

## Findings (filled in as the investigation progresses)

| Date | Phase | Outcome | Evidence |
|---|---|---|---|
| 2026-05-19 | Pre-Q2 prep | Logging already at needed verbosity; no INI edit required for Q2 probe. | `LogPakFile: Display: Mounting pak file ../../../SCUM/Content/Paks/pakchunk9-WindowsServer.pak.` present in live `SCUM.log` (line 5) — `Display` channel is on by default. |
| 2026-05-19 | Pre-Q2 prep | No `bRequirePakSignatures` / `bRequireEncryptedSignatures` override in INI. | Grep across `C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\` returns no matches. Whether the executable defaults to require-signed is the open Q2 question. |
| 2026-05-19 | Pre-Q2 prep | Confirmed SCUM Server install layout: `Content/Paks/` populated, `Saved/Config/WindowsServer/` is the runtime override dir, `Saved/Logs/SCUM.log` is the active log. | direct `ls` of paths. |
| 2026-05-19 11:55 AM | Q2 local probe | **NO — with hard crash.** SCUM enumerated our pak and started mounting it, then crashed the server when it couldn't find a `.sig` sidecar. Crash is SCUM-specific (`LogSCUM: Error:` prefix), not vanilla UE4. | `SCUM.log` lines 4-8: `Found Pak file...TurdMODProbe_P.pak` → `Mounting pak file...TurdMODProbe_P.pak` → `Warning: Couldn't find pak signature file '...TurdMODProbe_P.pak'` → `LogSCUM: Error: Requested application exit with the following error message:` → `LogSCUM: Error:   Pak file or matching sig file integrity compromised: ...TurdMODProbe_P.pak`. Probe pak built via `scripts/pak-probe/build-probe-pak.ps1`, deployed via `deploy-probe.ps1`, removed post-crash to unblock server boot. |
| 2026-05-19 | Q1 community intel | **MOOT.** Q2 NO invalidates the pak-mod path regardless of G-Portal's policy. G-Portal trial-account probe cancelled. (Separate G-Portal security-research project spawned in sibling repo; that work pursues access for its own sake under authorized bounty terms, not for pak-mod feasibility.) | n/a |
| (pending) | Q3.a Hello world | Gated on Pro-tier signature bypass landing first. | — |

---

## See also

- `docs/scum-internals/20-umg-server-driven-surfaces.md` — what server-driven UI primitives vanilla SCUM exposes
- `apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp` — the bridge's `dumpAdminCommands` BP-discovery walk (proof that BP subclasses register properly)
- Memory `dumper7_setup_gotchas` — extraction toolchain that produces the assets we'd subclass against
- Memory `ue4ss_header_generator_status` — header reconstruction if asset-borrowing route fails
- Memory `battleye_always_off` — anti-cheat prerequisite for any pak-mod tier
