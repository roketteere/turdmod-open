# Admin-Command Coverage Map — 217 native commands vs bridge handlers

No-player parity tracker. There is **no generic `#`-runner** (proven dead end — HANDBOOK 2.2/2.6: `ProcessEvent` aborts off game-thread + needs a sender PC; SCUM RCON is BattlEye-only). The ONLY path is the **bypass model** — a typed bridge handler per command calling the engine function directly.

Legend: ✅ already mimicked by a bridge handler · 🟡 easy direct-call to add (generic property/config/spawn primitive) · 🔴 needs real RE (complex/stateful or no obvious engine hook).

> Buckets are heuristic (by name/prefix) — 🟡 means *a plausible direct path exists*, not that it's verified. Validate per command against the live engine before building.
---

## Summary

- ✅ **Already mimicked:** 51 / 217
- 🟡 **Easy direct-call to add:** 116
- 🔴 **Needs RE:** 50

## ✅ Already mimicked (51)

| `#Announce` -> `broadcastChat` | `#BanPlayer` -> `banPlayer` |
| `#ChangeCurrencyBalance` -> `setCurrencyBalance` | `#ChangeCurrencyBalanceToAll` -> `setCurrencyBalance` |
| `#ChangeCurrencyBalanceToAllOnline` -> `setCurrencyBalance` | `#ChangeFamePoints` -> `setFamePoints` |
| `#DestroyVehicle` -> `damageVehicle` | `#DumpAllSquadsInfoList` -> `listSquads` |
| `#GetMeshInfo` -> `getNearbyActors` | `#GetUserID` -> `getOnlinePlayers` |
| `#GetUserIDByRank` -> `getOnlinePlayers` | `#GrantElevatedStatus` -> `grantElevatedStatus` |
| `#KickPlayer` -> `kickPlayer` | `#ListPlayers` -> `getOnlinePlayers` |
| `#ListSpawnedAnimals` -> `getNearbyActors` | `#ListSpawnedArmedNPCs` -> `getNearbyActors` |
| `#ListSpawnedVehicles` -> `listSpawnedVehicles` | `#ListSquads` -> `listSquads` |
| `#Location` -> `getPlayerPositions` | `#MapTeleport` -> `teleportPlayer` |
| `#PlayerInfo` -> `getOnlinePlayers` | `#RevokeElevatedStatus` -> `revokeElevatedStatus` |
| `#SendNotification` -> `sendNotification` | `#SetCurrencyBalance` -> `setCurrencyBalance` |
| `#SetCurrencyBalanceToAll` -> `setCurrencyBalance` | `#SetCurrencyBalanceToAllOnline` -> `setCurrencyBalance` |
| `#SetFamePoints` -> `setFamePoints` | `#SetFamePointsToAll` -> `setFamePoints` |
| `#SetFamePointsToAllOnline` -> `setFamePoints` | `#SetGender` -> `setGender` |
| `#SetGodMode` -> `setGodMode` | `#SetInfiniteAmmo` -> `setInfiniteAmmo` |
| `#SetPrisonerImmortality` -> `setImmortal` | `#SetSuperJump` -> `setSuperJump` |
| `#SetTime` -> `setTimeOfDay` | `#SetWeather` -> `setWeather` |
| `#ShutdownServer` -> `shutdownServer` | `#SpawnAnimal` -> `spawnAI` |
| `#SpawnArmedNPC` -> `spawnAI` | `#SpawnItem` -> `spawnItem` |
| `#SpawnRandomAnimal` -> `spawnAI` | `#SpawnRandomZombie` -> `spawnAI` |
| `#SpawnVehicle` -> `spawnVehicle` | `#SpawnZombie` -> `spawnAI` |
| `#SquadInfo` -> `listSquads` | `#Teleport` -> `teleportPlayer` |
| `#TeleportTo` -> `teleportPlayer` | `#TeleportTo3pm` -> `teleportPlayer` |
| `#TeleportToMe` -> `teleportPlayer` | `#TeleportToVehicle` -> `teleportPlayer` |
| `#UnbanPlayer` -> `unbanPlayer` |

## 🟡 Easy direct-call to add (116) — writeActorProperty / writePlayerProperty / writeConfig / spawn primitives

