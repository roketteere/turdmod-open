# Bridge handler candidates — SCUM UFunction shortlist

> Surveyed 2026-05-18 from scumdump v23128915 reflection dump
> (14,507 classes). Goal: identify SCUM UFunctions worth wrapping
> as bridge handlers, ranked by expected mod value.

## How to read this

- **NetMulticast_*** runs on the server and replicates to all
  clients. **Highest-confidence callable** from our PE bypass —
  no client-auth check in the way.
- **Server_*** is the channel a client uses to ask the server to
  do something. The server *can* call these locally, but per
  `feedback_scum_admin_auth_blocker.md` they sometimes silently
  reject when the network metadata path is bypassed. Try, but
  treat as medium-confidence.
- **Set\*** / **Spawn\*** / **Add\*** etc. on world objects (no
  Server_/NetMulticast_ prefix) are usually plain UFunctions
  that work via ProcessEvent.

## Tier S — Banner/announcement broadcast (highest value, lowest risk)

The Notifications.json system already gives us banner overlays
but only at scheduled times. These UFunctions let us fire banners
**on demand** from a handler. Same widget path the game already
trusts.

### `GlobalRaidProtectionManager` (5 NetMulticast_*)
- `NetMulticast_ShowRaidStartAnnouncementMessage`
- `NetMulticast_ShowRaidEndAnnouncementMessage`
- `NetMulticast_ShowRaidConcludedMessage`
- `NetMulticast_ShowRaidAllowedMessage`
- `NetMulticast_ShowRaidTimesMessages`

**Mod use cases:**
- "Event starting in 5 minutes" banner before a community event
- Server-wide broadcast for staff actions
- Tournament countdown system

## Tier S — Live economy adjustment

### `ConZEconomyManager` (4 NetMulticast_*)
- `NetMulticast_UpdateGoldPriceMasterMultiplier`
- `NetMulticast_UpdateDateVsGoldPriceMasterMultiplierMap`
- `NetMulticast_UpdateTradeablePriceMultiplierFactor`
- `NetMulticast_UpdateTradeableClassMapHelperOverrides`

**Mod use cases:**
- "Weekend sale" — push 0.5× trader prices for 48 hours
- "Gold rush event" — temporary gold price spike
- Per-event item-class discount (e.g. medical sale during PvP week)

### `BankAccountRegistry`
- `ResetDailyTransactionLimitsOnAllAccounts`

**Use case:** unstick players whose bank limits got stuck.

## Tier A — Admin actions on players

### `ConZPlayerController` (29 callable)
- `Teleport` (already wrapped)
- `SetFamePoints` — direct fame admin
- `SetCurrencyBalanceRep` — direct currency admin
- `SetGameEventCooldown` — force-trigger event timers
- `Server_SelfKickFromGameSession` — soft-kick a player
- `Server_RequestSurvivalStats` — pull live player stats

**Mod use cases:**
- Admin Panel "Adjust fame/currency" buttons (already in the works
  for Admin GUI)
- Tournament point management

### `Prisoner` (104 callable; 23 NetMulticast_*)
Top picks beyond the obvious:
- `Teleport` (works)
- `SetTargetOnServer`
- `SetMeleeTarget` / `SetMeleeTargetSelectionMode`
- `SetRotationTarget`
- `SetNightVisionEnabled`
- `NetMulticast_UpdateAdminStates`
- `NetMulticast_TurnPrisonerInPlace`

**Mod use cases:**
- "Possess this player's camera" admin debug
- Force-face-direction for cinematic/event setups

### `InventoryUserComponent` (18 callable)
- `Server_InventoryComponent_AddOrMoveEntry`
- `Server_InventoryComponent_RemoveEntry`
- `Server_CharacterInventoryComponent_SetItemInHands`
- `Server_CharacterInventoryComponent_UnequipClothes`
- `Server_OpenInventory` / `Server_CloseInventory`

**Mod use cases:**
- Admin remote-inspect inventory (read state via Server_StartSendingInventoryState)
- "Give item to player" via existing flow (vs spawnItem which bypasses inventory rules)
- Force-equip an event uniform

## Tier A — Base / fortification staging

### `ConZBaseManager` (14 NetMulticast_*)
- `NetMulticast_SpawnBaseElement`
- `NetMulticast_TransferOwnership`
- `NetMulticast_SetBaseHasActiveEncounter`
- `NetMulticast_SetBaseOwnerPlayerId`
- `NetMulticast_UpdateItemElementsLocationsAndRotations`

