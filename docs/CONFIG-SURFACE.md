# SCUM server config surface (build 23451409)

Every config file the server/BattlEye reads, and the keys each accepts. Generated from
SCUMServer.exe strings. ServerSettings.ini holds the 580 `scum.*` keys below.

## Config files

| File | Purpose | Contents |
|---|---|---|
| `ServerSettings.ini` | main server tuning | 580 `scum.*` keys (sections below) |
| `GameUserSettings.ini` | client/display defaults | resolution, audio, UI |
| `Input.ini` | keybinds | input mappings |
| `AdminUsers.ini` | regular admins | SteamID64 [+[BracketPerms]] per line |
| `ServerSettingsAdminUsers.ini` | super-admins | bare SteamID64 = full |
| `ExclusiveUsers.ini` | whitelist (exclusive) | SteamID64 list |
| `WhitelistedUsers.ini` | whitelist | SteamID64 list |
| `SilencedUsers.ini` | muted players | SteamID64 list |
| `BannedUsers.ini` | bans | SteamID64 list |
| `BattlEye/BEServer_x64.cfg` | BattlEye + RCON | GameID, MasterPort, RConPassword/RConPort/RConIP |

## JSON data overrides (powerful — structured data, not key/value)

| File | Overrides |
|---|---|
| `EconomyOverride.json` | trader prices / economy |
| `Notifications.json` | scheduled server messages |
| `RaidTimes.json` | raid windows |
| `Zones.json` / `GeneralZoneModifiers.json` | zone definitions + modifiers |
| `Entities.json` | entity spawns |
| `CooldownGroups.json` | item cooldown groups |
| `CustomQuestList.json` / `DefaultQuestList.json` | quests |
| `BlockedQuests.json` | disabled quests |

## ServerSettings.ini — all 580 `scum.*` keys, by category

### Encounter (40)
`EncounterBaseCharacterAmountMultiplier`, `EncounterCanClampCharacterNumWhenOutOfResources`, `EncounterCanRemoveLowPriorityCharacters`, `EncounterCharacterAINoiseResponseRadiusMultiplier`, `EncounterCharacterAggressiveSpawnChanceOverride`, `EncounterCharacterRespawnBatchSizeMultiplier`, `EncounterCharacterRespawnDistanceMaxOverrideLTZ`, `EncounterCharacterRespawnDistanceMaxOverrideLargePOI`, `EncounterCharacterRespawnDistanceMinOverrideLTZ`, `EncounterCharacterRespawnDistanceMinOverrideLargePOI`, `EncounterCharacterRespawnTimeMultiplier`, `EncounterCharacterSpawnDistanceMaxOverrideLTZ`, `EncounterCharacterSpawnDistanceMaxOverrideLargePOI`, `EncounterCharacterSpawnDistanceMinOverrideLTZ`, `EncounterCharacterSpawnDistanceMinOverrideLargePOI`, `EncounterDebugMode`, `EncounterEnableSpawnPreventionAreaSpawnOnCharacterDeath`, `EncounterExtraCharacterPerPlayerMultiplier`, `EncounterExtraCharacterPlayerCapMultiplier`, `EncounterGlobalZoneCooldownMultiplier`, `EncounterHTZRadiusMultiplier`, `EncounterHordeActivationChanceMultiplier`, `EncounterHordeBaseCharacterAmountMultiplier`, `EncounterHordeExtraCharacterPerPlayerMultiplier`, `EncounterHordeExtraCharacterPlayerCapMultiplier`, `EncounterHordeGroupBaseCharacterAmountMultiplier`, `EncounterHordeGroupExtraCharacterPerPlayerMultiplier`, `EncounterHordeGroupExtraCharacterPlayerCapMultiplier`, `EncounterHordeGroupRefillTimeMultiplier`, `EncounterHordeNoiseCheckCooldownMultiplier`, `EncounterHordePuppetHordeActivationScreamOverrideChance`, `EncounterHordeShouldPlayActivationSound`, `EncounterHordeSpawnDistanceMultiplier`, `EncounterLTZRadiusMultiplier`, `EncounterMTZRadiusMultiplier`, `EncounterManagerDebugMode`, `EncounterManagerZoneDebugMode`, `EncounterNeverRespawnCharacters`, `EncounterVirtualizedTimeOverride`, `EncounterZoneActivationDistanceMultiplier`

