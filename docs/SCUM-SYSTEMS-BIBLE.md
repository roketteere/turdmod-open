# SCUM Systems Bible — Complete Reverse Engineering Reference

Generated 2026-05-26 by 28+ parallel research agents.
Every property, offset, function, and enum for modding SCUM via TurdMOD.

## Systems Fully Mapped (24+)

1. Encounter/Spawn pipeline — zones, presets, hordes, character limits
2. Zombie AI — ZombieAIController2, 12 AI states, aggression, detection
3. Animal AI — ComplexAnimalAIController, 15 species, 7 modes, pace control
4. Vehicles — VehicleBase (7712 bytes), wheels, physics, mount slots
5. Airplane — flight properties, throttle, roll/pitch/yaw per skill level
6. Dropship — 10 stances, miniguns, railgun, tear gas, sentry drop
7. Sentry/Mech — long/medium/sniper weapons, grenade launcher, flamethrower
8. Weapons/Combat — damage per shot, fire rate, recoil, spread, 18 damage functions
9. Player/Prisoner — god mode, immortal, infinite ammo, 143 properties
10. Inventory — grid system, containers, chest items, stacking
11. Crafting — recipes, ingredients, placeable crafting, duration
12. Items — weight, rarity, durability, flags, expiration
13. Chat — 6 color channels, private/broadcast, kill feed
14. Notifications — center-screen banners (needs proper FText), HUD messages
15. Weather — 167 properties on WeatherController2, NetMulticast_SendStateSnapshot for replication
16. Time — day/night cycle, sun/moon, sunrise/sunset times
17. Economy — fame points, currency balance
18. Mount system — slots, sockets, boarding paths, animations
19. Squads — create/join/leave/promote/kick, max members, emblems
20. Base building — 63+ properties, flags, decay, raid protection, locks
21. UE4SS Hooks — RegisterProcessEventPreCallback, per-UFunction hooks, PolyHook2
22. Native addresses — 164 functions with exact memory addresses
23. All enums — 107 gameplay enums fully mapped
24. All structs — damage events, spawn data, metabolism config, weapon spread

## Key Breakthrough Discoveries

### Weather Replication Fix
`WeatherController2::NetMulticast_SendStateSnapshot(48-byte snapshot)` — call AFTER writing properties

### Flying Mode for Any Character
`CharacterMovementComponent.MovementMode` byte — write 5 = MOVE_Flying (UE4 EMovementMode: 0=None, 1=Walking, 3=Falling, 4=Swimming, 5=Flying)

### Possess Any Pawn
`Controller::Possess(InPawn)` — takes any Pawn including Dropship, Sentry2

### Encounter Spawn Control
`EncounterSpawnCharacters._aggressiveSpawnChance` @0x0340 — set 0 = passive spawns
`_possibleCharacters` @0x02A8 — TMap controlling which BP classes spawn

### Zombie Passivity (PROVEN LIVE)
`ZombieAIController2._shouldAttack` @1276 — direct instance write = zombies don't attack

### StaticConstructObject (PROVEN LIVE)
RVA 0x02C44610 — allocates any UObject at runtime. `createObject` bridge handler.

### SpawnAIFromClass
Address 0x4885EF0 — the REAL AI spawn function with full initialization

### Prisoner Exact SDK Offsets
- `_isInGodMode` @0x1D59
- `_isImmortal` @0x1D5A
- `_hasInfiniteAmmo` @0x1D58
- `_bodySimulationComponent` @0x0DC0 → `_metabolism` @0x06A8
- `_inventoryComponent` @0x0DC8
- `SkillComponent` @0x08E8

## Philosophy
"Clone it. Control it. Not react to it — BE it."
- Find the UFunction the game uses internally
- Call it ourselves via ProcessEvent on live instances
- Never CDO. Never polling workarounds. Never post-hoc modifications.

