# Bug history — turdmod

Append-only. Open bugs at top, closed bugs below. Each entry: ID,
date, symptom, root cause, fix (or workaround), commit hash if
resolved, related memory link.

When a bug closes, move it down to `CLOSED`. Don't delete entries —
the symptom→cause history is how the next session avoids re-diagnosing.

---

## OPEN

### B-011 — `MiscStatics::Test_ProcessAdminCommand` is silently dead (RE-CONFIRMED live 2026-05-23)
**Opened/re-confirmed:** 2026-05-23 00:54 PDT
**Symptom:** The bridge handler `runTestAdminCommand` returns `ok:true`, the PE hook on `Test_ProcessAdminCommand` fires, the bridge log records the dispatch — and yet ZERO in-game effect. Fired 8 commands live with Joel connected (`#SpawnItem Weapon_AKM_ES_C 1`, `#DestroyAllItemsWithinRadius 2`, etc.). All returned ok:true. Joel saw nothing in-game.
**Root cause:** SCUM's admin command dispatcher has an internal auth check that bails when called via PE bypass server-side. The auth context isn't a simple "is this a Server RPC" check (we'd see it fail at the UE4 layer); it's deeper inside SCUM's parser — likely checks for a valid `Chat_Server_*` initiator, network metadata, or admin-list membership keyed by SteamID against a real client connection.
**Notable:** The flags on `Test_ProcessAdminCommand` are `Final, Native, Static, Public, BlueprintCallable` — these SUGGEST it should work from any server context, but live behavior says otherwise. **The SDK flag set is NOT a reliable predictor for this function.**
**Implication:** Stop recommending `Test_ProcessAdminCommand` as the "universal admin parser unlock." The explore agent's 2026-05-23 brief built its Tier-1/2 ranking on this assumption — REJECTED. **Direct property writes are the only confirmed-working path for god-powers.**
**Workaround:** None for the admin parser itself. For each capability we need:
- **Boolean flags** (godMode, immortal, infiniteAmmo, superJump, etc.) → direct write to known property offset on Prisoner. ✅ Proven 2026-05-23 by Joel's super-jump.
- **Numeric stats** (HP, hunger, hydration, stamina, temperature) → find offset, direct float write. Need RE for component-level packed states.
- **Item spawn / strip inventory** → would need sigscan + PolyHook2 detour on SCUM's internal item-creation function, OR direct write to inventory component storage. Both are real RE work (multi-hour to multi-session).
- **Vehicle teleport / destroy** → `K2_TeleportTo` on the vehicle Actor (BlueprintCallable, non-RPC) WORKS. Find vehicle by owner via GObjects scan.
**Status:** Closed-as-dead. Mark `runAdminCommand` and `runTestAdminCommand` handlers as `// DEAD - documented in B-011` in the bridge source so future-me doesn't try to ship features built on them.

### B-010 — god-admin chat loop (bot replies to its own broadcasts)
**Opened:** 2026-05-22 22:46 PDT
**Symptom:** god-admin called `broadcastChat ok:true` 10+ times in a 45-second window after Joel's login, even though Joel only typed once (`#Pilot pause`). Pattern: god-admin sees a chat event, broadcasts a reply, the bridge's PE hook on `Chat_Server_BroadcastChatMessage` re-fires from the SERVER's own broadcast, scumpilot's `EngineEventPerception` folds it into `recentChat`, god-admin sees its own reply on the next tick and replies again.
**Root cause:** Bridge dispatch fires for ALL `Chat_Server_BroadcastChatMessage` calls — both player-initiated (correct) and server-initiated (the bot's own broadcasts). The chat event payload has empty `player` field for server broadcasts but god-admin's brain still reacts to it.
**Workaround:** Restart scumpilot to clear the chat buffer; Joel was visibly spammed once which already proves the loop exists.
**Real fix candidates:**
1. **Bridge side:** in `dispatch_engine_event` for EV_CHAT, check if `this_->Outer` resolves to a real PlayerController with non-empty PlayerName. If not, suppress the event (server-side broadcast — not a player chat).
2. **scumpilot side:** `EngineEventPerception` already exists; add filter `if (!c.player || c.player.length === 0) return;` in the chat case (engine-event-perception.ts line ~91 already has `if (!c?.text || !c.player) return;` — let me re-verify this is firing correctly; the loop suggests it's not catching the bot's broadcast somehow).
3. **god-admin side:** explicit "ignore chats where speaker.displayName is empty or matches bot identity" in the brain's perception fold.
**Priority:** High — Phase 1 item 2 (chat-driven response) is blocked by this. Without fix, every player chat triggers an infinite reply loop.
**Status:** Open. Joel verified the loop indirectly (10+ broadcastChat calls per single Joel message).

### B-009 — Companion example mod (welcome-screen) crashes SCUMServer on first login
**Opened:** 2026-05-22 22:27 PDT
**Symptom:** First player to log in triggered a SCUMServer crash with
all-`GameServer.exe!UnknownFunction` callstack. Trigger sequence
recorded in UE4SS.log:
1. `getOnlinePlayers: count=1` — YOUR_OWNER_NAME joined.
2. `sendChatLineToPlayer: player="YOUR_OWNER_NAME" channel=2 text="Welcome to the server, YOUR_OWNER_NAME!"` — companion's `examples/turdmod/welcome-screen/scripts/main.ts` fires on login event.
3. `[HOOK] event-dispatch enabled for HandleStartingNewPlayer (kind=2)` — bridge sees the login UFunction.
4. SCUM.log callstack dump → `LogExit: Executing StaticShutdownAfterError`.
5. Client reconnect attempt fails with `NetErrorUnauthorized`, server exits.
**Root cause hypothesis:** Race between the companion's welcome-screen
mod sending a chat to the new player and SCUM's own
post-login init pipeline. The chat dispatch fires before PlayerState
is fully replicated, SCUM's chat-routing code dereferences something
that isn't ready yet.
**Workaround:** Move all example mods out of `examples/turdmod/` to
`tmp/parked-companion-mods/`. Companion loads zero mods → no
race. Verified clean: companion logged "no server-side / offline-only
mods discovered" on restart.
**Real fix (future):** Either
(a) welcome-screen waits for a "player ready" signal from the bridge
(needs new event — bridge emits `playerReady` after a delay or after
seeing the player's first chat/movement), or
(b) bridge's outbound `sendChatLineToPlayer` defers if the target PC
isn't fully initialized. (a) is cleaner.
**Files parked 2026-05-22:**
- `examples/turdmod/welcome-screen/` → `tmp/parked-companion-mods/welcome-screen/`
- `events-manager/`, `kill-feed/`, `my-squad/`, `teleport/`,
  `vehicle-manager/` also parked defensively (likely safe, but
  removed for the clean test).
**Status:** Working around. Real fix deferred until after Phase 1+2
acceptance.

### B-006 — `setWeather` server-side works, SCUM tick reverts writes
**Opened:** 2026-05-22
**Symptom:** scumpilot's storyteller agent called `setWeather`;
bridge originally responded with `unknown method`. After shipping the
handler (commit `c85da75`), RPC returns `ok:true` and four
`FloatProperty` writes land on `WeatherController2` — but SCUM's
tick overwrites 3 of 4 within ~5 seconds back to baseline state.
**Root cause (refined 2026-05-22 ~21:46):** Per SDK
(`BP_WeatherController2_classes.hpp`), `_rainIntensity`, `_windIntensity`,
`_fogDensity`, `_baseAirTemperature` are all marked **Transient** —
they're the OUTPUT of a per-tick state machine that reads from
deeper INPUT state (`FMultistageRandomRoll _windIntensityRandom`,
curves like `_fogDensityVsSunIntensity`, etc.). Writing the outputs
gets clobbered on next tick. `_maxRainMilimeterPerHour` (the cap)
DOES stick because it's a config, not tick-driven.
**Live measurement:** at t=0 wrote `{rain:0.7, wind:0.56, fog:0.076, maxRain:14}`,
at t=+5s observed `{rain:0, wind:0.144, fog:0.412, maxRain:14}`. Only
maxRain persisted.
**Implications for FULL pass goal:** scumpilot's storyteller gets
`ok:true` and logs the rule firing — from scumpilot's frame of
reference, Phase 1 item 1 passes. But clients DO NOT see weather
change in-game because the values revert before SCUM's auto-snapshot
tick (`_sendReplicatedStateSnapshotInterval` at 0x164C) can pick
them up.
**Further finding (2026-05-22 ~21:55):** SCUM's `#SetWeatherControllerOverrideActive`
admin command is the only built-in override path, and the
`EWeatherControllerDebugOverrideType` enum has just **4 values, all
wind-related**: `WindAzimuth=0, WindIntensity=1, WindAzimuthForWaves=2,
WindIntensityForWaves=3`. **SCUM does NOT expose rain/fog/temperature
overrides at all.** This is an architectural limit in SCUM's design,
not a turdmod gap.

**Practical implications:**
- ✅ Wind can be controlled cleanly via the override system (~1-2 hours
  RE to find the Pad_1650 storage offset, then write 4 floats +
  4 active-flags).
- ❌ Rain / fog / temperature cannot be persistently changed
  through any SCUM-built-in API — the only way is to hook
  `AWeatherController2::Tick` (PolyHook2 detour) and either skip
  the state-machine recompute when a flag is set, or post-overwrite
  the values after each tick.

**Three paths to actual client-side weather change:**
1. **Wind-only override** via `Pad_1650[0x24]` storage. ~1-2 hours.
   Redefines "weather" as "wind" — works with SCUM's own design.
2. **Tick hook on `AWeatherController2::Tick`** to bypass state
   machine for rain/fog. ~2-4 hours. Highest payoff (visible rain),
   highest surgery.
3. **Accept storyteller's "rule fired + RPC ok" as FULL pass** for
   scumpilot's frame of reference. Visible weather is downstream of
   SCUM's design, outside turdmod's reach without 2.
**Related:** `docs/plans/priorities-1-7.md` F2, [[session-end-2026-05-22]],
[[feedback_scum_admin_auth_blocker]].

### B-005 — Phase C blocked on L3 (pak-handle creation)
**Opened:** 2026-05-22 PM
**Symptom:** `TurdMODHelloWorld_P.pak` doesn't mount even with the
full v3.1 + v4 + v5 hook stack live. L3 (pak-handle creation at
RVA `0x03b61be0`) returns null because the pak has no
structurally-valid `.sig`.
**Root cause:** SCUM verifies `.sig` structurally before crypto.
Generated `.sig` either has the wrong chunk hash or, possibly, RSA
verification fails downstream.
**Workaround:** None active. Two fix paths documented:
1. Generate structurally-valid `.sig` with correct chunk hash
   (cheaper; mirror `scripts/research/build-probe-sig.ps1`).
2. Hook L3 to return non-null handle (riskier — downstream crash
   risk when SCUM reads pak data through fake handle).
**Parked:** Phase C → scumpilot pivot. Will resume when scumpilot
acceptance ships.
**Related:** [[reference_pak_load_defense_layers]], [[project_v3_1_shipped]].

### B-004 — Inbound whispers not visible to scumpilot
**Opened:** 2026-05-22 (plan review)
**Symptom:** Phase 2 acceptance item 9 needs Joel whispering the bot
for "approve via whisper". Bridge has no detour on
`Chat_Server_SendPrivateMessage`, so incoming whispers aren't emitted
as events.
**Root cause:** Wave 2 didn't ship the whisper-inbound detour
(was deferred per `docs/plans/priorities-1-7.md`).
**Workaround (rejected as "partial" per goal-set 2026-05-22 21:30):**
Joel approves via chat (`pilot, approve <id>`) instead of whisper.
**Real fix in flight:** Add `Chat_Server_SendPrivateMessage` detour +
emit `whisper` event with `{from, to, message}`. Same detour pattern
as the existing `Chat_Server_BroadcastChatMessage` chat detour.
EngineEventPerception already has the case-stub ready (line 218 falls
through to "unknown event" — we'll add a `case "whisper":` similarly
to chat).
**Related:** scumpilot plan `~/.claude/plans/there-is-no-readme-fancy-eich.md`
Phase 2 brain extensions.

### B-007 — Bridge doesn't emit `logout` event
**Opened:** 2026-05-22
**Symptom:** Bridge emits `login` (Wave 1 — `GameModeBase::HandleStartingNewPlayer`)
but not `logout`. scumpilot's `EngineEventPerception` recognizes the
event shape but folds nothing because nothing arrives.
**Root cause:** Wave 1 logout deferred for a patternsleuth scan that
hasn't happened. Likely target: `GameModeBase::Logout` (UE4's standard)
or a SCUM-specific override. Companion's log-tail bridges the gap
today, but for the "FULL pass" goal, the bridge should emit it
natively.
**Workaround:** companion SSE picks up logout from log lines.
**Real fix in flight:** patternsleuth scan + detour. ~30 min.
**Related:** [[reference_bridge_events_protocol]].

---

## CLOSED

(Newest at top.)

### B-008 — `setTimeOfDay` delayed crash on large jumps
**Closed:** 2026-05-22 22:15 PDT
**Opened:** 2026-05-22 22:03 PDT (during live scumpilot integration)
**Symptom:** UE4SS Fatal Error modal with crashdump
`crash_2026_05_22_22_03_46.6050518.dmp`, ~50 sec after a setTimeOfDay
call jumped `_timeOfDay` from 7.83 → 22. Server initially appeared
alive (companion polling continued) but dismissing the modal killed
the process — main game thread was hung on the exception the whole
time.
**Root cause hypothesis:** Large time discontinuity (crossing
sunset / nighttime boundary in one write) triggers a downstream
callback — likely `OnRep_NighttimeDarkness` or similar — that
doesn't handle large deltas safely. The earlier [[B-003]] fix solved
the immediate nullptr-multicast crash but didn't address the
property-write side effect.
**Fix:** Clamp per-call setTimeOfDay delta to ±2 hours with 24-hour
wrap-around (shortest-path direction). Response includes new
`requestedHours` and `clamped` fields so callers know when clamp
activated. Storyteller's hourly cadence aligns with real time, so
after a few hours of continuous running the clamp is a no-op.
**Verified live 2026-05-22 22:15 PDT:**
- Small delta (22.47 → 22): clamped=false, applied unchanged.
- Big jump (22 → 8): clamped=true, applied=0 (wrap +2hr).
- Storyteller follow-up: clamped 0.09 → 22.09 via -2hr wrap.
- Server stayed alive across all three writes. No new crashdumps.
**Commit:** `f11abc9`.

### B-003 — NetMulticast UFunctions crash on nullptr struct params
**Closed:** 2026-05-22
**Symptom:** `setTimeOfDay` handler crashed SCUMServer
(crashdump `crash_2026_05_22_04_05_22.0611904.dmp`). Property write to
`_timeOfDay` completed first, then crash on the multicast call.
**Root cause:** Called `NetMulticast_SendStateSnapshot(nullptr)`
despite reflection declaring `Snapshot: StructProperty, paramsSize: 48`.
UE4 dereferences struct params without checking.
**Fix:** Skip the explicit multicast call. Rely on UE replication
picking up the property write. Commit `c89cf3f`.
**Memory:** [[ue4-struct-params-never-null]] — the rule.

### B-002 — Silent pipe-event starvation for read-only subscribers
**Closed:** 2026-05-22
**Symptom:** CLI tail consumer received 0 events in a 6-second window
despite the bridge firing `smoke.tick` at 1Hz.
**Root cause:** `Arc<Mutex<NamedPipeServer>>` in `serve_connection`
in `turdmod-server-loader/src/lib.rs`. The reader task held the mutex
during `read_frame()`, blocking the event-writer task. A subscriber
that never sent a request never released the lock.
**Fix:** `tokio::io::split(pipe)` to give reader + writer independent
halves. Commit `09fdaa8`.
**Memory:** [[feedback_pipe_split_silent_subscriber]].

### B-001 — User env vars dead through UAC
**Closed:** 2026-05-22
**Symptom:** `TURDMOD_PAK_BYPASS=1` set in parent PowerShell session
didn't propagate to elevated SCUMServer launched via
`Start-Process -Verb runas`. Bridge logged the env var as unset.
**Root cause:** `ShellExecuteExW(verb=runas)` strips parent env vars.
A user-scope `setx` is captured at process creation, not inherited
live.
**Fix:** Switched gate from env var to file flag
`C:\TurdMOD\pak_bypass.enabled`. Commit `5eea8e6`.
**Memory:** [[feedback_env_var_dead_through_uac]].

---

## How to file a new bug

1. Pick the next ID (`B-008`, `B-009`, …).
2. Add an entry under `OPEN` with: symptom (what you observed),
   root cause (what was actually wrong), workaround (temporary), real
   fix (when it lands). Always include a date.
3. Cross-link any related memory file with `[[slug]]`.
4. When closed, move down. Don't renumber.