### Enable (23)
`Enable3DAudio`, `EnableAirplaneFlightAssist`, `EnableBCULocking`, `EnableDeena`, `EnableDeenaOnServer`, `EnableDigitalDeluxeFreeGoldCard`, `EnableDigitalDeluxeStarterPack`, `EnableDropshipAbandonedBunkerEncounter`, `EnableDropshipBaseBuildingEncounter`, `EnableEncounterManagerLowPlayerCountMode`, `EnableExplosionDebugger`, `EnableFog`, `EnableItemCooldownGroups`, `EnableLockedLootContainers`, `EnableLogOnGardenSearch`, `EnableLootPuppetHorde`, `EnableNetWatchdog`, `EnableNetworkObjectLogging`, `EnableNewPlayerProtection`, `EnableSelectedSkillsPromotion`, `EnableSentryRespawning`, `EnableSpawnOnGround`, `EnableSquadMemberNameWidget`

### Allow (22)
`AllowAdminChat`, `AllowAutomaticParachuteOpening`, `AllowComa`, `AllowCrosshair`, `AllowEvents`, `AllowFirstPerson`, `AllowFlagPlacementOnBBElements`, `AllowFloorPlacementOnHalfAndLowWalls`, `AllowGlobalChat`, `AllowKillClaiming`, `AllowLocalChat`, `AllowMapScreen`, `AllowMinesAndTraps`, `AllowMultipleFlagsPerPlayer`, `AllowSectorRespawn`, `AllowShelterRespawn`, `AllowSkillGainInSafeZones`, `AllowSquadChat`, `AllowSquadmateRespawn`, `AllowThirdPerson`, `AllowVoting`, `AllowWallPlacementOnHalfAndLowWalls`

### Max (16)
`MaxAllowedAnimals`, `MaxAllowedBirds`, `MaxAllowedCharacters`, `MaxAllowedDrones`, `MaxAllowedHunts`, `MaxAllowedKillboxKeycards`, `MaxAllowedKillboxKeycards_PoliceStation`, `MaxAllowedKillboxKeycards_RadiationZone`, `MaxAllowedNPCs`, `MaxAllowedPuppets`, `MaxPing`, `MaxPingCheckEnabled`, `MaxPlayers`, `MaxQuestsPerCyclePerTrader`, `MaxServerTickRate`, `MaxSimultaneousQuestsPerTrader`

### Base (13)
`BaseBuildingAttackerSentryDamageMultiplier`, `BaseBuildingAttackerSentryGrenadeDamageMultiplier`, `BaseBuildingAttackerSentryHealthMultiplier`, `BaseBuildingAttackerSentryRailgunDamageMultiplier`, `BaseBuildingDestructionLogDamageThreshold`, `BaseBuildingEncounterDamagePercentageIncreasePerSquadMember`, `BaseBuildingEncounterMaximumMinToEndReduction`, `BaseBuildingEncounterMinNumElementsToEnd`, `BaseBuildingEncounterMinNumElementsToStart`, `BaseBuildingEncounterTimeToFullMinNumToEnd`, `BaseBuildingEncounterTriggerChance`, `BaseBuildingEncounterTriggerTimeMultiplier`, `BaseElementsDecayRateMultiplier`

### Debug (13)
`DebugAirplane`, `DebugDamage`, `DebugHeatSources`, `DebugHumanoidObstacleDetection`, `DebugInfectiousChangeRate`, `DebugMeleeKnockout`, `DebugProjectileCollisions`, `DebugResting`, `DebugSedentaryNPCBackgroundInteractions`, `DebugVehicle2W`, `DebugVehicleBattery`, `DebugVehicleFuel`, `DebugVehicleMassProperties`

