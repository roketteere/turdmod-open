# TurdMOD Live Test Pass — WhalleyBot Parity Verification

Goal: flip each parity feature from 🔧 (coded+deployed) to ✅✅ (PROVEN BY BOTH) by firing
representative in-game commands on OVH. **Joel types the command in-game; I watch UE4SS.log for the
service's response; Joel confirms he saw it.** Testing one core command per feature proves the module
(175 total commands exist — full variant coverage is optional later).

**Flow:** do a group, ping me "done G1" (etc.), I read UE4SS.log + confirm, we check the box.

Legend: `[ ]` untested · `[~]` typed, awaiting my log-confirm · `[x]` ✅✅ PROVEN BOTH · `[!]` failed/bug

---

## G1 — Economy / banking  (`banking.rs`, `economy.rs`)
- [ ] `!bal`  — show your coin balance
- [ ] `!daily` — claim daily login reward (loyalty)
- [ ] `!transfer <player> 100` — send coins (Lilac↔YOUR_OWNER_NAME)

## G2 — Lottery / gambling  (`lottery.rs`)
- [ ] `!lottery` — lottery status / buy ticket
- [ ] `!slots` or `!coinflip` — gamble

## G3 — Shops / market / auction  (`player_shops.rs`, `auction.rs`)
- [ ] `!shop` — open shop / list
- [ ] `!buy <item>` and/or `!sell <item>`
- [ ] `!market` or `!bid` — auction

## G4 — Kits / care packages  (`kits.rs`)
- [ ] `!kits` — list kits
- [ ] `!kit <name>` or `!care` — claim a kit/care package

## G5 — Factions / clans / territory  (`factions.rs`, `clans.rs`, `territory.rs`)
- [ ] `!faction` — faction status/menu
- [ ] `!clan` — clan info
- [ ] `!territory` / `!terr` — claim/POI

## G6 — Leaderboards / stats  (`leaderboard.rs`, `scoreboard.rs`)
- [ ] `!leaderboard` — overall board
- [ ] `!topkills` — kill ranking
- [ ] `!mystats` / `!kd` — personal stats

## G7 — Prestige / progression  (`prestige.rs`, `reputation.rs`, `achievements.rs`)
- [ ] `!prestige` — prestige status
- [ ] `!rep` — reputation
- [ ] `!achievements` / `!title`

## G8 — Vehicles  (`vehicle_ownership.rs`, `vehicle_registry.rs`, `vehicle_interactions.rs`)
- [ ] `!myride` / `!vehicles` — your vehicles
- [ ] `!vspawn <type>` or `!garage` — spawn/retrieve
- [ ] `!insure` / `!lock` / `!honk` — interactions

## G9 — Taxi / fast travel  (`fast_travel.rs`, `convoy.rs`)
- [ ] `!taxi` or `!travel` — request taxi / fast-travel
- [ ] `!destinations` / `!stops` — list

## G10 — Quests / bounties  (`quests.rs`, `scavenger_hunt.rs`, `npc_contracts.rs`)
- [ ] `!quests` — quest list
- [ ] `!bounty` / `!bountyboard` — bounties
- [ ] `!scav` / `!contract` — scavenger / NPC contract

## G11 — Events  (`warzone.rs`, `racing.rs`, `duels.rs`, `fishing_tournament.rs`)
- [ ] `!event` — current/next event
- [ ] `!race` or `!duel` — start/join
- [ ] `!warzone` / `!wzstatus` · `!fish`

## G12 — Jail / moderation  (`jail.rs`, admin)
- [ ] `!jailstatus` — your status
- [ ] `!jail <player>` (admin) / `!unjail`
- [ ] `!warn` / `!whois` / `!wanted`

## G13 — Raid protection  (`safe_zones.rs`, `zilla_protection.rs`)
- [ ] `!raidstatus` — raid window status
- [ ] `!protect` / `!protectinfo` — protection
- [ ] `!raidtimes`

## G14 — God / admin powers (engine bridge)  ⚠️ paced — destructive ones consent-gated
- [ ] `!heal` / `!cure` — heal self
- [ ] `!god` — godmode toggle
- [ ] `!spawn <item>` — spawn item
- [ ] `!tame` — tame animal

## G15 — Personas / AI  (scumpilot + service)
- [x] **Aetherius chat→spell** — already ✅✅ PROVEN (broadcastChat + setTimeOfDay)
- [ ] `!doc` — Doctor persona
- [ ] `!vera` / `!ziggy` / `!ask` — other personas / AI

---

## Already PROVEN BY BOTH (no retest needed)
- ✅✅ Aetherius chat→spell loop · `broadcastChat` · `setTimeOfDay` · consent gating · owner-override

## Results log
_(I append per-group confirmations here as we go, with the UE4SS.log evidence line + timestamp.)_

| Group | Result | Evidence (UE4SS.log) |
|---|---|---|
| | | |

_Started 2026-06-07._