| `#AddGardenPlantPest` | `#AddOrRemoveWidget` | `#AddPrisonerBodyEffect` | `#BoatDebug` |
| `#DebugProjectileCollisions` | `#DebugWeapon` | `#DemolitionSkillDebug` | `#DestroyAllBaseBuildingElementsForFlag` |
| `#DestroyAllBaseBuildingElementsForPlayer` | `#DestroyAllBaseBuildingElementsForSquad` | `#DestroyAllBaseBuildingElementsWithinRadius` | `#DestroyAllFlagsForPlayer` |
| `#DestroyAllItemsWithinRadius` | `#DestroyAllRazorsWithinRadius` | `#DestroyAllVehicles` | `#DestroyArmedNPCsWithinRadius` |
| `#DestroyCorpsesWithinRadius` | `#DestroyEncountersAtPlayerLocation` | `#DestroyEntity` | `#DestroyFlag` |
| `#DestroyZombiesWithinRadius` | `#DisablePrisonerBodyEffects` | `#DistanceDebug` | `#DoorDebug` |
| `#DrawDebugZombieCapsulesOnLegacySpawnPoints` | `#DrawNearbyEncounters` | `#DrawSentryHealthBar` | `#DumpWetnessDebug` |
| `#EnableAdminViolations` | `#EnableGameplayMetadataLogging` | `#EnableHuntingClueDebugArrow` | `#EnableOrDisableServer` |
| `#ListActiveAbandonedBunkers` | `#ListActiveHunts` | `#ListActiveSecretBunkers` | `#ListFeatureFlags` |
| `#ListFlags` | `#ListItemsSpawnLocations` | `#ListMutedPlayers` | `#ListPrimaryAssets` |
| `#ListPrisonerBodyConditionInteractions` | `#ListPrisonerBodyEffects` | `#ListPrisonerForeignSubstances` | `#ListSilencedPlayers` |
| `#ListSquadMembers` | `#ListWeatherControllerOverrides` | `#PlacementDebug` | `#PrintEntities` |
| `#PrintGlobalRaidProtectionRaidTimes` | `#RemovePrisonerBodyEffect` | `#ResetAchievements` | `#ResetAllHuntCooldownsForPlayer` |
| `#ResetEconomy` | `#ResetPlayerBalances` | `#ResetSquadInfo` | `#SetAIInvisibility` |
| `#SetAchievementUnlocked` | `#SetAirplaneMaxVelocity` | `#SetAllInventoryAccess` | `#SetBodyType` |
| `#SetCraftingSearch` | `#SetDecayTimeDilation` | `#SetDeluxeVersion` | `#SetFakeName` |
| `#SetFarmingSimulationSpeed` | `#SetFeatureFlag` | `#SetGardenNutrientsHigh` | `#SetGardenPlantGrowthStage` |
| `#SetGardenPlantingTime` | `#SetHealthToItemInHands` | `#SetItemDebugMode` | `#SetMalfunctionProbability` |
| `#SetMountedVehicleProperty` | `#SetPrisonerAttributes` | `#SetPrisonerBladderVolume` | `#SetPrisonerExhaustion` |
| `#SetPrisonerInfiniteOxygen` | `#SetPrisonerInfiniteStamina` | `#SetPrisonerMetabolismSimulationSpeed` | `#SetPrisonerStamina` |
| `#SetPrisonerStomachVolume` | `#SetReplishableResourceAmount` | `#SetShouldPrintExamineSpawnerPresets` | `#SetSkillLevel` |
| `#SetTimeSpeed` | `#SetWeatherControllerOverrideActive` | `#SetWeatherControllerOverrideValue` | `#ShowBaseBuildingDebug` |
| `#ShowFlagInfo` | `#ShowFlagLocations` | `#ShowNameplates` | `#ShowRespawnTimes` |
| `#ShowVehicleDebug` | `#ShowVehicleInfo` | `#ShowVehicleLocations` | `#ShowWeaponInfo` |
| `#SpawnAllItems` | `#SpawnBrenner` | `#SpawnDebugAnimalTrack` | `#SpawnInventoryFullOf` |
| `#SpawnPrimaryActorAsset` | `#SpawnRandomPrimaryActorAsset` | `#SpawnRazor` | `#SpawnReflectionSphere` |
| `#ToggleAmbientSound` | `#ToggleFamePointsDebugVisualization` | `#ToggleFog` | `#ToggleZombieNavigationLogging` |
| `#TrapsDebug` | `#VisualizeAnimalLocation` | `#VisualizeArmedNPCLocation` | `#VisualizeBulletTrajectories` |
| `#VisualizePath` | `#VisualizePlayerAiming` | `#VisualizeVehicleTrajectory` | `#VisualizeZombieLocation` |

## 🔴 Needs real RE (50) — complex/stateful or no obvious hook

| `#AdminLight` | `#ArmorAbsorptionOutput` | `#CancelVote` | `#CheckServerTime` |
| `#ClearEncounterCooldowns` | `#ClearFakeName` | `#CookRecipe` | `#CrashMajestically` |
| `#CreateEntity` | `#DumpEncounterManagerData` | `#EndTournamentMode` | `#EnhancedPhotoMode` |
| `#EquipParachute` | `#ExecuteConsoleCommand` | `#ExecutePrisonerBodyConditionInteraction` | `#ExportDefaultItemSpawnerPresets` |
| `#ExportDefaultItemSpawningCooldownGroups` | `#ExportDefaultItemSpawningParameters` | `#ExportItemLootTree` | `#ExportItemSpawnerPresetsInZone` |
| `#ExportQuests` | `#FindSquadMember` | `#ForceBBEncounterOnNearbyOwnedBase` | `#ForceEncounterAtPlayerLocation` |
| `#GardenPlantRandomPlants` | `#Inventory` | `#KnockoutPrisoner` | `#LeaveCorpse` |
| `#Loot` | `#MutePlayer` | `#Quests` | `#RandomizePriceDeltas` |
| `#ReloadCustomMapConfig` | `#ReloadLootCustomizationsAndResetSpawners` | `#RenameVehicle` | `#ReportDesync` |
| `#ScheduleCargoDrop` | `#ScheduleWorldEvent` | `#ShouldShowOtherPlayerInfo` | `#ShouldShowOtherPlayerLocations` |
| `#SilencePlayer` | `#SkipDiseaseIncubationStage` | `#Sleep` | `#StartTournamentMode` |
| `#TrackShotsFired` | `#UnmutePlayer` | `#UnsilencePlayer` | `#UpgradeBaseBuildingElementsWithinRadius` |
| `#VehicleCheat` | `#Vote` |

---
_Source: scumdump v23451409 AdminCommand_* classes x bridge handler registry (TurdMODEngineBridge.cpp). Generated 2026-06-07._