### Squad (12)
`SquadCooldownResetMultiplier`, `SquadFamePointsPenaltyPerPrevSquadMember`, `SquadMemberCountAtIntLevel1`, `SquadMemberCountAtIntLevel2`, `SquadMemberCountAtIntLevel3`, `SquadMemberCountAtIntLevel4`, `SquadMemberCountAtIntLevel5`, `SquadMemberCountLimitForPunishment`, `SquadMoneyPenaltyPerPrevSquadMember`, `SquadRespawnCooldown`, `SquadRespawnInitialTime`, `SquadRespawnPrice`

### Abandoned (10)
`AbandonedBunkerActiveDurationHours`, `AbandonedBunkerBCUTerminalCooldown`, `AbandonedBunkerCommotionThreshold`, `AbandonedBunkerCommotionThresholdPerPlayerExtra`, `AbandonedBunkerEnemyActivationThreshold`, `AbandonedBunkerEnemyActivationThresholdPerPlayerExtra`, `AbandonedBunkerKeyCardActiveDurationHours`, `AbandonedBunkerMaxSimultaneouslyActive`, `AbandonedBunkerResetArmoryLockersOnActivationOnly`, `AbandonedBunkerZonaManagerDebugMode`

### Raid (10)
`RaidProtectionEnableLog`, `RaidProtectionFlagSpecificChangeSettingCooldown`, `RaidProtectionFlagSpecificChangeSettingPrice`, `RaidProtectionFlagSpecificMaxProtectionTime`, `RaidProtectionGlobalShouldShowRaidAnnouncementMessage`, `RaidProtectionGlobalShouldShowRaidStartEndMessages`, `RaidProtectionGlobalShouldShowRaidTimesMessage`, `RaidProtectionOfflineMaxProtectionTime`, `RaidProtectionOfflineProtectionStartDelay`, `RaidProtectionType`

### Mouse (9)
`MouseSensitivityATM`, `MouseSensitivityBombDefusal`, `MouseSensitivityDTS`, `MouseSensitivityDrone`, `MouseSensitivityFP`, `MouseSensitivityLockpicking`, `MouseSensitivityPhone`, `MouseSensitivityScope`, `MouseSensitivityTP`

### Show (9)
`ShowAbandonQuestWarning`, `ShowAdditionalItemInfoWithoutHover`, `ShowAnnouncementMessages`, `ShowChatTimestamps`, `ShowCutItemWarning`, `ShowMusicPlayerDisplay`, `ShowSimpleTooltipOnHover`, `ShowUnofficialServerWarning`, `ShowVehicleDebug`

### Gasoline (8)
`GasolinePeriodicInitialAmountMultiplier`, `GasolinePeriodicMaxAmountMultiplier`, `GasolinePeriodicReplenishAmountMultiplier`, `GasolinePeriodicReplenishIntervalMultiplier`, `GasolinePricePerUnitMultiplier`, `GasolineProximityReplenishAmountMultiplier`, `GasolineProximityReplenishChanceMultiplier`, `GasolineProximityReplenishTimeoutMultiplier`

### Hunt (8)
`HuntFailureDistance`, `HuntFailureTime`, `HuntTriggerChanceOverride_ContinentalForest`, `HuntTriggerChanceOverride_ContinentalMeadow`, `HuntTriggerChanceOverride_Mediterranean`, `HuntTriggerChanceOverride_Mountain`, `HuntTriggerChanceOverride_Urban`, `HuntTriggerChanceOverride_Village`

### Propane (8)
`PropanePeriodicInitialAmountMultiplier`, `PropanePeriodicMaxAmountMultiplier`, `PropanePeriodicReplenishAmountMultiplier`, `PropanePeriodicReplenishIntervalMultiplier`, `PropanePricePerUnitMultiplier`, `PropaneProximityReplenishAmountMultiplier`, `PropaneProximityReplenishChanceMultiplier`, `PropaneProximityReplenishTimeoutMultiplier`