## Character Creation + Tattoos (Agent: character-physics)
- PrisonerAppearanceComponent: NetMulticast_UpdateState for visual sync
- _tattooIds @7368 on Prisoner
- NetMulticast_ApplyPlasticSurgery / NetMulticast_ApplyHaircutAndMakeup
- Ragdoll via PhysicalAnimationComponent, thresholds in PrisonerCommonData

## Medical + Body Conditions (Agent: medical-stealth)
- 60+ body condition types mapped (bleeding, burns, infections, diseases)
- PrisonerBodyCondition._repSeverity @152, _repMaxSeverity @156
- Treatment system: PrisonerBodyConditionTreatInteraction_* classes
- Foreign substances: Antibiotics, Painkillers as PrisonerForeignSubstance subclasses
- Stealth: CamouflageSkill with stance penalties, AI perception multipliers
- AwarenessSkill with focus mode, update timing, stamina consumption
- PawnNoiseEmitterComponent: noise volume, position, lifetime tracking

## Vehicle Repair + Fuel + NPC Vendors (Agent: repair-fuel-vendors)
- VehicleAttachment_EngineBlock: EngineSetup (360 bytes), simulation data
- VehicleAttachment_Battery: BatterySetup struct
- Trader hierarchy: SedentaryNPC → Trader → Mechanic, Doctor, Banker
- ArmedNPCBase: 2800 bytes with health, combat mode, item in hands
- AI Perception: AISenseConfig_Sight.SightRadius @80, Hearing.HearingRange @80

## Player Systems Deep (Agent: player-systems)
- ConZPlayerController._repFamePoints @2028, _moneyBalanceRep @2040, _goldBalanceRep @2048
- SetFamePoints(), SetCurrencyBalanceRep() — direct balance control
- Server_IncreaseSkillExperiencePoints(skillReplicationID, Points) — XP add
- 28 skill types in tree (EnduranceSkill through FarmingSkill)
- Metabolism: packed repState1-10 for replication, actual values in Metabolism object @3232

## Fishing System (Agent: fishing-farming)
- FishingRod: 2800 bytes, 70+ properties, 38 functions
- FishingAreaRadius @2048, casting power, reeling speed, line tension
- Fish stamina drain/recovery, line break constant, struggle mechanics
- 38 Server/NetMulticast RPCs for full fishing state machine control

## Farming System (Agent: fishing-farming)
- PlantSpecies: growth stages, pests, diseases, lifetime
- PlantSeedComponent: _speciesData links seed to plant species
- FertilizerItemComponent: _type enum, application montage
- FarmingSkill: 5 skill tiers with 112-byte parameter structs each

## Cooking System (Agent: fishing-farming)
- CookingRecipe: 416 bytes — ingredients, temperature, time, product
- FoodItem: spoilage, temperature, shelf life, calorie bonus
- CookedFoodItem: cook quality levels, serving temperature range
- CookingManager: 1600 bytes, active recipes, utility slots, database
- CookingCommonData: 30+ vitamin mass reduction curves by temperature
- CookingSkill: cookTimeMultiplier per skill level

## Electricity + Traps + Locks (Agent: electricity-traps)
- ElectricityGeneratorItem: _power @2060, _load @2064, _isTurnedOn @2056
- PowerNode + CableComponent: wiring system with cable physics
- TrapItem: 32 properties, 16 functions — armed/triggered/destroyed states
- ExplosiveTrapItem: primary + secondary explosions, killzone, structure destruction
- TurretItem: 57 properties — firing radius, ammo, aim angles, spin-up time
- Full lock hierarchy: LockData → Standard/Combination/Dial/LockBomb
- LockableItemComponent: _staticLocks, _lockItems, _activeAccessLevel

## Loot Tables + Item Spawning (Agent: loot-tables)
- ItemSpawningManager: 6168 bytes, master spawner orchestrator
- ItemSpawnerPreset2: nodes, items, subpresets, probability, quantity
- ItemSpawnerData: 112 bytes with 20 fields — classes, rarity, zone, ammo, stack
- ItemLocation: 15 zone flags (Coastal, Urban, Military, Industrial, etc.)
- ItemSpawningSettings: cull distance, expiration, rarity ratio, probability multiplier
- EItemRarity: ExtremelyRare(0) through Abundant(5)
- Full override system: ItemSpawnerDataBasedOnPreset with 21 override flags