**Mod use cases:**
- Admin "transfer abandoned base ownership" tool
- Custom encounter overlay (mark a base as "under attack")

### `FortificationManager` (5 NetMulticast_*)
- `NetMulticast_AddFortification` / `AddFortifications`
- `NetMulticast_DestroyFortification`
- `NetMulticast_RemoveAllFortifications`
- `NetMulticast_UpdateFortification`

**Mod use cases:**
- Event "free wall day" — temporarily mass-add fortifications
- Admin "clear griefed walls" cleanup

## Tier B — Encounter staging (event mode)

### `Sentry2` (24 NetMulticast_*)
Trigger sentry behaviors on demand: attack effects, walking-away
sounds, melee montages, target-list manipulation. Useful for
scripted set pieces where admin wants the sentry to do something
specific.

### `Dropship` (9 NetMulticast_*)
- `NetMulticast_PlayDropshipMontage`
- `NetMulticast_PlayRailgunFiredEffects`
- `NetMulticast_Explode`
- `NetMulticast_OnDropshipDeath`

**Mod use case:** scripted dropship event for community wipe night.

### `KillboxComponent` (8 NetMulticast_* + 11 setters)
- `SetPanicbutton` / `SetMusicComponent`
- `NetMulticast_StopMusic`
- `NetMulticast_ReportKillActivation`

**Mod use case:** custom killbox encounters w/ host quotes.

## Tier B — Quests (admin debug)

### `PlayerQuestComponent` (24 callable)
- `Server_StartQuest` / `Server_AbandonQuest`
- `Server_StartTask` / `Server_AbandonTask`
- `Server_SetTrackedQuest`
- `Server_UpdateTrackingData`
- `Client_UpdateAvailableQuestInfo` / `Client_UpdateTrackingData`

**Confidence:** Medium — `Server_*` may hit the auth blocker.
Worth a probe handler before building UI.

**Mod use cases:**
- Admin "force-start quest" to unstick broken player state
- Admin "abandon ghost quest" to clean up bugged trackers

## Tier B — Live server settings

### `PlayerRpcChannel` (37 callable, including)
- `Server_ServerSettingsSendToServer`
- `Server_ServerSettingsLock_RequestLockAcquisition`
- `Server_ServerSettingsLock_RequestLockRelease`

**Conjecture:** this is the path the in-game admin panel uses to
push live settings without restart. If we can call it server-side,
we can change ServerSettings.ini values **without bouncing the
process**. Big quality-of-life win.

**Confidence:** Low (auth-gated) but high-payoff.

## Tier C — Mood-setting (weather, encounters)

### `WeatherController2`
- `NetMulticast_SendStateSnapshot`
- `NetMulticast_ResetStateSnapshots`
- `NetMulticast_OnRep_NighttimeDarkness`

**Mod use case:** "fog of war" event — push spooky weather
state on demand.

## Recommended build order

1. **`broadcastRaidBanner` handler** — wraps the five
   `GlobalRaidProtectionManager::NetMulticast_*` UFunctions.
   Net-new banner overlay that bypasses Notifications.json
   scheduling. Highest payoff, lowest risk.

2. **`setEconomy` handler family** — `ConZEconomyManager::*`
   multipliers. Live economy levers from Admin UI.

3. **`setFamePoints` / `setCurrencyBalance` handlers** —
   `ConZPlayerController` setters. Already-requested admin UI
   needs these.

4. **`giveInventoryItem` handler** —
   `InventoryUserComponent::Server_AddOrMoveEntry`. Inventory-aware
   item giving (vs spawnItem which bypasses inventory rules).

5. **`probeQuestHandlers`** — one-shot smoke test against
   `PlayerQuestComponent::Server_StartQuest` to confirm or deny
   the auth-blocker hypothesis on Server_* calls. Result decides
   whether the rest of Tier B is reachable.

6. **`pushServerSettings` (research)** —
   `PlayerRpcChannel::Server_ServerSettingsSendToServer` probe.
   If callable, this is a marquee feature ("change any setting,
   live, no restart").

## Notes

- All `cpp` kind only. Blueprint UFunctions (`*_C`) often work
  but are harder to keep stable across SCUM patches; prefer cpp
  when both exist.
- Hook one at a time per `feedback_bridge_rpc_one_at_a_time.md`
  — these are heavy reflection ops.
- The auth-blocker pattern from
  `feedback_scum_admin_auth_blocker.md` is the main thing to
  watch for; smoke-test each new handler against a live SCUM
  server before adding UI.