### Sentry (8)
`SentryBaseBuildingDamageMultiplier`, `SentryBlindMode`, `SentryCannotFire`, `SentryDamageMultiplier`, `SentryDebugMode`, `SentryGrenadeDamageMultiplier`, `SentryHealthMultiplier`, `SentryRailgunDamageMultiplier`

### Water (8)
`WaterPeriodicInitialAmountMultiplier`, `WaterPeriodicMaxAmountMultiplier`, `WaterPeriodicReplenishAmountMultiplier`, `WaterPeriodicReplenishIntervalMultiplier`, `WaterPricePerUnitMultiplier`, `WaterProximityReplenishAmountMultiplier`, `WaterProximityReplenishChanceMultiplier`, `WaterProximityReplenishTimeoutMultiplier`

### Armed (7)
`ArmedNPCDamageMultiplier`, `ArmedNPCDifficultyLevel`, `ArmedNPCHealthMultiplier`, `ArmedNPCLimpingHealthThreshold`, `ArmedNPCNetCullDistanceOverride`, `ArmedNPCRunningSpeedMultiplier`, `ArmedNPCSpreadMultiplier`

### Cargo (7)
`CargoDropCooldownMaximum`, `CargoDropCooldownMinimum`, `CargoDropDropshipEncounterWeightMultiplier`, `CargoDropFallDelay`, `CargoDropFallDuration`, `CargoDropSelfdestructTime`, `CargoDropZombieEncounterWeightMultiplier`

### Disable (7)
`DisableBaseBuilding`, `DisableExamineGhost`, `DisableExhaustion`, `DisableLootPuppetSpawning`, `DisableSentrySpawning`, `DisableSuicidePuppetSpawning`, `DisableTimedGifts`

### Dropship (7)
`DropshipAbandonedBunkerEncounterTriggerChance`, `DropshipBaseBuildingElementsDamageMultiplier`, `DropshipDamageMultiplier`, `DropshipDebugMode`, `DropshipHealthMultiplier`, `DropshipRailgunDamageMultiplier`, `DropshipWorldEncounterSpawnWeightMultiplier`

### Maximum (7)
`MaximumAmountOfElementsPerFlag`, `MaximumBaseProximityWhenSpawning`, `MaximumDurabilityOfArmedNPCsDroppedItemFromHands`, `MaximumNumberOfExpandedElementsPerFlag`, `MaximumTimeForChestsInForbiddenZones`, `MaximumTimeForVehiclesInForbiddenZones`, `MaximumTimeOfVehicleInactivity`

### Item (6)
`ItemCooldownGroupsDurationMultiplier`, `ItemDecayDamageMultiplier`, `ItemVirtualizationEventProcessingTimeBudget`, `ItemVirtualizationRelevancyUpdatePeriod`, `ItemVirtualizationVisitorBounds`, `ItemVirtualizationVisitorDistanceTravelledForUpdate`

### Kinglet (6)
`KingletDusterMaxAmount`, `KingletDusterMaxFunctionalAmount`, `KingletDusterMinPurchasedAmount`, `KingletMarinerMaxAmount`, `KingletMarinerMaxFunctionalAmount`, `KingletMarinerMinPurchasedAmount`

### Shelter (6)
`ShelterCooldownResetMultiplier`, `ShelterPricePerSquadmateModifier`, `ShelterRespawnCooldown`, `ShelterRespawnInitialTime`, `ShelterRespawnPrice`, `ShelterRespawnPriceOutsideFlagArea`

### Turrets (6)
`TurretsAttackAnimals`, `TurretsAttackArmedNPCs`, `TurretsAttackPrisoners`, `TurretsAttackPuppets`, `TurretsAttackSentries`, `TurretsAttackVehicles`

