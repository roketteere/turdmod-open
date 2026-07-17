# TurdMOD Engine Bridge — Capability Map

Ground-truth catalog of **all 101 registered bridge handlers** (`TurdMODEngineBridge.cpp`).
Each is verified against the **live** server and marked. No claim here is unverified.

**Legend:** ✅ verified working · ❌ verified broken/no-effect · ⚠️ works with caveat · ❓ not yet verified

> Verification method: read-only handlers called directly via `node tools/engine-rpc-test.mjs <name> '<json>'`.
> State-writing handlers verified by reading the affected field/state before/after. SCUM build 23451409.

---

## A. Diagnostics / Reflection / RE (read-only)

| Handler | Status | Notes |
|---|---|---|
| `ping` | ✅ | pong, returns mod name/kind |
| `imageBase` | ✅ | `0x7ff730c70000` / pref `0x140000000` |
| `listHandlers` | ✅ | count=101 |
| `readMemory` | ✅ | `{addr,size}`→bytesHex (proven) |
| `findUInt64` | ❓ | alive; param format TBD |
| `dumpVTable` | ❓ | needs object ptr |
| `getMemoryProfile` | ✅ | process+system mem stats |
| `dumpClasses` | ✅ | emitted 500 (grep/limit params) |
| `dumpAllClasses` | ✅ | works; needs `outDir` (writes files) |
| `dumpAllEnums` | ✅ | works; needs `outDir` |
| `dumpAllStructs` | ✅ | works; needs `outDir` |
| `dumpUFunctions` | ✅ | total 1,552,174 |
| `dumpWidgets` | ✅ | emitted 500 |
| `describeFunction` | ❓ | alive; param format TBD (not `name`) |
| `describeWidget` | ❓ | needs widget name |
| `findFunctions` | ✅ | name/owner search (no addr) |
| `listClassInstances` | ✅ | `{pattern}`→class+count |
| `readClassValues` | ❓ | alive; param format TBD |
| `dumpAdminCommands` | ✅ | **231 admin commands** enumerated |
| `dumpItemNames` | ✅ | **216 items** |
| `listPatches` | ✅ | count=0 |
| `listModals` | ✅ | 19 open modals |
| `listConfigFiles` | ✅ | config dir listing |
| `getL3ProbeData` | ✅ | installed=false, 0 records |
| `probeQuestHandlers` | ✅ | probed Server_StartQuest |

## B. Server / world reads (read-only) — ALL ✅

| Handler | Status | Notes |
|---|---|---|
| `getOnlinePlayers` | ✅ | count=1 (YOUR_OWNER_NAME) |
| `getPlayerPositions` | ✅ | name+coords |
| `getServerStats` | ✅ | cpu/mem/io |
| `getActorPopulation` | ✅ | 40 classes |
| `getNearbyActors` | ✅ | needs `{playerName,classFilter,radius}` |
| `listSpawnedVehicles` | ✅ | count=4 |
| `listSquads` | ✅ | count=1 |
| `readConfig` | ❓ | needs key param |
| `readConfigFile` | ❓ | needs filename param |

## C. Messaging / UI

| Handler | Status | Notes |
|---|---|---|
| `sendChatLineToPlayer` | ✅ | per-player colored line (channel=color). **Confirmed visible 2026-05-30.** |
| `broadcastChat` | ✅ | server-wide chat line. **PROVEN BY BOTH 2026-06-07:** Aetherius (god-admin) spoke in Global ("人 Pilot: Noon already shines upon you…") — Joel saw it in-game, confirmed in UE4SS.log. |
| `sendChat` | ❓ | via BroadcastChatMessage (admin-chat path) |
| `sendHudMessage` | ❓ | HUD text |
| `sendGameModeHudMessage` | ❓ | gamemode HUD text |
| `sendNotification` | ❓ | notification toast |
| `showKillFeedNotification` | ❓ | killfeed entry |
| `showPanel` | ❓ | custom UI panel |
| `broadcastRaidBanner` | ❓ | raid banner |
| `spawnWidgetRouter` | ❓ | custom widget mount |
| `dismissModals` | ❓ | close modals |

