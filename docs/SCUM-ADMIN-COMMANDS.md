# SCUM Native Admin Commands (`#` console)

**217 commands**, extracted from the SCUM **client reflection** (`scumdump v23451409`, `AdminCommand_*` classes). These are the built-in `#`-commands admins type in chat. Each class carries `_verb` (typed token), `_argumentDescriptions`, `_numberOfRequiredArguments`, and `_requiredExecutorLevel`. The token is the name below (from the class name; `_verb` default = same in nearly all cases).

> Authoritative native list — NOT visible in server files, only in client reflection. `#WUMBO`-style commands are NOT here, confirming they are custom mod additions (e.g. Whalley).

---

## Teleport / movement (9)

| `#EquipParachute` | `#Location` | `#MapTeleport` |
| `#Sleep` | `#Teleport` | `#TeleportTo` |
| `#TeleportTo3pm` | `#TeleportToMe` | `#TeleportToVehicle` |

## Player / prisoner state (30)

| `#AddPrisonerBodyEffect` | `#ClearFakeName` | `#DisablePrisonerBodyEffects` |
| `#ExecutePrisonerBodyConditionInteraction` | `#Inventory` | `#KnockoutPrisoner` |
| `#ListPrisonerBodyConditionInteractions` | `#ListPrisonerBodyEffects` | `#ListPrisonerForeignSubstances` |
| `#Loot` | `#RemovePrisonerBodyEffect` | `#SetAllInventoryAccess` |
| `#SetBodyType` | `#SetFakeName` | `#SetGender` |
| `#SetGodMode` | `#SetHealthToItemInHands` | `#SetInfiniteAmmo` |
| `#SetPrisonerAttributes` | `#SetPrisonerBladderVolume` | `#SetPrisonerExhaustion` |
| `#SetPrisonerImmortality` | `#SetPrisonerInfiniteOxygen` | `#SetPrisonerInfiniteStamina` |
| `#SetPrisonerMetabolismSimulationSpeed` | `#SetPrisonerStamina` | `#SetPrisonerStomachVolume` |
| `#SetSkillLevel` | `#SetSuperJump` | `#SkipDiseaseIncubationStage` |

## Players / moderation (19)

| `#BanPlayer` | `#FindSquadMember` | `#GetUserID` |
| `#GetUserIDByRank` | `#GrantElevatedStatus` | `#KickPlayer` |
| `#ListMutedPlayers` | `#ListPlayers` | `#ListSilencedPlayers` |
| `#MutePlayer` | `#PlayerInfo` | `#RevokeElevatedStatus` |
| `#ShouldShowOtherPlayerInfo` | `#ShouldShowOtherPlayerLocations` | `#ShowNameplates` |
| `#SilencePlayer` | `#UnbanPlayer` | `#UnmutePlayer` |
| `#UnsilencePlayer` |

## Spawning (19)

| `#CookRecipe` | `#CreateEntity` | `#DestroyEntity` |
| `#PrintEntities` | `#SpawnAllItems` | `#SpawnAnimal` |
| `#SpawnArmedNPC` | `#SpawnBrenner` | `#SpawnDebugAnimalTrack` |
| `#SpawnInventoryFullOf` | `#SpawnItem` | `#SpawnPrimaryActorAsset` |
| `#SpawnRandomAnimal` | `#SpawnRandomPrimaryActorAsset` | `#SpawnRandomZombie` |
| `#SpawnRazor` | `#SpawnReflectionSphere` | `#SpawnVehicle` |
| `#SpawnZombie` |

## Vehicles (10)

| `#DestroyAllVehicles` | `#DestroyVehicle` | `#ListSpawnedVehicles` |
| `#RenameVehicle` | `#SetMountedVehicleProperty` | `#ShowVehicleDebug` |
| `#ShowVehicleInfo` | `#ShowVehicleLocations` | `#VehicleCheat` |
| `#VisualizeVehicleTrajectory` |

## World / time / weather (17)

| `#CheckServerTime` | `#EnhancedPhotoMode` | `#ListWeatherControllerOverrides` |
| `#PrintGlobalRaidProtectionRaidTimes` | `#ReloadCustomMapConfig` | `#ScheduleCargoDrop` |
| `#ScheduleWorldEvent` | `#SetDecayTimeDilation` | `#SetGardenPlantingTime` |
| `#SetTime` | `#SetTimeSpeed` | `#SetWeather` |
| `#SetWeatherControllerOverrideActive` | `#SetWeatherControllerOverrideValue` | `#ShowRespawnTimes` |
| `#ToggleAmbientSound` | `#ToggleFog` |

## Economy / fame (14)

| `#ChangeCurrencyBalance` | `#ChangeCurrencyBalanceToAll` | `#ChangeCurrencyBalanceToAllOnline` |
| `#ChangeFamePoints` | `#RandomizePriceDeltas` | `#ResetEconomy` |
| `#ResetPlayerBalances` | `#SetCurrencyBalance` | `#SetCurrencyBalanceToAll` |
| `#SetCurrencyBalanceToAllOnline` | `#SetFamePoints` | `#SetFamePointsToAll` |
| `#SetFamePointsToAllOnline` | `#ToggleFamePointsDebugVisualization` |

## Base building / flags (13)

