# Roadmap — turdmod priorities 1-7

## Status (2026-05-22 ~05:30 PDT)

| Phase | Effort estimate | Actual status |
|---|---|---|
| **A — Quick wins** | 1-2 sessions | ✅ **DONE** — diff system end-to-end (synthetic `_diff.json` + UI card with build picker + drill-down), setEconomy* already shipped from earlier work |
| **B — Pak-bypass v2** | 1-2 sessions | ⚠ **PARTIAL** — v2 code shipped (call-trampoline-and-override) but still triggers SCUM.uproject modal because we override the return for ALL callers including SCUM's own resource validators. Default OFF; engine work proven cleanly (967k UObjects walked, setTimeOfDay live `3.81 → 6`). **Proper fix = caller-aware v3.** Deferred to a focused RE session. |
| **C — P1 Hello World pak** | 2-3 sessions | ⚠ **CLAUDE-SIDE COMPLETE, BLOCKED ON v3** (2026-05-22 ~12:00 PDT). Joel authored BPHelloWorld in UE Editor, cook + deploy ran clean, pak (2.78 KB) mounted in SCUM Server/Content/Paks/. Bridge handler `runHelloWorld` shipped + deployed with forgiving HelloWorld* substring match. **But v2 bypass triggers the SCUM.uproject modal → world init never completes → GUObjectArray empty → handler can't find the class.** Stub-uproject workaround crashed SCUM before bridgeReady. Verification gated on v3 caller-aware filter (Phase B re-open). |
| **D — P2 notifications** | 1-2 sessions | gated on C |
| **E — P3 GUI Builder runtime** | 3-5 sessions | gated on D |
| **F — Cleanup** | 1 session | gated on E |

**Recommended next move:** Phase C is unblocked. No download, no
install. Open the recipe at `docs/runbooks/uproject-cook.md` and
start step 2 (BP authoring shim). The first session goal: get
`BP_HelloWorld` cooked into a `_P.pak`, mounted, and visible via
`listClassInstances --pattern HelloWorld`. Add the `runHelloWorld`
bridge handler last.

---

## Commits shipped under this plan

| date | commit | what |
|---|---|---|
| 2026-05-22 | `cf7ebf3` | Plan committed to repo |
| 2026-05-22 | `1756d06` | Phase A2 diff card build picker + drill-down |
| 2026-05-22 | `148e125` | Phase B v2 (always-on; reverted) |
| 2026-05-22 | `917a809` | Phase B v2 default-off (engine works clean) |

---

## Context

Joel set `/goal` 2026-05-22 ~04:25 PDT after the Wave 2 verification
session ended. The seven priorities are the queued items from
[[session-end-2026-05-22]] memory; they span ~9-15 focused work
sessions and form the bridge between "Wave 2 shipped" and "GUI Builder
runtime alive" — the locked plan's North Star (`docs/server-side-custom-ui-plan.md`).

The seven, restated:

1. **Pak-bypass v2** — call-trampoline-and-override pattern
2. **FWeatherStateSnapshot struct layout** — for setTimeOfDay's force-broadcast
3. **P1 pak Hello World** — first BP class shipped in a pak (gates 4 + 5)
4. **P2 custom notifications via pak** — Option 2 from the locked plan
5. **P3 GUI Builder runtime** (custom UMG widgets + custom RPC) — the moat
6. **Dump diff system** — surface "patch notes the devs don't publish"
7. **Bridge handler shortlist** — setEconomy*, giveInventoryItem, dispatchClientRpc

This plan is a **roadmap**, not a single implementation. Each phase has
its own discrete verification gate; later phases are gated on earlier
ones, but quick-wins are pulled forward where possible.

---

## Dependency graph (critical path)