### Battery (5)
`BatteryChargeWithAlternatorMultiplier`, `BatteryChargeWithDynamoMultiplier`, `BatteryDrainFromDevicesMultiplier`, `BatteryDrainFromEngineMultiplier`, `BatteryDrainFromInactivityMultiplier`

### Custom (5)
`CustomMapCenterXCoordinate`, `CustomMapCenterYCoordinate`, `CustomMapEnabled`, `CustomMapHeight`, `CustomMapWidth`

### Puppet (5)
`PuppetCullDistanceOverride`, `PuppetHealthMultiplier`, `PuppetLimpingHealthThreshold`, `PuppetRunningSpeedMultiplier`, `PuppetWorldEncounterSpawnWeightMultiplier`

### Quests (5)
`QuestsEnabled`, `QuestsGlobalCycleDuration`, `QuestsNoticeBoardRefillCooldown`, `QuestsPhoneRefillCooldown`, `QuestsTraderRefillCooldown`

### Random (5)
`RandomCooldownResetMultiplier`, `RandomPricePerSquadmateModifier`, `RandomRespawnCooldown`, `RandomRespawnInitialTime`, `RandomRespawnPrice`

### Sector (5)
`SectorCooldownResetMultiplier`, `SectorPricePerSquadmateModifier`, `SectorRespawnCooldown`, `SectorRespawnInitialTime`, `SectorRespawnPrice`

### Server (5)
`ServerBannerUrl`, `ServerDescription`, `ServerName`, `ServerPassword`, `ServerPlaystyle`

### Fame (4)
`FameGainMultiplier`, `FamePointPenaltyOnDeath`, `FamePointPenaltyOnKilled`, `FamePointRewardOnKill`

### Human (4)
`HumanToHumanArmedMeleeDamageMultiplier`, `HumanToHumanDamageMultiplier`, `HumanToHumanThrowingDamageMultiplier`, `HumanToHumanUnarmedMeleeDamageMultiplier`

### Airplane (3)
`AirplaneMaxAmount`, `AirplaneMaxFunctionalAmount`, `AirplaneMinPurchasedAmount`

### Animal (3)
`AnimalDebugMode`, `AnimalNetCullDistanceOverride`, `AnimalWorldEncounterSpawnWeightMultiplier`

### Bicycle (3)
`BicycleMaxAmount`, `BicycleMaxFunctionalAmount`, `BicycleMinPurchasedAmount`

### Commit (3)
`CommitSuicideCooldown`, `CommitSuicideCooldownResetMultiplier`, `CommitSuicideInitialTime`

### Cruiser (3)
`CruiserMaxAmount`, `CruiserMaxFunctionalAmount`, `CruiserMinPurchasedAmount`

### Delete (3)
`DeleteBannedUsers`, `DeleteDuplicateChestsOnServerStartup`, `DeleteInactiveUsers`

### Dinghy (3)
`DinghyMaxAmount`, `DinghyMaxFunctionalAmount`, `DinghyMinPurchasedAmount`

### Dirtbike (3)
`DirtbikeMaxAmount`, `DirtbikeMaxFunctionalAmount`, `DirtbikeMinPurchasedAmount`

### First (3)
`FirstPersonDrivingFOV`, `FirstPersonFOV`, `FirstPlantHarvestAdditionalChance`

### Hide (3)
`HideKillNotification`, `HideLifeIndicators`, `HideQuickAccessBar`

### Laika (3)
`LaikaMaxAmount`, `LaikaMaxFunctionalAmount`, `LaikaMinPurchasedAmount`

### Log (3)
`LogChestOwnership`, `LogSuicides`, `LogVehicleDestroyed`

### Logout (3)
`LogoutTimer`, `LogoutTimerInBunker`, `LogoutTimerWhileCaptured`

### Master (3)
`MasterServerIsLocalTest`, `MasterServerUpdateSendInterval`, `MasterVolume`

### Motorboat (3)
`MotorboatMaxAmount`, `MotorboatMaxFunctionalAmount`, `MotorboatMinPurchasedAmount`

