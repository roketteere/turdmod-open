# TurdMOD vs WhalleyBot — Feature Parity Checklist

Comparison of [WhalleyBot](https://scum.wiki.gg/wiki/WhalleyBot) (the dominant commercial SCUM
server bot) against TurdMOD / scumpilot. Built 2026-06-07, **corrected via lobe audit of the
turdmod-service source** (first draft drastically under-counted what's shipped).

**WhalleyBot pricing:** Free `$0/mo` · Premium `$15/mo`.

**Bottom line:** TurdMOD has a **coded + deployed equivalent for essentially every WhalleyBot
feature** (the `turdmod-service.exe` Rust service — 15.9k LOC, ~50 modules, live on OVH) **and
exceeds it** on an entire axis WhalleyBot can't touch: engine-tier UE control + an AI god-admin.
The open question is no longer "do we have it" (we do) — it's **"is each feature live-PROVEN."**

### Status legend
| Mark | Meaning |
|---|---|
| ✅✅ | **PROVEN BY BOTH** — Joel saw it in-game *and* I confirmed via logs/RPC |
| ✅ | Verified by me (RPC/log) |
| 🔧 | **Coded + deployed** (in `turdmod-service`/`turdmod-bot` on OVH), per-feature live-proof pending |
| ➕ | We **exceed** WhalleyBot (no equivalent on their side) |
| ❌ | Not built |
| ⚠️ | N/A by design |

---

## A. WhalleyBot FREE tier → TurdMOD

| WhalleyBot | TurdMOD equivalent | Status |
|---|---|---|
| Discord integration + role/channel sync | `turdmod-bot` (`role-sync.ts`, gateway, live-feed, consent webhook) | 🔧 |
| Kill feed stats + rankings | `leaderboard.rs` + `scoreboard.rs` + `feed-bundle` + `showKillFeedNotification` (`!topkills`/`!kd`/`!top`) | 🔧 |
| Chat / admin / public logs | companion log-tail SSE + runner audit JSONL | ✅ |
| Player activity monitoring | `getOnlinePlayers` ✅ + `login` events ✅ | ✅ |
| VAC / game-ban screening | `vac_screening.rs` + `mods/vac-screening/` | 🔧 |
| Bot status / info commands | `getServerStats` ✅ + `!help`/`!mods`/`!rules` | ✅ |

## B. WhalleyBot PREMIUM tier → TurdMOD

| WhalleyBot | TurdMOD equivalent | Status |
|---|---|---|
| In-Game Chat (IGC) | `broadcastChat` + `sendChatLineToPlayer` + **AI chat→spell (Aetherius)** | ✅✅ ➕ |
| In-game overlay (HUD) | overlay framework (`OVERLAY_ENABLED`) + `sendHudMessage` + custom panels | 🔧 |
| Server-management dashboard | `apps/web` (turdmod-manager, ~250 config keys) | 🔧 |
| World events / World War | `warzone.rs`, `convoy.rs`, `racing.rs`, `duels.rs`, `fishing_tournament.rs`, `scavenger_hunt.rs` + native `#ScheduleWorldEvent` | 🔧 |
| Banking + loyalty rewards | `banking.rs` + `economy.rs` (`!bal`/`!daily`/`!claim`/`!transfer`/`!value`) | 🔧 |
| **Lottery** | `lottery.rs` | 🔧 |
| Shop / packs / kits | `player_shops.rs` + `auction.rs` + `kits.rs` | 🔧 |
| Item spawning + spawn-code editing | `spawnItem` + `placeItemInInventory` + 217 native `#Spawn*` | 🔧 |
| **Item scaling (`#WUMBO` 0–50×)** | `SetActorScale3D` new bridge verb (proven possible) | ❌ ~1 build cycle |
| Taxi system | `fast_travel.rs` + `convoy.rs` (+ "Pilot Taxi" AI roadmap) | 🔧 |
| **Traders** (custom inventory) | `npc_contracts.rs` + `npc/services.rs` | 🔧 |
| **Bounties / quests** | `quests.rs` + `scavenger_hunt.rs` + native `#Quests` | 🔧 |
| **Factions** (POI / CTF / claims) | `factions.rs` + `clans.rs` + `territory.rs` + CTF events | 🔧 |
| Squad member location tracking | `listSquads` ✅ + `getPlayerPositions` ✅ | ✅ |
| Developer API | **engine bridge JSON-RPC (101 handlers) + service `:9090` + scumpilot CLI** | ✅ ➕ far deeper |
| BattleMetrics / NightKingdoms sync | — (not found in audit) | ❌ verify |
| Host integrations (G-Portal/Nitrado/…) | self-hosted engine tier; G-Portal **can't** run engine tier | ⚠️ N/A by design |

## C. TurdMOD features WhalleyBot has NO equivalent for (➕)

- ➕ **AI god-admin (Aetherius)** — chat→spell, persona, consent-gated magic. ✅✅ PROVEN.
- ➕ **Engine-tier RPC** — 101 handlers, live reflection, memory read/patch (`readMemory`/`patchInstructions` ✅). WhalleyBot is RCON/log-tier.
- ➕ **Persona fleet** — Bouncer/Doctor/Mechanic/Storyteller + `!vera`/`!ziggy`/`!ask`.
- ➕ **Owner override + consent queue** (`#pilot pause/resume`, destructive-verb approval). ✅✅ PROVEN.
- ➕ **Extra game systems** WhalleyBot lacks: `prestige`, `reputation`, `achievements`, `referral`, `companions`, `jail`, `safe_zones`/`zilla_protection` (raid protection), `vehicle_ownership`/`registry`/insurance, `racing`, `duels`, `mechman`, `voting`, `map_markers`.

## D. PROVEN BY BOTH — the rigorous subset (Joel in-game + me)

1. ✅✅ chat→spell/admin loop end-to-end
2. ✅✅ `broadcastChat` (Aetherius speaks in Global)
3. ✅✅ `setTimeOfDay` (±2h-step recipe)
4. ✅✅ consent gating (enqueue → owner approves)
5. ✅✅ owner-override (`#pilot pause/resume`)

Everything 🔧 is **coded + deployed but not individually live-proven by both of us yet.**

---

## E. What I CAN'T determine (needs a live-test pass or Joel's recall)

The honest gap. I confirmed every 🔧 feature **exists and is deployed** (lobe-audited the source,
verified `turdmod-service.exe` is the OVH binary). I **cannot** confirm from code alone whether each
is **live-working + bug-free in-game** — that's the "PROVEN BY BOTH" bar, which needs either:
- a structured **live-test pass** (fire each `!`/`#` command on OVH with Joel watching), or
- **Joel's recall** of which he's already tested in play.

**Specific unknowns to resolve:**
1. Per-module live status of the ~50 `turdmod-service` features (which are battle-tested vs merely shipped).
2. BattleMetrics / NightKingdoms sync — not found in the audit; confirm not-built vs missed.
3. Overlay HUD panels + manager dashboard — deployed, but live-functional state unconfirmed.
4. `#WUMBO` item-scaling — the one concrete missing command (small `setActorScale3D` verb).

---

## F. The `#WUMBO` note

`#WUMBO <0..50>` is a WhalleyBot **custom** command (NOT in SCUM's 217 native admin commands — see
`SCUM-ADMIN-COMMANDS.md`). Scales the held item via `SetActorScale3D`. We don't have it, but it's a
trivial bridge add (`writeActorProperty` is scalar-only, `callActorFunction` is no-arg-only; a small
`setActorScale3D` verb modeled on `teleportPlayer`'s param-struct closes it). ~one build cycle.

_Built 2026-06-07. Sources: https://scum.wiki.gg/wiki/WhalleyBot · SCUM client reflection v23451409 ·
lobe audit of turdmod-service · CAPABILITY-MAP · live session results._