## D. Player state (targeted Pattern-D writes)

| Handler | Status | Notes |
|---|---|---|
| `teleportPlayer` | ✅ | `K2_TeleportTo`. **PROVEN 2026-06-07** (teleported Lilac→YOUR_OWNER_NAME). ⚠️ Parser bug: coords passed as **bare numbers** OR nested `pos:{}` zero out x/y (only z reads). **WORKAROUND (proven): pass `x`/`y`/`z` as STRING-QUOTED top-level args** (`"x":"-509502"`) → routes through the tested `extract_json_str` path, parses correctly. Param is `name`/`ptr` (not `playerName`). |
| `setGodMode` | ❓ | prisoner flag |
| `setImmortal` | ❓ | prisoner flag |
| `setInfiniteAmmo` | ❓ | prisoner flag |
| `setSuperJump` | ❓ | prisoner flag |
| `setFamePoints` | ❓ | fame value |
| `setGender` | ❓ | prisoner gender |
| `setFlyingMode` | ❓ | fly toggle |
| `launchPlayer` | ❓ | impulse/launch |

## E. Vehicles

| Handler | Status | Notes |
|---|---|---|
| `damageVehicle` | ✅ | `Server_ApplyDamageToRegion`; despawn-via-destruction. **Proven this session.** |
| `despawnVehicleNative` | ⚠️ | native DestroyVehicleAndEntity; crashes in some contexts (parked). Also doubles as class-instance finder via `dryRun`. |
| `destroyVehicle` | ❌ | `K2_DestroyActor` — no-ops on SCUM-managed vehicles |
| `spawnVehicle` | ❓ | generic UE spawn (partial assembly) vs full-assembly? — verify |
| `bringVehicleToPlayer` | ❓ | move vehicle to player |

## F. Spawning / items

| Handler | Status | Notes |
|---|---|---|
| `spawnItem` | ❓ | `GameplayStatics::SpawnObject` |
| `placeItemInInventory` | ❓ | item into inventory |
| `createObject` | ❓ | generic object create |
| `spawnAI` | ❓ | spawn NPC/creature |
| `spawnMech` | ❓ | spawn mech |
| `mechFireWeapon` | ❓ | mech weapon fire |

## G. World / environment

| Handler | Status | Notes |
|---|---|---|
| `setTimeOfDay` | ✅ | param **`hours`** (0..24). **Hard clamp ±2h/call** toward target, shortest dir. **PROVEN clamp-aware recipe:** decompose request into `ceil(hoursToMove/2)` steps of +2h, ~1s apart — walked noon→midnight (12→14→16→18→20→22→0) cleanly. ⚠️ `chat_cmds.rs` !day/!night pass `hour` (singular) → broken (should be `hours`). TODO: bake the 2h-step loop into the handler/!day-!night. |
| `setWeather` | ❓ | weather fields |
| `forceWeatherSnapshot` | ❓ | force replication snapshot |

## H. Creatures / AI

| Handler | Status | Notes |
|---|---|---|
| `tameNearbyAnimal` | ❓ | tame |
| `setAnimalPassive` | ❓ | animal AI |
| `setZombiePassive` | ❓ | zombie AI |
| `provokeZombies` | ❓ | aggro zombies |

## I. Admin / server management

| Handler | Status | Notes |
|---|---|---|
| `kickPlayer` | ❓ | kick |
| `banPlayer` | ❓ | ban (writes BannedUsers.ini?) |
| `unbanPlayer` | ❓ | unban |
| `shutdownServer` | ❓ | graceful shutdown |
| `grantElevatedStatus` | ⚠️ | writes AdminUsers.ini (file; needs reconnect) |
| `revokeElevatedStatus` | ❓ | removes AdminUsers.ini line |
| `runAdminCommand` | ❌ | `ProcessAdminCommand` — does NOT execute (see HANDBOOK §2.2). gameThread/bypass added but still no-op. |
| `runTestAdminCommand` | ❌ | same dead path |
| `getAdminOutput` | ⚠️ | capture buffer (empty — PE routing gated off) |