### Rager (3)
`RagerMaxAmount`, `RagerMaxFunctionalAmount`, `RagerMinPurchasedAmount`

### Ris (3)
`RisMaxAmount`, `RisMaxFunctionalAmount`, `RisMinPurchasedAmount`

### Setting (3)
`Setting_EnableCharacterCreationIntroVideo`, `Setting_EnableDigitalDeluxeBiomeSuit`, `Setting_ForceFirstPersonWhenDrawingBows`

### Shadow (3)
`ShadowPrecision`, `ShadowQuality`, `ShadowResolution`

### Sidecar (3)
`SidecarBikeMaxAmount`, `SidecarBikeMaxFunctionalAmount`, `SidecarBikeMinPurchasedAmount`

### Tractor (3)
`TractorMaxAmount`, `TractorMaxFunctionalAmount`, `TractorMinPurchasedAmount`

### Use (3)
`UseBuildingProximityRestrictions`, `UseMapBaseBuildingRestriction`, `UsePaniniProjection`

### Visualize (3)
`VisualizeAvailabilityGrid`, `VisualizeBulletTrajectories`, `VisualizeThrowingTrajectories`

### Weapon (3)
`WeaponDecayDamageOnFiring`, `WeaponRackMaxAmountPerFlagArea`, `WeaponRackStartDecayingIfFlagAreaHasMoreThan`

### Wheelbarrow (3)
`WheelbarrowMaxAmount`, `WheelbarrowMaxFunctionalAmount`, `WheelbarrowMinPurchasedAmount`

### Wolfswagen (3)
`WolfswagenMaxAmount`, `WolfswagenMaxFunctionalAmount`, `WolfswagenMinPurchasedAmount`

### Distance (2)
`DistanceFieldAmbientOcclusion`, `DistanceFieldShadows`

### Effects (2)
`EffectsQuality`, `EffectsVolume`

### Examine (2)
`ExamineSpawnerExpirationTimeMultiplier`, `ExamineSpawnerProbabilityMultiplier`

### Fishing (2)
`FishingDebugMode`, `FishingHighActivityZoneRotationTime`

### Foliage (2)
`FoliageLODDithering`, `FoliageQuality`

### Invert (2)
`InvertAirplaneMouseY`, `InvertMouseY`

### Is (2)
`IsFirstPlaySession`, `IsTelemetrySet`

### Last (2)
`LastEntitlementFlags`, `LastUserProfile`

### Life (2)
`LifeIndicatorTransparency`, `LifeIndicatorVisibilityPreference`

### Message (2)
`MessageOfTheDay`, `MessageOfTheDayCooldown`

### Name (2)
`NameChangeCooldown`, `NameChangeCost`

### Oven (2)
`OvenMaxAmountPerFlagArea`, `OvenStartDecayingIfFlagAreaHasMoreThan`

### Perception (2)
`PerceptionDangerDecreaseInterpSpeed`, `PerceptionDangerIncreaseInterpSpeed`

### Player (2)
`PlayerMinimalVotingInterest`, `PlayerPositiveVotePercentage`

### Puppets (2)
`PuppetsCanOpenDoors`, `PuppetsCanVaultWindows`

### Quick (2)
`QuickAccessTransparency`, `QuickAccessVisibilityPreference`

### Razor (2)
`RazorAIControllerDebugMode`, `RazorDebugMode`

### SUPMax (2)
`SUPMaxAmount`, `SUPMaxFunctionalAmount`

### Smoker (2)
`SmokerMaxAmountPerFlagArea`, `SmokerStartDecayingIfFlagAreaHasMoreThan`

### Spawner (2)
`SpawnerExpirationTimeMultiplier`, `SpawnerProbabilityMultiplier`

### Stamina (2)
`StaminaDrainOnClimbMultiplier`, `StaminaDrainOnJumpMultiplier`

### Survival (2)
`SurvivalSkillMultiplier`, `SurvivalTipLevel`

