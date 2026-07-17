# Engine-tier TODO — RE + native-command work (post 90/90)

Created 2026-06-10 after the full mod-surface migration (all 90 mods on the `registry::Mod` trait).
These items can't be done with ServerSettings/config alone — they need engine-tier RE on the live
local server (bridge: `findInstancesByClass` + `writeActorProperty` + `writeClassDefault`, the same
technique as `vehicle_durability`'s `_linearEnergyAbsorption`) or native `#` admin commands. Batch
the RE items in one session against the LOCAL engine (bring it up: `turdmod-service --console` +
POST `/server/start`).

## 1. Zombie damage-to-clothes −40%  [engine-tier]
- ServerSettings exposes ONLY `scum.ZombieDamageMultiplier` (already set to 0.70 = −30% to players).
  There is NO clothes/armor-damage key.
- Approach: find the puppet melee/attack class (or the clothing damage-application path) via
  `findInstancesByClass` on loaded puppets, locate the clothing-damage property, `writeClassDefault`
  it ×0.60. Probe property names on a loaded `BP_Prisoner`/puppet class.

## 2. Zombie bleed / infect / status-effect chances −20%  [engine-tier]
- No ServerSettings key (confirmed — only ZombieDamageMultiplier + SurvivalSkillMultiplier exist).
- Approach: the wound/infection roll lives on the puppet attack or the status-effect application
  class. Find the chance/probability property, ×0.80 via `writeClassDefault`.

## 3. Mech / NPC passive toggle  [needs bridge handler]
- `!zombies` / `!animals` passive on/off + banner is DONE/live (`passive_control` mod, uses
  `setZombiePassive` / `setAnimalPassive`).
- Mechs (Sentry2) + human NPCs have NO `setXxxPassive` bridge handler yet. Need a new bridge handler
  (set the AI aggression/target property on Sentry2 + NPC controllers), then extend `passive_control`
  with `!mech` / `!npcs` (the mod is structured so adding a type is a few lines).

## 4. Clothing / armor repair at ANY condition  [engine-tier]  (Joel 2026-06-10)
- Goal: with the proper repair kit, repair clothing AND armor regardless of how damaged it is — remove
  the condition floor that blocks repairing badly-damaged items.
- NOT a ServerSettings key (only decay-RATE keys exist: ItemDecayDamageMultiplier etc.).
- Approach: the repair logic enforces a min-condition / max-condition-after-repair on the clothing/
  armor item class. Find that property (e.g. a `MinConditionToRepair` / `MaxRepairableCondition` on
  the ClothingItem/ArmorItem class) and `writeClassDefault` it to 0 / 1.0. Probe a loaded clothing
  item class for the repair-threshold property.

## 5. Native `#` quest + world-event integration  [native commands — FOUND]
- The 231-verb catalog (`docs/engine/admin-commands-catalog.json`) HAS native quest/event verbs:
  - `Quests` (class `Quests_C`) — "Control quests system." with subcommands (`QuestsSubCommand_C`,
    `QuestSetup_C`, `Quest Asset`).
  - `ExportQuests` — exports default quest configuration.
  - `ScheduleWorldEvent` (class `ScheduleWorldEvent_C`) — "Schedules a world event of the specified
    type at the specified location" (param: `World Event` / `WorldEvent_C`).
- Our `quests`, `event_scheduler`, `objective_events` mods reinvent quests/events in JSON. They SHOULD
  delegate to the real systems via `runAdminCommand` (`#Quests …`, `#ScheduleWorldEvent <type> <loc>`)
  so players get the native quest UI + real world events instead of chat-only sims.
- Next: dump the exact `#Quests` subcommand grammar + the `ScheduleWorldEvent` world-event type list
  (run `#Quests` / inspect `QuestsSubCommand_C` via the bridge), then refactor the 3 mods to drive them.

---
Already done/live (not TODO): 90/90 trait migration; zombie dmg −30%; `!zombies`/`!animals` passive
toggle + "now your friends!" banner; banner capture-template placeholder text replaced with branded copy.