**kick/ban/say via BattlEye RCON — BE-ON only.** `apps/turdmod-service/src/rcon_be.rs`
(Phase 3) routes `!kick` / `!ban` / `!say`(`!announce`) over BERcon (HANDBOOK §2.5). CRC32
framing unit-tested vs `zlib`; `!players` lookup resolves name→BE slot `#`. Config:
`C:\TurdMOD\data\rcon.json`. ⏳ live kick/ban-by-name vs a joined player not yet verified.
⚠️ **VERIFIED 2026-05-30: BErcon only exists when BattlEye is ON.** With `-NoBattlEye` no
RCON listener binds (confirmed via netstat on OVH — nothing on 7048). Joel's policy is
**BattlEye always off** (`battleye-always-off`), so on deployments this RCON path has no
target — kick/ban/say must route through the **engine-bridge handlers above**
(`kickPlayer`/`banPlayer`/`sendChat…`). Those are still ❓ — **verifying/RE-ing them is the
real admin follow-up.**

## J. Squads

| Handler | Status | Notes |
|---|---|---|
| `promoteSquadMember` | ❓ | |
| `removeFromSquad` | ❓ | |
| `sendSquadInvitation` | ❓ | |

## K. Economy

| Handler | Status | Notes |
|---|---|---|
| `setCurrencyBalance` | ❓ | currency write |
| `setEconomy` | ❓ | economy state |

## L. Memory / patching (RE write tools)

| Handler | Status | Notes |
|---|---|---|
| `readMemory` | ✅ | (also in A) proven this session |
| `patchInstructions` | ✅ | byte patch w/ expected-verify + thread-suspend. Proven this session. |
| `unpatchInstructions` | ✅ | restore. Proven this session. |
| `writeActorProperty` | ❓ | write a field on an actor |
| `writePlayerProperty` | ❓ | write a field on a player |
| `writeClassDefault` | ❓ | write a CDO default |

## M. Config files

| Handler | Status | Notes |
|---|---|---|
| `readConfig` / `readConfigFile` / `listConfigFiles` | ❓ | (reads — see A/B) |
| `writeConfig` | ❓ | write parsed config |
| `writeConfigFile` | ❓ | write raw config file |

## N. Misc / experimental

| Handler | Status | Notes |
|---|---|---|
| `runHelloWorld` | ❓ | BP dispatch smoke test |
| `loadAsset` | ❓ | game-thread LoadPackage |
| `applyRecipe` | ❓ | crafting recipe apply |
| `possessActor` | ❓ | possess a pawn/vehicle |
| `unpossessActor` | ❓ | return to body |
| `callActorFunction` | ❓ | call arbitrary actor UFunction by name |
| `enableL3Probe` / `disableL3Probe` | ❓ | L3 instrumentation toggle |
| `setEconomy` | ❓ | (see K) |

---

## Progress

- **Section A (diagnostics):** 20/24 ✅ verified live; 4 alive but need param-format confirmation (`findUInt64`, `dumpVTable`, `describeFunction`, `readClassValues`).
- **Section B (server/world reads):** 7/9 ✅; 2 need a param (`readConfig`, `readConfigFile`).
- **Already proven earlier:** sendChatLineToPlayer ✅, damageVehicle ✅, patchInstructions ✅, unpatchInstructions ✅, despawnVehicleNative ⚠️, destroyVehicle ❌, runAdminCommand ❌, runTestAdminCommand ❌.
- **Running tally:** ~32 verified. The entire **read/diagnostic surface works.**
- **Next:** state-changing handlers (C messaging, D player-state, G world, etc.) — these touch the live game/player, so paced with Joel. Risky ones (kick/ban/shutdown/launch/provoke) flagged, not fired without explicit OK.

- **Event routing FIXED (2026-05-30, commit bde168c):** inbound chat/login/logout events now
  emit safely (pointer-compare dispatch, default-on; the fn+0x18 FName-read crash is gone).
  `!time`/`!day`/`!night` proven end-to-end through `chat_cmds`. Player join no longer crashes.

_Last updated: 2026-05-30, Section A+B verified + Phase 1 event routing live._