```
  ┌── Phase A (quick wins) ────────────────────────────────┐
  │   6 (diff UI) + 7 (setEconomy*) — independent          │
  └────────────────────────────────────────────────────────┘

  Phase B ── 1 (pak-bypass v2) ──┐
                                  │
                                  ▼ unlocks pak shipping
  Phase C ── 3 (P1 Hello World pak) ──┐
                                       ▼
  Phase D ── 4 (P2 custom notifications) ──┐
                                            ▼
  Phase E ── 5 (P3 GUI Builder runtime) ──┐ ← the moat
                                           ▼
                                       DONE

  Phase F (cleanup) ── 2 (struct fallback) + 7 finish
```

**Critical path:** B → C → D → E. Phase A + Phase F can be slotted
into any "cool-down" session.

---

## Phase A — Quick wins (1-2 sessions)

**Goal:** Ship the cheap items first while pak-bypass research happens.
Both items here are low-risk and INDEPENDENT of the pak chain.

### A1. Run scumdump diff against existing builds + verify

We already have `phase-diff.ts` complete and `dump_diff_summary` /
`dump_list_builds` Tauri commands wired (verified by Explore agent).
But no `_diff.json` has ever been generated.

- Today there's only one build (`v23128915`) so the diff against a
  prior build isn't meaningful yet. Either wait for a SCUM update OR
  fabricate a synthetic prior build for testing (copy `v23128915` →
  `v23128914`, mutate a few classes, re-diff).
- Run `pnpm diff` in `scumdump/` once and inspect the resulting JSON.

### A2. Diff Manager UI card

`apps/turdmod-manager/src/pages/DumpManagementPage.tsx` already
imports `dumpDiffSummary` and `dumpListBuilds` — but no card renders
them. Add a "Diff vs previous build" section to the page with:

- Summary counts: "+47 / -3 / ~12 changed" per Phase A category
- Build picker dropdown (uses `dumpListBuilds`)
- Drill-down on click: shows the actual added/removed/changed lists
- Optional v2: "Export as markdown" button

**Files:**
- `apps/turdmod-manager/src/pages/DumpManagementPage.tsx` — new section
- `apps/turdmod-manager/src/hooks/useDumpStatus.ts` — `useDumpDiffSummary`
  may already exist; verify and reuse

### A3. Priority 7 (partial) — setEconomy*

Per `bridge-handler-candidates.md` this is Tier S, HIGH confidence,
no smoke test needed. Wraps `ConZEconomyManager::NetMulticast_UpdateGoldPriceMasterMultiplier`
+ `NetMulticast_UpdateTradeablePriceMultiplier`.

- Mirror the recently-shipped `setFamePoints` / `setCurrencyBalance`
  handler pattern in `apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp`.
- Add the smoke card to BridgeSmokePage.
- Build + deploy.

**Effort:** Phase A total = 1-2 sessions.

---

## Phase B — Pak-bypass v2 (1-2 sessions)

**Goal:** Replace always-return-0 with call-trampoline-and-override.
Preserves the original validator's side effects (so reflection still
inits) AND keeps probe paks loading.

### B1. Inspect SCUM's pak validator signature

Use the existing `sigscan` skill OR direct patternsleuth to read the
function at `0x143b5b530`. Determine:

- Calling convention args (rcx, rdx, etc.)
- Side effects (what state does it set up that downstream needs?)
- Whether the original validator calls `ExitProcess` on a bad pak
  (the 60s `server_hooks.rs` guard handles that for us)

### B2. Rewrite `hooked_pak_validator`

`apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp` ~line 226:

```cpp
extern "C" int64_t hooked_pak_validator()
{
    // Call original to do all its state setup + get the real verdict.
    auto orig = reinterpret_cast<int64_t(*)()>(g_pak_validator_trampoline);
    int64_t real_verdict = orig();

    // Always claim valid, regardless of what the real validator said.
    // The 60s ExitProcess guard catches anything the original tries
    // to do on a bad-pak path. Our probe paks should now load AND
    // reflection should populate.
    return 0;
}
```

If the function takes args (rcx etc.), the trampoline call needs
forwarding. Forwarding from a no-arg C++ stub means passing whatever
happens to be in rcx — usually OK because the trampoline expects them
to be there from the caller's setup.

