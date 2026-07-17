# Hooks Needed — engine bridge handlers + service lifecycle hooks

Living tracker of every hook the TurdMOD systems depend on. As a feature needs a hook we add
it here, mark status, and "get it" (verify it exists / build it). @related: [CAPABILITY-MAP.md](CAPABILITY-MAP.md),
[reference_live_re_toolkit]. Status: ✅ proven live · 🟡 exists, unverified · 🔨 must build · ❌ unusable.

## Bridge RPC handlers (engine — called over the named pipe)

| Handler | Status | Used by | Notes |
|---|---|---|---|
| `ping` | ✅ | health | pong smoke test |
| `findInstancesByClass` `{class,exact?,limit?}` | ✅ | vehicle id, durability | safe GUObjectArray scan, no ProcessEvent. **Replaces `getNearbyActors`.** |
| `readActorByPtr` `{ptr,expandArrays?,includeInherited?}` | ✅ | read props/ownership | SEH-guarded; recurse object graph by ptr |
| `writeActorProperty` `{ptr,propertyName,value,valueKind}` | ✅ | durability (instances) | proven 2026-06-07: set `_linearEnergyAbsorption` 0.2→0.8 on 31 Rager parts |
| `writeClassDefault` | 🟡 | durability **boot hook** | CDO write → new/reconstructed parts spawn tough. UNVERIFIED — validate before relying |
| `readMemory` / `patchInstructions` / `unpatchInstructions` | ✅ | low-level RE | proven |
| mounted-vehicle → entity_id read | 🔨 | lock-gated register | need a SAFE way to get the vehicle a player is in + its entity id (current code uses crash-prone `getNearbyActors` + a raw +7464 offset). Prefer findInstancesByClass + `_repServerEntitySetupAndId` decode |
| vehicle native owner + lock lookup | ✅ | lock-gated register | **FOUND 2026-06-07** (corrects earlier "disproven"). Signal is in `item_entity.xml` on the vehicle's item-container, NOT `entity.owning_entity_id`. Chain: `vehicle_entity(entity_id) → item_container_entity_id → item_entity.xml`. XML carries `_owningUserProfileId="<profile>"` (→ `user_profile.user_id` = SteamID) **and** `<Locks><LockSlot _assetId="Item:Lock_Item_*"/></Locks>`. **The lock placement sets ownership — the duo is native.** Persistent in SCUM.db. Gate `!register`: require a `<LockSlot>` AND `_owningUserProfileId == player`. |

## Service lifecycle hooks (turdmod-service)

| Hook | Status | Purpose |
|---|---|---|
| `on_boot` (after bridge ready) | 🔨 | **durability re-apply** — `writeClassDefault` the Rager part classes (`_linearEnergyAbsorption=0.8`) so durability survives restarts. Also re-assert any other CDO tunables |
| `pre_restart` / maintenance | 🔨 | **"Car Repo Time"** — recompute per-type `<Type>MaxAmount` in ServerSettings.ini from registrations; delete TTL-expired temp vehicles (server-off, per [feedback_scumdb_edit_safety]) |
| registration expiry sweep | 🔨 | promote/cull temp slots; drive `!garage` countdowns |

## Acquisition order (current build: vehicle economy Part 1)
1. **Verify** locking sets `owning_entity_id` (live DB) — gates the whole lock-gate design.
2. **Validate** `writeClassDefault` on one Rager part class — gates the durability boot hook.
3. Build mounted-vehicle entity-id read (safe path) → native owner lookup → lock-gated `!register`.
4. Build `on_boot` durability hook.

## Follow-up: stamp real SteamID onto chat events
Chat events from the bridge carry `player` (name) but often NOT `steam` (the SteamID isn't
a BattlEye artifact — it lives in the engine's ConZPlayerController/user-profile, readable by
the in-process bridge with no BattlEye + no detection). WORKAROUND (shipped): resolve profile by
name via `scumdb::profile_id_for_name` (name → user_profile → user_id = SteamID). PROPER FIX:
bridge reads the player's SteamID off the engine player object and attaches it to every chat
event, so all steam-dependent commands are solid. 🔨