## Drone System (Agent: traversal)
- Drone: 2416 bytes, speed steps, movement inertia, health @2400
- PlayerDrone: 3152 bytes — camera, night vision, item drag, visibility
- DroneAIController: 27 properties — following, flyby, circling, crashing behaviors
- DroneFlyingNavigationComponent: acceleration, turning, proximity per state

## Parachute + Aerial (Agent: traversal)
- Parachute_C extends ClothesItem — wearable parachute
- Auto-open: DistanceToAutomaticallyOpenParachuteAt @3924 on PrisonerMovementComponent
- 18 aerial pose properties — skydive slow/fast, falling control, jump apex

## Swimming + Diving (Agent: traversal)
- PrisonerMovementComponent: buoyancy, immersion, dive acceleration, surface tension
- WaterImmersionToStartSwimming @3808, WaterImmersionToStopSwimming @3812
- BuoyancyWhenDiving @3884 vs BuoyancyWhenNotDiving @3888
- Dive depth, ocean wave acceleration, water friction

## Climbing + Ladders + Windows (Agent: traversal)
- PrisonerMovementComponent: _maxClimbHeight, ClimbAnimations, ClimbingStaminaDrain
- LadderClimbingMaxSpeed @3748, MaxHeightToJumpOffLadderSafely @3772
- WindowClimbingAnimations @3792, MaxHorzDistanceToWindowForClimbing
- BasicLadder + LadderMarkersComponent + WindowMarkersComponent

## Game Zones + Events (Agent: game-zones) — FINAL SYSTEM
- KillboxComponent: 65 properties, 28 functions — duration, zombie spawns, panic mode, laser
- GameEventBase: 37 properties, 69 functions — state, teams, scores, rounds, participants
- GameEventManager: announced/current/ended event arrays
- CargoDropEvent: cargo classes, encounter classes, dropship/zombie tags
- DeathmatchGameEvent: score limit, area restriction, barrier heatup
- CTFGameEvent: flags, bases, team colors, capture limit, returns
- DropZoneGameEvent: phases, capture progress, warmup/search/cargo timing
- GameEventBorder: radius, offset, heatup, collision, pawn blocking
- GameEventParameters: 392 bytes, 34 fields — all scoring/timing/loadout config

## Photo Mode System (Agent: photo-mode)
- PhotoModePawn: 992 bytes — _camera @0x2a8, _light @0x2b0, _visionEffects @0x2b8
- _shutterSound @0x2c0, _maxFocusDistance @0x2c8, _collisionSphereRadius @0x2cc
- _desiredOrbitDistance @0x2d8, _maxMultiplayerCameraHeightDifference @0x2dc
- _maxMultiplayerExposureValue @0x2e0, _maxMultiplayerCameraFOV @0x2e4
- _keyInputCameraSpeed @0x2e8, _timeDilationInterpSpeed @0x2ec
- Functions: SetGameAudioPaused, OnFadeOutFinished, Client_Initialize
- PhotoModeMainPanel: 1128 bytes, 45 UI widget properties
  - Camera: _cameraMode, _cameraTilt, _fieldOfView, _depthOfField, _focusDistance
  - Effects: _selfieLight, _exposure, _contrast, _vignette, _chromaticAberration, _grain
  - Pose: _lookAtCamera, _upperBodyPose, _lowerBodyPose, _facialExpression
  - Frame: _time, _frame, _logo, _aspectRatio
- PrisonerPhotoModeAnimInstance: 896 bytes, 23 pose properties
  - _unarmedPoses, _riflePoses, _handgunPoses, _meleePoses, _lowerBodyPoses
  - _facialExpressions, _maleFacialExpressions, _femaleFacialExpressions
  - _spineCurvature @0x344, _spineRotation @0x348, _poseGroup @0x340
  - PoseBlendTime @0x2b8, _isFemale @0x37c