### B3. Verify: SCUM boots, reflection populates, probe pak loads

Verification matrix:
- Without pak: SCUM boots cleanly → bridgeReady fires → `listClassInstances` returns non-zero scanned.
- With probe pak: SCUM mounts it → `dumpClasses` includes a `TurdMODProbe*` class → no modal.

**Files:**
- `apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp`
- Default `TURDMOD_PAK_BYPASS` to ON post-fix (since the bug is gone).

**Risk:** if v2 also breaks reflection (unlikely but possible), fall
back to env-gated v1 + accept that pak shipping requires opt-in.

**Effort:** 1-2 sessions.

---

## Phase C — P1 Hello World pak (2-3 sessions)

**Goal:** Ship ONE custom BP class in a `.pak`, prove the toolchain
end-to-end, capture the cook recipe as a runbook.

### C1. UE 4.27.2 install + project scaffold

Per `docs/pak-mod-investigation-plan.md` line 122 — install UE 4.27.2
from Epic Launcher (uncheck Android/iOS). Long pole: ~30 GB download.

Scaffold a stub UE4 project at `apps/turdmod-helloworld-pak/`:
- Minimal C++ project (no game content)
- One BP class: `BP_HelloWorld`
- One UFunction the bridge can call: `BroadcastHelloWorld(message: FString)`
  that internally calls `MiscStatics::BroadcastChatLine(this, message, 1)`
  (Squad channel for visibility)

### C2. Cook recipe → runbook