### Texture (2)
`TextureMemory`, `TextureQuality`

### Third (2)
`ThirdPersonDrivingFOV`, `ThirdPersonFOV`

### Turret (2)
`TurretMaxAmountPerFlagArea`, `TurretStartDecayingIfFlagAreaHasMoreThan`

### Wall (2)
`WallWeaponRackMaxAmountPerFlagArea`, `WallWeaponRackStartDecayingIfFlagAreaHasMoreThan`

### Well (2)
`WellMaxAmountPerFlagArea`, `WellStartDecayingIfFlagAreaHasMoreThan`

### Zombie (2)
`ZombieDamageMultiplier`, `ZombieDebugMode`

### Aim (1)
`AimDownSightsMode`

### Archery (1)
`ArcherySkillMultiplier`

### Armor (1)
`ArmorAbsorptionOutput`

### Auto (1)
`AutoStartFirstDeenaTask`

### Automatic (1)
`AutomaticParachuteOpening`

### Aviation (1)
`AviationSkillMultiplier`

### Awareness (1)
`AwarenessSkillMultiplier`

### Bear (1)
`BearMaxHealthMultiplier`

### Bedroll (1)
`BedrollVisibilityTimer`

### Bloom (1)
`BloomQuality`

### Boar (1)
`BoarMaxHealthMultiplier`

### Body (1)
`BodySimulationSpeedMultiplier`

### Brawling (1)
`BrawlingSkillMultiplier`

### Camera (1)
`CameraBobbingIntensity`

### Camouflage (1)
`CamouflageSkillMultiplier`

### Cardiophobia (1)
`CardiophobiaMode`

### Chest (1)
`ChestAcquisitionDuration`

### Chicken (1)
`ChickenMaxHealthMultiplier`

### Chromatic (1)
`ChromaticAbberation`

### Client (1)
`ClientSettingsVersion`

### Cloud (1)
`CloudShadowQuality`

### Clouds (1)
`CloudsQuality`

### Con (1)
`ConZFlyingDebugMode`

### Cooking (1)
`CookingSkillMultiplier`

### DLSSFr (1)
`DLSSFrameGeneration`

### DLSSSu (1)
`DLSSSuperResolution`

### Days (1)
`DaysSinceLastLoginToBecomeInactive`

### Deer (1)
`DeerMaxHealthMultiplier`

### Default (1)
`DefaultInventorySortType`

### Demolition (1)
`DemolitionSkillMultiplier`

### Depth (1)
`DepthOfFieldQuality`

### Display (1)
`DisplayMode`

### Donkey (1)
`DonkeyMaxHealthMultiplier`

### Driving (1)
`DrivingSkillMultiplier`

### Endurance (1)
`EnduranceSkillMultiplier`

### Engineering (1)
`EngineeringSkillMultiplier`

### Extra (1)
`ExtraElementsPerFlagForAdditionalSquadMember`

### FSR (1)
`FSR`

### Farming (1)
`FarmingSkillMultiplier`

### Film (1)
`FilmGrain`

### Fish (1)
`FishDebugMode`

### Flag (1)
`FlagOvertakeDuration`

### Fog (1)
`FogQuality`

### Food (1)
`FoodDecayDamageMultiplier`

### Fuel (1)
`FuelDrainFromEngineMultiplier`

### Full (1)
`FullWipe`

### Gamma (1)
`Gamma`

### Garden (1)
`GardenMaxAmountPerFlagArea`

### Goat (1)
`GoatMaxHealthMultiplier`

### Gold (1)
`GoldWipe`

### Graphics (1)
`GraphicsPreset`

### Handgun (1)
`HandgunSkillMultiplier`

### Horse (1)
`HorseMaxHealthMultiplier`

### Killbox (1)
`KillboxDefuseFailureBonus`

### Language (1)
`Language`

### Large (1)
`LargeAquaticAnimalDebugMode`

### Lens (1)
`LensFlareQuality`