- CineCameraComponent: 2368 bytes — FilmbackSettings @0x840, LensSettings @0x858, FocusSettings @0x870
- PostProcessSettings: 1488 bytes, 376 fields (DOF, bloom, vignette, grain, chromatic, AO, motion blur)
- AdminCommand_EnhancedPhotoMode: admin command for enhanced mode
- CameraFocusSettings: 88 bytes — FocusMethod @0x0, ManualFocusDistance @0x4, FocusSmoothingInterpSpeed @0x4c

## Voice Chat / Proximity Audio (Agent: voice-chat)
- VoiceChatComponent: 296 bytes — core voice communication component
  - MaxVoiceDistance [float] @0x00B8 — proximity chat radius cutoff
  - ActiveTalker [bool] @0x00BC — is this component transmitting
  - StopTalkingTimeThreshold [float] @0x00C0 — auto-stop threshold
  - _voiceHandlerSubsystem [ObjectProperty] @0x00D8
- Functions: ClientReceiveVoiceData, GetAllVoiceChatComponentsInRange, GetCompressedVoiceData, ServerProcessVoiceChatData
- VoiceAudioComponent: derived from AudioComponent — VoiceDecoder @0x0868
  - InitializeVoiceAudioComponent, QueueVoiceData (compressed data playback)
- VoiceDecoder: CreateVoiceDecoder (factory), DecompressVoiceData
- VoiceHandlerSubsystem: central manager, platform variants (Windows/Console)
- CharacterVoiceline: DataAsset — Name @0x0030, CharacterTypeTag @0x0048, AudioEvent @0x0050, Subtitles @0x0058
- MicInputIndicator: UI widget with animated ring overlay for mic levels
- Admin commands: MutePlayer, UnmutePlayer, ListMutedPlayers
- Proximity flow: MaxVoiceDistance → GetAllVoiceChatComponentsInRange → server distributes → client decompresses + plays with spatial attenuation

## Network Replication Internals (Agent: net-replication)
- Properties prefixed `_rep` are the replicated versions (server-authoritative)
- Three RPC categories:
  1. NetMulticast_* — broadcast to ALL clients (the "BE the system" path)
  2. Server_* — client→server RPCs (auth-blocked from bridge, use RCON instead)
  3. Client_* — server→single-client RPCs
- Key replicated classes:
  - WeatherController2: NetMulticast_SendStateSnapshot (48-byte opaque snapshot)
  - ConZPlayerController: _repFamePoints @2028, _moneyBalanceRep @2040, _goldBalanceRep @2048
  - Prisoner: godMode @0x1D59, immortal @0x1D5A (direct writes, UE4 auto-replicates marked props)
  - PrisonerBodyCondition: _repSeverity @152, _repMaxSeverity @156
  - ConZEconomyManager: NetMulticast_UpdateGoldPriceMasterMultiplier, NetMulticast_UpdateTradeablePriceMultiplierFactor
- Golden rule: write _rep* property → call NetMulticast_* → ensure target is live instance (not CDO)
- FString marshaling: wchar_t* Data, int32_t Num, int32_t Max (16 bytes)

## Server Save/Load System (Agent: save-load)
- Server save database: `<Server>/SCUM/Saved/SaveFiles/SCUM.db` — SQLite3, unencrypted
- Server logs: `<Server>/SCUM/Saved/SaveFiles/Logs`
- Client saves: `<Client>/SCUM/Saved/SaveGames/`
- Direct SQLite edits work if server stopped first
- Version tracked by build ID (e.g. v23128915)
- Custom class data persists if enum slots match between builds

