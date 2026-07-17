# Badass Server — Feature Build Plan

Vision (Joel 2026-06-07): make it a *bad ass* server. PvP zones (no-rules: kill/raid/rob/kidnap)
with **way more zombies + way more loot** (esp. high-value POIs); server-wide refresh events
("coffee & laundry" = clean all clothes + clear exhaustion); random objective events (kill X /
collect X) for rewards. Fun, cool, exciting.

## Feasibility ladder (grounded in reflection + turdmod-service audit)

### 🟢 Phase 1 — Objective events (pure turdmod-service Rust, proven pattern)
Build on the `warzone.rs` pattern (event-bus loop → bridge RPC). Already have kill-event tracking
+ economy (`C:\TurdMOD\data\economy.json`) + `scavenger_hunt.rs`/`quests.rs` frameworks.
- **Random objective events:** "Kill 10 zombies → +X coins", "Loot N high-value items", "Be first to POI".
  Subscribe to `kill`/`loot` events, count per-player, reward, announce. ~1 new module.
- **Coffee & laundry refresh** (own name, e.g. `!spa` / "Pit Stop"): scheduled + `!`-triggered.
  Needs a small bridge verb: iterate online players → `ClothesItem::SetDirtiness(0)` on equipped clothes
  + clear exhaustion (`#SetPrisonerExhaustion`/prisoner attr). Announce "The spa is open — everyone's
  fresh!". MEDIUM (one new bridge handler + service event).

### 🟡 Phase 2 — PvP zones via SCUM's NATIVE Custom Zone system
SCUM ships `CustomZoneRegistry` + `CustomZoneDataAsset` + `CustomZoneSettings*` (regions with
configurations: categories/options/events + handling methods). This is the real mechanism for
per-zone rule/loot/spawn overrides.
- **RE needed:** what the configuration "categories/options/events" actually control (loot mult?
  spawn density? PvP/raid flags?), and HOW to write a region+config (data asset vs save file vs
  runtime registry). Drive via a new bridge handler (`writeCustomZone`) once understood.
- **Then:** define PvP regions (no raid-protection, building damage on, kidnap allowed) with boosted
  loot + zombie density, especially over high-value POIs.
- Fallback if native config can't boost loot/spawns per-zone: pair zones with a turdmod-service loop
  that, while players are inside a PvP region, periodically `spawnItem`/`spawnZombie` around them
  (geofence via `getPlayerPositions` + region test) — gives the "more loot/zombies in PvP" feel
  without engine zone-RE.

### 🔵 Phase 3 — Polish / hype
- Zone HUD/overlay (entering PvP zone warning), kill streaks, bounties on PvP-zone players,
  Aetherius narrating events in-character, leaderboards for PvP-zone kills.

## Build/deploy reality
- turdmod-service is **Rust** → new features need `cargo build` + stage `turdmod-service-new.exe`
  to OVH; `go-live.ps1` swaps + restarts `TurdMODService`. New bridge verbs (SetDirtiness/zone) need
  the **TurdMODEngineBridge.cpp** rebuilt + DLL swap (server-off).
- Phase 1 objective events = service-only (no engine rebuild) → fastest to ship.

## Status: planning. Start = Phase 1 objective events (no engine dep).