New file `docs/runbooks/uproject-cook.md`:
- Exact `UnrealPak.exe` invocation
- Cook target: `WindowsServer`
- Asset path conventions (must match SCUM's pak content layout)
- Where to drop the resulting `.pak` (`SCUM Server/SCUM/Content/Paks/`)

### C3. Manual smoke + bridge handler

- Drop the cooked pak; restart SCUMServer (pak-bypass v2 active).
- Bridge handler `runHelloWorld` finds `BP_HelloWorld` via
  `dumpClasses`, calls `BroadcastHelloWorld("hello from pak")` via
  ProcessEvent.
- Verify chat line appears in-game.

### C4. Memory + IDEAS update

Pin the pak's compile/cook hashes; document the asset-path conventions
so a future SCUM patch's cook reproduces.

**Files:**
- `apps/turdmod-helloworld-pak/` — new UE4 project
- `docs/runbooks/uproject-cook.md` — new
- `apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp` — add
  `runHelloWorld` handler
- `apps/turdmod-manager/src/pages/BridgeSmokePage.tsx` — add card

**Risk:** asset-path conventions vary by SCUM build. P5 of the locked
plan addresses version-pinning; for now we just pin to v23128915.

**Effort:** 2-3 sessions (download + first-cook learning curve).

---

## Phase D — P2 custom notifications (1-2 sessions)

**Goal:** Ship our own `BasicNotificationDescriptionData` assets in
the pak; bridge dispatches `NotificationsManager::NetMulticast_RequestNotification`
to make custom toasts appear in-game.

### D1. Author 3-5 notification asset variants in the pak

Continue from Phase C's UE4 project. Each asset is 56 bytes per the
locked plan summary:
- FText message
- icon (Texture2D ref)
- duration (float)

Variants: success/warning/error/info/announce (different colors).

### D2. Bridge handler `sendNotification`

`apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp`:
- Locate `NotificationsManager` singleton (find_first_instance_of_class).
- Find `NetMulticast_RequestNotification` UFunction.
- Params: `{ player: <name|all>, kind: <variant>, message?: <override> }`.
- Build params block, dispatch.

### D3. Re-wire AdminPage notification toggle

The existing `AdminPage.tsx` notification UI sends `broadcastChat`
with `chatType=N`. Re-route it through `sendNotification` for the
toast variants; keep `broadcastChat` for plain chat.

### D4. Verify in-game

Manager → Admin → Send notification → player sees branded toast.

**Files:**
- UE4 project (Phase C's)
- Bridge cppmod
- `apps/turdmod-manager/src/pages/AdminPage.tsx`

**Effort:** 1-2 sessions.

---

## Phase E — P3 GUI Builder runtime (3-5 sessions) ← the moat

**Goal:** Ship custom UMG widgets in the pak + a custom Multicast RPC
that the bridge can dispatch. THE differentiator per IDEAS.

### E1. Author one minimal UMG widget class

`BP_TurdmodBanner` — single text label, fixed style. Just enough to
prove the runtime can instantiate it.

### E2. Author a custom Multicast RPC class

`BPC_TurdmodWidgetDispatcher` with:
- `NetMulticast_ShowWidget(widget_class_path: FString, params_json: FString)`
- Client-side: BP code constructs the widget, sets text from params,
  adds to viewport.

### E3. Bridge handler `pushCustomWidget`

- Locate the dispatcher singleton (or the GameMode that owns one).
- Dispatch the multicast with the widget path + params.
- For player-targeted variants: use a Client_* RPC instead of multicast.

### E4. Wire Manager's existing GuiBuilder

`apps/turdmod-manager/src/pages/GuiBuilderPage.tsx` already exists.
Wire its "Push to player" button to `pushCustomWidget`.

### E5. Iterate

Once the runtime is alive, expand:
- Multiple widget classes in the pak (banner / panel / modal / menu)
- Player-targeted vs broadcast variants
- "Preview as <player>" mode

**Files:**
- UE4 project (Phase C-D's)
- Bridge cppmod
- `apps/turdmod-manager/src/pages/GuiBuilderPage.tsx`

**Risk:** This is the most unknown territory. Custom RPCs in a pak
must register correctly into UE's UFunction table; if registration
fails, multicast dispatch silently no-ops. Smoke test EARLY.

**Effort:** 3-5 sessions.

---

## Phase F — Cleanup (1 session)

### F1. Priority 7 finish — giveInventoryItem + dispatchClientRpc fate

- `giveInventoryItem`: implement; smoke-test against the auth blocker
  via probeQuestHandlers pattern. If it works, ship. If it silently
  no-ops, document + reroute via spawnItem (existing bypass).
- `dispatchClientRpc`: per Explore agent, NOT in `bridge-handler-candidates.md`.
  The contract's "Path B for visible welcome panels" intent is
  fulfilled by Phase E's `pushCustomWidget`. Update
  `reference_admin_command_routes` memory to redirect future readers
  to that handler. No separate handler needed.

### F2. Priority 2 fate — FWeatherStateSnapshot

Per Explore agent, the struct is opaque (48 bytes, zero reflected
fields). Three possible outcomes:

1. **Best case:** `FWeatherReplicatedStateSnapshot{}` (zero-init) +
   pass to NetMulticast_SendStateSnapshot doesn't crash AND
   correctly broadcasts. Ship it.
2. **Middle case:** Zero-init doesn't crash but doesn't broadcast
   useful state either. Skip permanently — rely on UE4's normal
   replication, which is what v1 already does. Document.
3. **Worst case:** Zero-init crashes (likely if the struct has
   internal pointers UE dereferences). Skip permanently. Document.

Effort: 0.5 session for one experiment. Don't sink more than that
into Priority 2 — the current "skip multicast" behavior already
works for all known consumers.

**Effort:** 1 session.

---

## Sequencing recommendation

| Phase | Effort | Slottable into |
|---|---|---|
| **A — Quick wins** | 1-2 sessions | Any cool-down sessions |
| **B — Pak-bypass v2** | 1-2 sessions | Dedicated session (critical) |
| **C — P1 Hello World pak** | 2-3 sessions | After B confirms |
| **D — P2 custom notifications** | 1-2 sessions | After C ships |
| **E — P3 GUI Builder runtime** | 3-5 sessions | After D ships — biggest rock |
| **F — Cleanup** | 1 session | After E |

**Total: 9-15 sessions.** Front-load A in parallel with B-research
to ship value continuously. E is the biggest unknown and the longest
tail.

---

## Verification per phase

| Phase | Pass criteria |
|---|---|
| **A1** | `_diff.json` exists at `scumdump/data/extracted/v23128915/_diff.json` |
| **A2** | Manager Dump Management page shows the diff card with real data |
| **A3** | BridgeSmokePage setEconomy* card fires; in-game trader prices change |
| **B** | SCUM boots without modal AND with pak-bypass v2 active. `listClassInstances` returns `scanned > 0`. Probe pak mounts. |
| **C** | `dumpClasses` includes `BP_HelloWorld`. `runHelloWorld` bridge handler produces in-game chat. |
| **D** | Manager → Admin → Send notification → player sees the toast variant we shipped. |
| **E** | `pushCustomWidget` causes a `BP_TurdmodBanner` to appear on a player's screen with the text we sent. |
| **F** | `giveInventoryItem` either works or is documented as blocked. Priority 2 outcome documented. |

---

## Out of scope for this plan

- Wave 1 logout (separate binary signature scan task)
- Wave 3 spawn handlers (Wave 3 work)
- Wave 4 events (kill / vehicle / weather / time)
- TeKi Bridge extraction
- Dev-app launcher GUI
- UI/UX Maker visual editor (depends on Phase E's runtime first)
- Standalone UE5 game

These items are in `IDEAS.md` for future planning sessions.

---

## Critical files referenced

```
# Source-of-truth plans (READ before each phase)
C:/Development/Claude/turdmod/docs/server-side-custom-ui-plan.md
C:/Development/Claude/turdmod/docs/pak-mod-investigation-plan.md
C:/Development/Claude/turdmod/docs/bridge-handler-candidates.md

# Phase A
C:/Development/Claude/scumdump/src/phase-diff.ts                    READ (already complete)
C:/Development/Claude/scumdump/src/cli.ts                           RUN `pnpm diff`
C:/Development/Claude/turdmod/apps/turdmod-manager/src-tauri/src/dump_commands.rs  READ
C:/Development/Claude/turdmod/apps/turdmod-manager/src/pages/DumpManagementPage.tsx EDIT
C:/Development/Claude/turdmod/apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp EDIT (handlers)

# Phase B
C:/Development/Claude/turdmod/apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp EDIT (hook)
C:/Development/Claude/turdmod/tmp/build-bridge.cmd                  RUN

# Phase C-E
C:/Development/Claude/turdmod/apps/turdmod-helloworld-pak/          NEW (UE4 project)
C:/Development/Claude/turdmod/docs/runbooks/uproject-cook.md        NEW
C:/Development/Claude/turdmod/apps/turdmod-manager/src/pages/AdminPage.tsx   EDIT (D)
C:/Development/Claude/turdmod/apps/turdmod-manager/src/pages/GuiBuilderPage.tsx EDIT (E)

# Reference (existing utilities to reuse)
find_first_instance_of_class()    bridge cppmod
find_pc_by_player_name()          bridge cppmod
find_ufunction()                  bridge cppmod
find_property_offset()            bridge cppmod
get_function_param_offsets()      bridge cppmod
emit_engine_event()               bridge cppmod (per session-end-2026-05-22)
```

---

## What to commit to next session

Recommend starting with **Phase A1+A2** (diff system completion) as
the easiest win — backend is done, just need the React UI. ~1 session.
Then **Phase B** (pak-bypass v2) as the highest-leverage move because
it unblocks the entire pak chain.

E (the GUI Builder runtime) is the moat — but be honest about its
3-5 session weight. Don't start E without 1-2 clear sessions blocked
out.

If you want this plan executed step by step in order: pick A first
(quick taste of motion), then B (the unlock), then C-D-E sequentially.
Phase F slots in at the very end.