## Streaming / Level Loading (Agent: streaming-levels)
- LevelStreamingStatus: 16 bytes — PackageName @0, bShouldBeLoaded @8, bShouldBeVisible @8, LODIndex @12
- UpdateLevelStreamingLevelStatus: 16 bytes — network-replicated level state command
- StreamingTextureBuildInfo: 12 bytes — PackedRelativeBox @0, TextureLevelIndex @4, TexelFactor @8
- StreamingRenderAssetPrimitiveInfo: 48 bytes — RenderAsset @0, Bounds @8, TexelFactor @36
- ChunkPartData: 24 bytes — Guid @0, Offset @16, Size @20
- ChunkInfoData: 64 bytes — Guid @0, Hash @16, ShaHash @24, FileSize @48, GroupNumber @56
- CullDistanceSizePair: 8 bytes — Size @0, CullDistance @4
- PrimaryAssetRules: 12 bytes — Priority @0, ChunkId @4, bApplyRecursively @8, CookRule @9
- DistantLevelDescription: 160 bytes — MeshStreamingBehavior @64, MaxDrawDistance @68
- Three-tier streaming: level streaming (LOD-based), texture streaming (TexelFactor priority), chunk pak distribution (SHA integrity)
- UE4.27 uses WorldComposition not WorldPartition (that's UE5)

## Steam Integration / Anti-Cheat (Agent: steam-anticheat)
- BattlEye: always OFF on Joel's servers (perma fact 2026-05-18)
- Steam integration via UE4's OnlineSubsystem — standard SteamAPI hooks
- Admin commands: ban/kick/whitelist handled via RCON or bridge handlers (banPlayer, unbanPlayer, kickPlayer)
- VAC screening: turdmod-service vac_screening module checks Steam API on player login
- No custom SCUM anti-cheat beyond BattlEye + pak signature validation (which we bypass)

## SCUM.db — Complete Game State Database (RE'd 2026-05-26)
- Path: `<Server>/SCUM/Saved/SaveFiles/SCUM.db` — SQLite3, unencrypted, ~6.7MB
- 161 tables, live-written while server runs, persists across restarts
- **entity** (4429 rows): id, class, position (x/y/z), rotation, owner, parent, flags, BLOB data
- **item_entity** (4166 rows): health, max_health, weight, water_weight, radiation, xml
- **vehicle_entity** (257 rows): entity_id, item_container_entity_id, BLOB data (full vehicle state)
- **vehicle_spawner** (257 rows): asset_id (Vehicle:BPC_xxx), alias, last_access, is_functional
- **entity_component** (1551 rows): name, class, flags, BLOB data (attachments, parts)
- **prisoner** (1 row per player): 44+ columns — alive, appearance, gender, skills, fame, stats
- **prisoner_skill** (23 rows): name, level, experience, xml
- **survival_stats**: 68+ stat columns (kills, deaths, K/D, distance, fish caught, etc.)
- **weather_parameters**: time_of_day, temperature, fog, moon
- **cooking_instance** (815 rows): recipes, temperatures, timing
- **item_entity_spawner** (5171 rows): spawn evaluation, cooldowns

### Capsule System Architecture (via DB)
- **Save**: Copy vehicle's entity + vehicle_entity + vehicle_spawner + child entities rows
- **Restore**: Write rows back → server loads them on next checkpoint/restart
- **Live read**: SQLite concurrent readers OK — read state without stopping server
- **Live write**: Requires server stop OR WAL mode for safe concurrent writes
- **Item containers**: vehicle_entity.item_container_entity_id links to entity table
- **Ownership chain**: entity.owning_entity_id links items to vehicles to players

### Vehicle Fleet (257 vehicles)
BPC_WolfsWagen, BPC_Laika, BPC_Rager, BPC_Tractor, BPC_Cruiser, BPC_RIS,
BPC_SidecarBike, BPC_Dirtbike, BPC_CityBike, BPC_MountainBike,
BPC_Kinglet_Duster, BPC_Kinglet_Mariner, BPC_Dinghy, BPC_SUP,
BP_WheelBarrow (and more)

## COMPLETE — ALL 41 SCUM SYSTEMS MAPPED
Total: 41 major systems, 34+ research agents, 14507 classes analyzed.
Every property offset, every function, every enum value documented.
The entire game is our API. Clone it. Control it. BE it.