### Light (1)
`LightShafts`

### Limit (1)
`LimitGlobalChat`

### Lock (1)
`LockProtectionDamageMultiplier`

### Loot (1)
`LootRework`

### Lower (1)
`LowerShadowQualityDuskTillDawn`

### Maintain (1)
`MaintainItemsExpirationTime`

### Medical (1)
`MedicalSkillMultiplier`

### Melee (1)
`MeleeWeaponsSkillMultiplier`

### Min (1)
`MinServerTickRate`

### Minimum (1)
`MinimumDurabilityOfArmedNPCsDroppedItemFromHands`

### Motion (1)
`MotionBlur`

### Motorcycle (1)
`MotorcycleSkillMultiplier`

### Movement (1)
`MovementInertiaAmount`

### Music (1)
`MusicVolume`

### Nametag (1)
`NametagMode`

### Near (1)
`NearObjectBlur`

### New (1)
`NewPlayerProtectionDuration`

### Nighttime (1)
`NighttimeDarkness`

### Nudity (1)
`NudityCensoring`

### PINCen (1)
`PINCensoring`

### Partial (1)
`PartialWipe`

### Permadeath (1)
`PermadeathThreshold`

### Plant (1)
`PlantHarvestExamineTimeMultiplier`

### Play (1)
`PlaySafeIdProtection`

### Post (1)
`PostProcessingQuality`

### Print (1)
`PrintEnvironmentDescription`

### Probability (1)
`ProbabilityForArmedNPCToDropItemFromHandsWhenSearched`

### Push (1)
`PushToTalk`

### Quest (1)
`QuestRequirementsBlockTradeableItems`

### RTSqua (1)
`RTSquadProbationDuration`

### Rabbit (1)
`RabbitMaxHealthMultiplier`

### Radio (1)
`RadioMode`

### Reflection (1)
`ReflectionQuality`

### Reflex (1)
`Reflex`

### Refraction (1)
`RefractionQuality`

### Render (1)
`RenderScale`

### Resolution (1)
`Resolution`

### Restorable (1)
`RestorableMeshInstancesManagerRestoreTimeDilation`

### Rifles (1)
`RiflesSkillMultiplier`

### Running (1)
`RunningSkillMultiplier`

### Rusty (1)
`RustyLocksLogging`

### SUPMin (1)
`SUPMinPurchasedAmount`

### Secret (1)
`SecretBunkerKeyCardActiveDurationHours`

### Separate (1)
`SeparateTranslucencyPass`

### Settings (1)
`SettingsVersion`

### Should (1)
`ShouldDestroyEntitiesOutsideMapLimitsOnRestart`

### Skeletal (1)
`SkeletalMeshLODBias`

### Skip (1)
`SkipInfectiousDiseaseIncubationStage`

### Sniping (1)
`SnipingSkillMultiplier`

### Spawn (1)
`SpawnEncountersInThreatZonesIgnoringBaseBuilding`

### Speaker (1)
`SpeakerConfiguration`

### Start (1)
`StartTimeOfDay`

### Stealth (1)
`StealthSkillMultiplier`

### Sunrise (1)
`SunriseTime`

### Sunset (1)
`SunsetTime`

### Telemetry (1)
`TelemetryLevel`

### Thievery (1)
`ThieverySkillMultiplier`

### Time (1)
`TimeOfDaySpeed`

### Tonemapper (1)
`TonemapperQuality`

### Translucency (1)
`TranslucencyVolumeBlur`

### UIVolu (1)
`UIVolume`

### VSync (1)
`VSync`

### View (1)
`ViewDistance`

### Virtualized (1)
`VirtualizedItemBounds`

### Voice (1)
`VoiceChatVolume`

### Voiceline (1)
`VoicelineVolume`

### Voting (1)
`VotingDuration`

### Welcome (1)
`WelcomeMessage`

### Wolf (1)
`WolfMaxHealthMultiplier`

### showVe (1)
`showVehicleDebug`