| `#DestroyAllBaseBuildingElementsForFlag` | `#DestroyAllBaseBuildingElementsForPlayer` | `#DestroyAllBaseBuildingElementsForSquad` |
| `#DestroyAllBaseBuildingElementsWithinRadius` | `#DestroyAllFlagsForPlayer` | `#DestroyFlag` |
| `#ListFeatureFlags` | `#ListFlags` | `#SetFeatureFlag` |
| `#ShowBaseBuildingDebug` | `#ShowFlagInfo` | `#ShowFlagLocations` |
| `#UpgradeBaseBuildingElementsWithinRadius` |

## Cleanup (radius destroy) (7)

| `#DestroyAllItemsWithinRadius` | `#DestroyAllRazorsWithinRadius` | `#DestroyArmedNPCsWithinRadius` |
| `#DestroyCorpsesWithinRadius` | `#DestroyEncountersAtPlayerLocation` | `#DestroyZombiesWithinRadius` |
| `#LeaveCorpse` |

## Encounters / quests / hunts (12)

| `#ClearEncounterCooldowns` | `#DrawNearbyEncounters` | `#DumpEncounterManagerData` |
| `#EnableHuntingClueDebugArrow` | `#ExportQuests` | `#ForceBBEncounterOnNearbyOwnedBase` |
| `#ForceEncounterAtPlayerLocation` | `#ListActiveAbandonedBunkers` | `#ListActiveHunts` |
| `#ListActiveSecretBunkers` | `#Quests` | `#ResetAllHuntCooldownsForPlayer` |

## Squads (5)

| `#DumpAllSquadsInfoList` | `#ListSquadMembers` | `#ListSquads` |
| `#ResetSquadInfo` | `#SquadInfo` |

## Garden / farming (5)

| `#AddGardenPlantPest` | `#GardenPlantRandomPlants` | `#SetFarmingSimulationSpeed` |
| `#SetGardenNutrientsHigh` | `#SetGardenPlantGrowthStage` |

## Server / admin (18)

| `#AddOrRemoveWidget` | `#AdminLight` | `#Announce` |
| `#CancelVote` | `#CrashMajestically` | `#EnableAdminViolations` |
| `#EnableGameplayMetadataLogging` | `#EnableOrDisableServer` | `#EndTournamentMode` |
| `#ExecuteConsoleCommand` | `#ReportDesync` | `#ResetAchievements` |
| `#SendNotification` | `#SetAchievementUnlocked` | `#SetDeluxeVersion` |
| `#ShutdownServer` | `#StartTournamentMode` | `#Vote` |

## Debug / visualize / misc (39)

| `#ArmorAbsorptionOutput` | `#BoatDebug` | `#DebugProjectileCollisions` |
| `#DebugWeapon` | `#DemolitionSkillDebug` | `#DistanceDebug` |
| `#DoorDebug` | `#DrawDebugZombieCapsulesOnLegacySpawnPoints` | `#DrawSentryHealthBar` |
| `#DumpWetnessDebug` | `#ExportDefaultItemSpawnerPresets` | `#ExportDefaultItemSpawningCooldownGroups` |
| `#ExportDefaultItemSpawningParameters` | `#ExportItemLootTree` | `#ExportItemSpawnerPresetsInZone` |
| `#GetMeshInfo` | `#ListItemsSpawnLocations` | `#ListPrimaryAssets` |
| `#ListSpawnedAnimals` | `#ListSpawnedArmedNPCs` | `#PlacementDebug` |
| `#ReloadLootCustomizationsAndResetSpawners` | `#SetAIInvisibility` | `#SetAirplaneMaxVelocity` |
| `#SetCraftingSearch` | `#SetItemDebugMode` | `#SetMalfunctionProbability` |
| `#SetReplishableResourceAmount` | `#SetShouldPrintExamineSpawnerPresets` | `#ShowWeaponInfo` |
| `#ToggleZombieNavigationLogging` | `#TrackShotsFired` | `#TrapsDebug` |
| `#VisualizeAnimalLocation` | `#VisualizeArmedNPCLocation` | `#VisualizeBulletTrajectories` |
| `#VisualizePath` | `#VisualizePlayerAiming` | `#VisualizeZombieLocation` |

---

## Executor levels & gated commands (why some `#` commands fail for the owner)

Every `AdminCommand_*` carries `_requiredExecutorLevel`. The level enum (engine reflection):

```
EExecutorStatus: Regular(0) < Admin(1) < SuperAdmin(2) < Elevated(3) < Developer(4)
```

| Source | Level granted |
|---|---|
| `AdminUsers.ini` entry | **Admin(1)** |
| `ServerSettingsAdminUsers.ini` (owner) | **SuperAdmin(2)** |
| `#GrantElevatedStatus` | **Elevated(3)** |
| (dev build / unlock) | Developer(4) |

Most commands need Admin/SuperAdmin and work for any listed admin or the owner. A few require
**Elevated/Developer** and return **"not authorized" even for the owner (SuperAdmin)** — they're
dev/single-player commands with no admin-file grant on a live MP server.

**`#SetAttributes` (AdminCommand_SetPrisonerAttributes) is one of these** — gated above SuperAdmin.
Bypass: write the values straight into the save. Base attributes (STR/CON/DEX/INT) live in
`prisoner.body_simulation` — see **[PRISONER-BODYSIM-FORMAT.md](PRISONER-BODYSIM-FORMAT.md)** and
`tools/scum-attrs.py`. Verified live on OVH 2026-06-07 (set TechyRican + Zilla to 8/5/5/5 with the
server stopped, no `#` command). For commands that genuinely need the Elevated tier, try
`#GrantElevatedStatus <steamid>` first (engine bridge: `grantElevatedStatus`).
