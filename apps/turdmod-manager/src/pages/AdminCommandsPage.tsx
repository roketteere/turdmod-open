import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useQuery } from '@tanstack/react-query';
import { listServers, testServer, type ServerProfile } from '../lib/tauri-servers';
import { Link } from 'react-router-dom';
import adminCommandsByCategory from '../data/admin-commands.json';
import { engineRpc } from '../lib/tauri-engine';
import { HostToggle } from '../components/HostToggle';
import { useEngineStatus } from '../hooks/useEngine';
import {
  useAdminCommandsMetadata,
  type BridgeCommandSpec,
} from '../hooks/useAdminCommandsMetadata';

type RouteMode = 'rcon' | 'bridge';

// Roster entry returned by the bridge's getOnlinePlayers handler.
interface PlayerEntry {
  name: string;
  steamId?: string;
  ptr?: string;
}

interface GetOnlinePlayersResp {
  count?: number;
  players?: PlayerEntry[];
}

// Convert camelCase / snake_case hint names into pretty labels:
// `currencyType` → `Currency Type`, `fake_name` → `Fake Name`, `x` → `X`.
function formatLabel(name: string): string {
  if (!name) return '';
  return name
    .replace(/[_-]+/g, ' ')
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/\b([a-z])/g, (_, c: string) => c.toUpperCase())
    .replace(/\s+/g, ' ')
    .trim();
}

// Distinguish local vs remote server profiles by host. Local is anything
// resolving to the loopback range; everything else is treated as remote.
function getHostKind(host: string): 'local' | 'remote' {
  const h = host.toLowerCase().trim();
  if (h === 'localhost' || h === '127.0.0.1' || h.startsWith('127.') || h === '0.0.0.0' || h === '::1') {
    return 'local';
  }
  return 'remote';
}

function hostKindIcon(kind: 'local' | 'remote'): string {
  return kind === 'local' ? '🏠' : '☁';
}

function hostKindLabel(kind: 'local' | 'remote'): string {
  return kind === 'local' ? 'Local' : 'Remote';
}

// Translate raw probe / RCON errors into actionable hints. SCUM doesn't
// enable RCON by default — most "timeout" failures mean the server isn't
// listening on the configured port at all.
function rconErrorHint(err: string | null, isLocal: boolean): string | null {
  if (!err) return null;
  const e = err.toLowerCase();
  if (e.includes('timeout') || e.includes('connection refused') || e.includes('no connection')) {
    return isLocal
      ? 'SCUM RCON is probably disabled on this server. Add `scum.RconEnabled=True`, `scum.RconPort=27015`, `scum.RconPassword=…` to ServerSettings.ini and restart. — OR use the TurdCOM route below (no RCON required).'
      : 'Host is unreachable on this port. Confirm the RCON port is open + the host accepts external connections (G-Portal: enable RCON in panel and whitelist your IP).';
  }
  if (e.includes('auth') || e.includes('password')) {
    return 'RCON authentication failed. Re-enter the RCON password in the Servers page (it must match `scum.RconPassword=` in ServerSettings.ini).';
  }
  if (e.includes('no rcon_port') || e.includes('rcon_port')) {
    return 'This server profile has no RCON port. Edit the profile on the Servers page and add the port + password.';
  }
  return null;
}

// ---------------------------------------------------------------------------
// AdminCommandsPage — dispatcher for SCUM's built-in #admin command verbs,
// sent through the existing RCON pipe.
//
// Why this page exists: SCUM has 214 AdminCommand_* classes (SetCurrency
// Balance, SetGodMode, SetWeather, SpawnPrimaryActorAsset, …) but they all
// expose zero UFunctions through reflection, so the bridge can't call them
// via ProcessEvent. They're authenticated through SCUM's own admin / RCON
// channel — which our Manager already speaks via manager_server_rcon. So
// the right wrapper is a UI on top of RCON, not more bridge handlers.
// ---------------------------------------------------------------------------

type Category =
  | 'player'
  | 'bulk'
  | 'world'
  | 'spawn'
  | 'destructive'
  | 'inspect'
  | 'garden'
  | 'debug'
  | 'misc';

interface ArgHint {
  name: string;
  hint: string;
  defaultValue?: string;
  // input type guides the renderer — 'bool' = toggle, 'enum' = dropdown,
  // 'player' = autocomplete-friendly text, 'number' / 'text' = plain.
  kind?: 'text' | 'number' | 'bool' | 'enum' | 'player';
  options?: { value: string; label: string }[];
}

interface CommandSpec {
  verb: string;
  category: Category;
  hints?: ArgHint[];
  description?: string;
  // True when the command takes no args and can fire as a single
  // button (we infer this if hints is empty and the verb is in the
  // ZERO_ARG set, otherwise it gets a free-text fallback).
  zeroArg?: boolean;
}

// Shared enum option sets. Numeric enums are labeled `(N) Name` so
// users see both the underlying RCON value and the human name. String
// enums (like weather preset names) just show the name.

// SCUM's #SetCurrencyBalance / #ChangeCurrencyBalance expect the LITERAL
// strings `Normal` or `Gold` — not 0/1. Verified 2026-05-18 from SCUM's
// in-game `#help` autocomplete (Joel confirmed `ChangeCurrencyBalance
// Normal 5000 SampleAdmin` worked while the 0/1 form silently rejected).
const CURRENCY_OPTIONS: ArgHint['options'] = [
  { value: 'Normal', label: 'Normal (cash)' },
  { value: 'Gold', label: 'Gold' },
];

// ESkillLevel from enums.json — 0=NoSkill .. 4=AboveAdvanced. 5=Count
// and 6=_MAX are sentinels, not real ranks.
const SKILL_LEVEL_OPTIONS: ArgHint['options'] = [
  { value: '0', label: '(0) NoSkill' },
  { value: '1', label: '(1) Basic' },
  { value: '2', label: '(2) Medium' },
  { value: '3', label: '(3) Advanced' },
  { value: '4', label: '(4) AboveAdvanced' },
];

// Skills SCUM's #SetSkillLevel accepts (strings, no numeric form).
// Derived from ESkillReplicationID minus the `Skill` suffix; matches
// what #ListPossibleSkills returns in-game.
const SKILL_NAMES = [
  'Endurance', 'Resistance', 'Running', 'Swimming', 'Medical',
  'Awareness', 'Stealth', 'AnimalHandling', 'Cooking', 'Survival',
  'BioChem', 'Boxing', 'MeleeWeapons', 'Rifles', 'Handgun',
  'Sniping', 'Camouflage', 'Tactics', 'Throwing', 'Archery',
  'Thievery', 'Driving', 'Motorcycle', 'Engineering', 'Demolition',
  'Aviation', 'Farming',
];
const SKILL_NAME_OPTIONS: ArgHint['options'] = SKILL_NAMES.map((s) => ({
  value: s,
  label: s,
}));


// Curated argument hints for high-frequency commands. Every other
// command falls back to a single free-text args input.
const ARG_HINTS: Record<string, ArgHint[]> = {
  // Currency: SCUM syntax is `#SetCurrencyBalance <Normal|Gold> <Amount> [Player]`
  // — currency first, amount second, player OPTIONAL and last (defaults
  // to caller when omitted). The numeric 0/1 form silently fails.
  SetCurrencyBalance: [
    { name: 'currencyType', hint: 'Normal or Gold', defaultValue: 'Normal', kind: 'enum', options: CURRENCY_OPTIONS },
    { name: 'amount', hint: '10000', kind: 'number' },
    { name: 'player', hint: '(optional) defaults to you', kind: 'player' },
  ],
  ChangeCurrencyBalance: [
    { name: 'currencyType', hint: 'Normal or Gold', defaultValue: 'Normal', kind: 'enum', options: CURRENCY_OPTIONS },
    { name: 'amount', hint: '+/- amount', kind: 'number' },
    { name: 'player', hint: '(optional) defaults to you', kind: 'player' },
  ],
  // *ToAll variants — no player arg (apply to everyone in DB).
  SetCurrencyBalanceToAll: [
    { name: 'currencyType', hint: 'Normal or Gold', defaultValue: 'Normal', kind: 'enum', options: CURRENCY_OPTIONS },
    { name: 'amount', hint: '10000', kind: 'number' },
  ],
  SetCurrencyBalanceToAllOnline: [
    { name: 'currencyType', hint: 'Normal or Gold', defaultValue: 'Normal', kind: 'enum', options: CURRENCY_OPTIONS },
    { name: 'amount', hint: '10000', kind: 'number' },
  ],
  ChangeCurrencyBalanceToAll: [
    { name: 'currencyType', hint: 'Normal or Gold', defaultValue: 'Normal', kind: 'enum', options: CURRENCY_OPTIONS },
    { name: 'amount', hint: '+/- amount', kind: 'number' },
  ],
  ChangeCurrencyBalanceToAllOnline: [
    { name: 'currencyType', hint: 'Normal or Gold', defaultValue: 'Normal', kind: 'enum', options: CURRENCY_OPTIONS },
    { name: 'amount', hint: '+/- amount', kind: 'number' },
  ],
  // SetFamePoints / ChangeFamePoints / SetFamePointsToAllOnline:
  // intentionally NOT curated. Their previous hand-curated entries had
  // arg order inverted (player first, amount second) vs SCUM's actual
  // `<Amount> [Player]` syntax — silently rejected by the parser. The
  // live bridge dump has the correct order + names ("Amount", "Player")
  // straight from SCUM's _argumentDescriptions, so let that drive.
  Announce: [{ name: 'message', hint: 'Server-wide announcement text' }],
  // Verbs Ban / Kick / Silence / SetImmortality / SetInfiniteStamina /
  // SetInfiniteOxygen / Mute / Unban / Unmute / Unsilence are intentionally
  // NOT curated here. SCUM renamed them out from under us (was BanPlayer,
  // KickPlayer, SilencePlayer, SetPrisonerImmortality, etc.); the live
  // bridge dump has the correct names + arg signatures so let it drive.
  SetGodMode: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' },
  ],
  SetInfiniteAmmo: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' },
  ],
  SetSuperJump: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' },
  ],
  SetAllInventoryAccess: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' },
  ],
  SetSkillLevel: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'skill', hint: 'skill name', defaultValue: 'Rifles', kind: 'enum', options: SKILL_NAME_OPTIONS },
    { name: 'level', hint: 'rank 0-4', defaultValue: '1', kind: 'enum', options: SKILL_LEVEL_OPTIONS },
  ],
  SetTime: [
    { name: 'hours', hint: '0-23', kind: 'number' },
    { name: 'minutes', hint: '0-59', defaultValue: '0', kind: 'number' },
  ],
  SetTimeSpeed: [
    { name: 'multiplier', hint: '1.0 = normal', defaultValue: '1.0', kind: 'number' },
  ],
  SetWeatherControllerOverrideActive: [
    { name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' },
  ],
  SpawnVehicle: [
    { name: 'assetName', hint: 'e.g. BPC_Kinglet_Duster' },
    { name: 'count', hint: '1', defaultValue: '1', kind: 'number' },
  ],
  SpawnAnimal: [
    { name: 'assetName', hint: 'e.g. BP_Deer' },
    { name: 'count', hint: '1', defaultValue: '1', kind: 'number' },
  ],
  SpawnZombie: [
    { name: 'assetName', hint: 'e.g. BP_PuppetMale' },
    { name: 'count', hint: '1', defaultValue: '1', kind: 'number' },
  ],
  SpawnItem: [
    { name: 'assetName', hint: 'e.g. M9' },
    { name: 'count', hint: '1', defaultValue: '1', kind: 'number' },
  ],
  Teleport: [
    { name: 'x', hint: 'world X', kind: 'number' },
    { name: 'y', hint: 'world Y', kind: 'number' },
    { name: 'z', hint: 'world Z', defaultValue: '0', kind: 'number' },
  ],
  TeleportTo: [{ name: 'player', hint: 'SampleAdmin', kind: 'player' }],
  SetGender: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    {
      name: 'gender',
      hint: 'M/F',
      defaultValue: '0',
      kind: 'enum',
      options: [
        { value: '0', label: '(0) Male' },
        { value: '1', label: '(1) Female' },
      ],
    },
  ],
  SetBodyType: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    {
      name: 'bodyType',
      hint: 'body type',
      defaultValue: '0',
      kind: 'enum',
      options: [
        { value: '0', label: '(0) Slim' },
        { value: '1', label: '(1) Normal' },
        { value: '2', label: '(2) Athletic' },
        { value: '3', label: '(3) Strong' },
      ],
    },
  ],
  SetFakeName: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'fakeName', hint: 'display name' },
  ],
  SendNotification: [
    { name: 'player', hint: 'SampleAdmin', kind: 'player' },
    { name: 'message', hint: 'Notification text' },
  ],
  SetGardenPlantGrowthStage: [
    { name: 'player', hint: 'SampleAdmin (near a garden)', kind: 'player' },
    {
      name: 'stage',
      hint: 'growth stage',
      defaultValue: '0',
      kind: 'enum',
      options: [
        { value: '0', label: '(0) Seed' },
        { value: '1', label: '(1) Sprout' },
        { value: '2', label: '(2) Young' },
        { value: '3', label: '(3) Mature' },
        { value: '4', label: '(4) Ready' },
      ],
    },
  ],
  SetGardenPlantingTime: [
    { name: 'player', hint: 'SampleAdmin (near a garden)', kind: 'player' },
    { name: 'minutes', hint: 'minutes until ready', defaultValue: '0', kind: 'number' },
  ],
  ScheduleCargoDrop: [
    { name: 'minutes', hint: 'minutes from now', defaultValue: '5', kind: 'number' },
  ],
  ScheduleWorldEvent: [
    { name: 'eventName', hint: 'event identifier' },
    { name: 'minutes', hint: 'minutes from now', defaultValue: '5', kind: 'number' },
  ],
  Vote: [
    { name: 'voteId', hint: 'active vote id' },
    {
      name: 'choice',
      hint: 'yes/no',
      defaultValue: '1',
      kind: 'enum',
      options: [
        { value: '0', label: '(0) No' },
        { value: '1', label: '(1) Yes' },
      ],
    },
  ],
};

// Commands that take zero args — fire as a single button.
const ZERO_ARG = new Set<string>([
  // inspect / list — all return data, no args
  'ListPlayers', 'ListFlags', 'ListMutedPlayers', 'ListSilencedPlayers',
  'ListSpawnedAnimals', 'ListSpawnedArmedNPCs', 'ListSpawnedVehicles',
  'ListSquads', 'ListSquadMembers', 'ListActiveAbandonedBunkers',
  'ListActiveHunts', 'ListActiveSecretBunkers', 'ListPrimaryAssets',
  'ListPrisonerBodyConditionInteractions', 'ListPrisonerBodyEffects',
  'ListPrisonerForeignSubstances', 'ListItemsSpawnLocations',
  'ListWeatherControllerOverrides',
  'DumpAllSquadsInfoList', 'DumpEncounterManagerData', 'DumpWetnessDebug',
  'CheckServerTime', 'PrintEntities', 'PrintGlobalRaidProtectionRaidTimes',
  'ReportDesync', 'Location', 'PlayerInfo', 'SquadInfo', 'Quests',
  // destructive / lifecycle — no args
  'ShutdownServer', 'CrashMajestically', 'ResetEconomy',
  'DestroyAllVehicles',
  // world lifecycle
  'StartTournamentMode', 'EndTournamentMode',
  // misc no-arg
  'CancelVote', 'ReloadCustomMapConfig',
  'ReloadLootCustomizationsAndResetSpawners',
  'RandomizePriceDeltas', 'ClearEncounterCooldowns',
  'GardenPlantRandomPlants', 'SetGardenNutrientsHigh',
]);

// ---------------------------------------------------------------------------
// Pattern groups — verbs that share a common arg shape. Lets us decorate
// dozens of commands with one entry instead of 200 hand-curated hints.
// ---------------------------------------------------------------------------

// Player + ON/OFF toggle
const PLAYER_BOOL_VERBS = new Set<string>([
  'AdminLight', 'EnhancedPhotoMode', 'EquipParachute',
  'MutePlayer', 'UnmutePlayer', 'SilencePlayer', 'UnsilencePlayer',
  'SetAllInventoryAccess', 'SetGodMode', 'SetInfiniteAmmo',
  'SetItemDebugMode', 'SetSuperJump', 'SetPrisonerImmortality',
  'SetPrisonerInfiniteOxygen', 'SetPrisonerInfiniteStamina',
  'SetDeluxeVersion',
]);

// Just ON/OFF (no player)
const BOOL_VERBS = new Set<string>([
  'EnableAdminViolations', 'EnableGameplayMetadataLogging',
  'EnableOrDisableServer',
  'ToggleAmbientSound', 'ToggleFog', 'ToggleZombieNavigationLogging',
  'ToggleFamePointsDebugVisualization',
  'ShowFlagInfo', 'ShowFlagLocations', 'ShowNameplates',
  'ShowRespawnTimes', 'ShowVehicleInfo', 'ShowVehicleLocations',
  'ShowWeaponInfo', 'ShowBaseBuildingDebug', 'ShowVehicleDebug',
  'ShouldShowOtherPlayerInfo', 'ShouldShowOtherPlayerLocations',
  'DrawDebugZombieCapsulesOnLegacySpawnPoints', 'DrawNearbyEncounters',
  'DrawSentryHealthBar',
  'VisualizeAnimalLocation', 'VisualizeArmedNPCLocation',
  'VisualizeBulletTrajectories', 'VisualizePath', 'VisualizePlayerAiming',
  'VisualizeVehicleTrajectory', 'VisualizeZombieLocation',
  'TrackShotsFired', 'SetAIInvisibility',
  'BoatDebug', 'DebugProjectileCollisions', 'DebugWeapon',
  'DemolitionSkillDebug', 'DistanceDebug', 'DoorDebug',
  'EnableHuntingClueDebugArrow', 'PlacementDebug', 'TrapsDebug',
  'SetCraftingSearch', 'SetShouldPrintExamineSpawnerPresets',
]);

// Just player name
const PLAYER_ONLY_VERBS = new Set<string>([
  'TeleportToMe', 'TeleportToVehicle', 'TeleportTo3pm',
  'KnockoutPrisoner', 'LeaveCorpse', 'ClearFakeName',
  'GetUserID', 'Sleep', 'Inventory', 'DisablePrisonerBodyEffects',
  'DestroyAllBaseBuildingElementsForPlayer', 'DestroyAllFlagsForPlayer',
  'ResetAllHuntCooldownsForPlayer', 'ResetPlayerBalances',
  'UnbanPlayer', 'RevokeElevatedStatus', 'GrantElevatedStatus',
]);

// Spawn pattern: assetName + count
const SPAWN_VERBS = new Set<string>([
  'SpawnArmedNPC', 'SpawnRandomAnimal', 'SpawnRandomZombie',
  'SpawnRandomPrimaryActorAsset', 'SpawnBrenner', 'SpawnRazor',
  'SpawnInventoryFullOf', 'SpawnAllItems',
]);

// Player + radius (destroy-within-radius pattern)
const PLAYER_RADIUS_VERBS = new Set<string>([
  'DestroyAllBaseBuildingElementsWithinRadius',
  'DestroyAllItemsWithinRadius', 'DestroyAllRazorsWithinRadius',
  'DestroyArmedNPCsWithinRadius', 'DestroyCorpsesWithinRadius',
  'DestroyZombiesWithinRadius',
  'UpgradeBaseBuildingElementsWithinRadius',
]);

function autoHintsForVerb(verb: string): ArgHint[] | undefined {
  if (PLAYER_BOOL_VERBS.has(verb)) {
    return [
      { name: 'player', hint: 'SampleAdmin', kind: 'player' },
      { name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' },
    ];
  }
  if (BOOL_VERBS.has(verb)) {
    return [{ name: 'enabled', hint: 'on/off', defaultValue: '1', kind: 'bool' }];
  }
  if (PLAYER_ONLY_VERBS.has(verb)) {
    return [{ name: 'player', hint: 'SampleAdmin', kind: 'player' }];
  }
  if (SPAWN_VERBS.has(verb)) {
    return [
      { name: 'assetName', hint: 'asset name (e.g. BP_*)' },
      { name: 'count', hint: '1', defaultValue: '1', kind: 'number' },
    ];
  }
  if (PLAYER_RADIUS_VERBS.has(verb)) {
    return [
      { name: 'player', hint: 'SampleAdmin (centerpoint)', kind: 'player' },
      { name: 'radius', hint: 'meters', defaultValue: '50', kind: 'number' },
    ];
  }
  return undefined;
}

// Convert a bridge dumpAdminCommands record into the UI's ArgHint shape.
// Priority order for picking the right `kind`:
//   1. Numeric dataType → number input
//   2. completionClass "Player" / startsWith("Player_") → player picker
//   3. completionValues populated (Constant-derived BP enum) → enum dropdown
//   4. else String dataType with a class hint → text input with hint
// Live Name/Description from the bridge (read via KismetTextLibrary::
// Conv_TextToString) is used for ArgHint.name + hint when present —
// these are the exact strings SCUM's `#` autocomplete shows.
// Curated ARG_HINTS still win when defined (better arg names and labels).
function bridgeArgsToHints(bcmd: BridgeCommandSpec): ArgHint[] {
  return bcmd.args.map((arg, i) => {
    const isOptional = i >= bcmd.numRequired;
    const liveName = arg.name?.trim();
    const liveDesc = arg.description?.trim();
    const baseName = liveName || `arg${i + 1}`;
    const optionalTag = isOptional ? '(optional) ' : '';

    if (arg.dataType === 'AdminCommandArgumentDataType_Numeric') {
      return {
        name: baseName,
        hint: liveDesc || `${optionalTag}number`,
        kind: 'number',
      };
    }
    if (arg.dataType === 'AdminCommandArgumentDataType_Bool') {
      return {
        name: baseName,
        hint: liveDesc || 'on/off',
        defaultValue: '1',
        kind: 'bool',
      };
    }

    const cc = arg.completionClass ?? '';

    if (cc === 'Player' || cc.startsWith('Player_')) {
      return {
        name: liveName || 'player',
        hint: liveDesc || 'select player',
        kind: 'player',
      };
    }

    if (arg.completionValues.length > 0) {
      return {
        name: baseName,
        hint: liveDesc || cc.replace(/_C$/, ''),
        kind: 'enum',
        options: arg.completionValues.map((v) => ({ value: v, label: v })),
      };
    }

    return {
      name: baseName,
      hint: liveDesc
        ? liveDesc
        : cc
          ? `${optionalTag}${cc.replace(/_C$/, '')}`
          : isOptional
            ? '(optional)'
            : 'value',
      kind: 'text',
    };
  });
}

function resolveHints(
  verb: string,
  metadata?: Map<string, BridgeCommandSpec>,
): ArgHint[] | undefined {
  // Curated > bridge live metadata > pattern fallback > undefined (free-text).
  if (ARG_HINTS[verb]) return ARG_HINTS[verb];
  const bcmd = metadata?.get(verb);
  if (bcmd && bcmd.args.length > 0) return bridgeArgsToHints(bcmd);
  return autoHintsForVerb(verb);
}

// Light heuristic to slot bridge-only verbs (ones not in the static
// admin-commands.json) into a sensible category. Falls back to 'misc'
// when nothing matches — the curated JSON should be updated to give
// these proper homes long-term.
function inferCategory(verb: string): Category {
  if (/^(Spawn|Create)/.test(verb)) return 'spawn';
  if (/^(Destroy|Remove|Reset|Clear|Cancel|Ban|Kick|Shutdown)/.test(verb)) return 'destructive';
  if (/^(Print|Dump|List|Show|Visualize|Get|Find|Check|Report|Track|Draw)/.test(verb)) return 'inspect';
  if (/(Toggle|Enable|Disable|Debug)/.test(verb)) return 'debug';
  if (/(Garden|Plant|Farming)/.test(verb)) return 'garden';
  if (/(ToAll|ToAllOnline)/.test(verb)) return 'bulk';
  if (/(Weather|Time|Map|Tournament|Vote|Encounter|Hunt|Cargo|Quest|Notification|Reload)/.test(verb)) return 'world';
  if (/(Player|Prisoner|Squad|Body|Skill|Currency|Fame|Teleport|Mute|Silence|Sleep|Inventory|Loot|Bleeding|Injury)/.test(verb)) return 'player';
  return 'misc';
}

// Build the decorated verb list. Starts from the static
// admin-commands.json categorisation and then unions in any bridge-only
// verbs (commands that exist as BP _C classes in-game but aren't in the
// curated JSON yet — e.g. AddBleedingInjury, PrintServerEntities).
function buildCommands(metadata?: Map<string, BridgeCommandSpec>): CommandSpec[] {
  const categoryByVerb = new Map<string, Category>();
  const groups = adminCommandsByCategory as Record<string, string[]>;
  for (const cat of Object.keys(groups)) {
    for (const verb of groups[cat]) {
      categoryByVerb.set(verb, cat as Category);
    }
  }

  const allVerbs = new Set<string>(categoryByVerb.keys());
  if (metadata) {
    for (const verb of metadata.keys()) allVerbs.add(verb);
  }

  const out: CommandSpec[] = [];
  for (const verb of allVerbs) {
    const bcmd = metadata?.get(verb);
    const liveDesc = bcmd?.description?.trim() || undefined;
    out.push({
      verb,
      category: categoryByVerb.get(verb) ?? inferCategory(verb),
      hints: resolveHints(verb, metadata),
      description: liveDesc,
      zeroArg: ZERO_ARG.has(verb),
    });
  }
  return out;
}

const CATEGORY_META: Record<Category, { icon: string; label: string; tone: string }> = {
  player:      { icon: '👤', label: 'Player buffs / setters', tone: 'text-sky-300' },
  bulk:        { icon: '👥', label: 'Bulk / mass actions',    tone: 'text-blue-300' },
  world:       { icon: '🌍', label: 'World + time',           tone: 'text-emerald-300' },
  spawn:       { icon: '✨', label: 'Spawn',                  tone: 'text-amber-300' },
  destructive: { icon: '💥', label: 'Destructive',            tone: 'text-red-400' },
  inspect:     { icon: '🔍', label: 'Inspect / list',         tone: 'text-cyan-300' },
  garden:      { icon: '🌱', label: 'Garden',                 tone: 'text-green-300' },
  debug:       { icon: '🐛', label: 'Debug',                  tone: 'text-purple-300' },
  misc:        { icon: '⚙️',  label: 'Misc',                   tone: 'text-turd-cream-dim' },
};

const CATEGORY_ORDER: Category[] = [
  'player',
  'bulk',
  'world',
  'spawn',
  'inspect',
  'garden',
  'debug',
  'destructive',
  'misc',
];

export function AdminCommandsPage() {
  const serversQuery = useQuery({
    queryKey: ['admin-commands-servers'],
    queryFn: listServers,
  });

  const [serverId, setServerId] = useState<string>('');
  useEffect(() => {
    if (!serverId && serversQuery.data && serversQuery.data.length > 0) {
      setServerId(serversQuery.data[0].id);
    }
  }, [serverId, serversQuery.data]);

  const [activeCat, setActiveCat] = useState<Category>('player');
  const [search, setSearch] = useState('');
  const [route, setRoute] = useState<RouteMode>('bridge');
  // Force mode: dispatch with the CanExecute auth gate bypassed so commands run
  // with ZERO admins online (uses an online player as the executor channel).
  // Default ON — it's the whole point of the bridge route. See resolve_admin_gate
  // in the bridge + reference_admin_gate_autolocate.
  const [bypass, setBypass] = useState(true);

  // Online players from the bridge — used to populate the dropdown on
  // every `kind: 'player'` hint. Falls back to free-text if the bridge
  // hasn't returned a roster yet.
  const rosterQuery = useQuery({
    queryKey: ['admin-cmds', 'online-players'],
    queryFn: () => engineRpc<GetOnlinePlayersResp>('getOnlinePlayers', {}),
    refetchInterval: 5000,
    retry: false,
  });
  const playerNames = useMemo(
    () => (rosterQuery.data?.players ?? []).map((p) => p.name).filter(Boolean),
    [rosterQuery.data],
  );

  const selectedProfile = useMemo(
    () => (serversQuery.data ?? []).find((p) => p.id === serverId),
    [serversQuery.data, serverId],
  );
  const hostKind = selectedProfile ? getHostKind(selectedProfile.host) : null;

  // Pull live admin-command metadata from the bridge. Falls back to
  // curated ARG_HINTS + pattern-matched autoHintsForVerb when the bridge
  // is offline (RCON-only mode) or before the dump arrives.
  const adminMetadata = useAdminCommandsMetadata();

  const commands = useMemo(
    () => buildCommands(adminMetadata.byVerb),
    [adminMetadata.byVerb],
  );

  const grouped = useMemo(() => {
    const out = {} as Record<Category, CommandSpec[]>;
    for (const cat of CATEGORY_ORDER) out[cat] = [];
    for (const c of commands) out[c.category].push(c);
    for (const cat of CATEGORY_ORDER) {
      out[cat].sort((a, b) => a.verb.localeCompare(b.verb));
    }
    return out;
  }, [commands]);

  // When a search query is present, span ALL categories — otherwise the
  // user has to guess which category a verb is in. With no query, scope
  // to the active category as before.
  const filteredVerbs = useMemo(() => {
    if (!search.trim()) return grouped[activeCat] ?? [];
    const q = search.toLowerCase();
    return commands
      .filter((c) => c.verb.toLowerCase().includes(q))
      .sort((a, b) => a.verb.localeCompare(b.verb));
  }, [grouped, activeCat, commands, search]);

  return (
    <div className="space-y-4">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="font-display text-lg text-turd-mustard-bright">
            Admin Commands
          </h1>
          <p className="mt-1 max-w-3xl text-xs text-turd-cream-dim">
            Full panel for SCUM's built-in <code className="font-mono">#admin</code>{' '}
            verbs. Default route is <span className="text-turd-mustard-bright">TurdCOM + Force</span>:
            the bridge dispatches each verb through SCUM's real admin parser with the
            auth gate bypassed, so commands run authoritatively with <strong>zero admins
            online</strong> (needs ≥1 player on as an executor channel). RCON is also
            available where enabled.
          </p>
        </div>
        <HostToggle />
      </header>

      <section className="space-y-3 rounded glass p-3">
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex min-w-[260px] flex-1 flex-col gap-1 text-xs">
            <span className="uppercase tracking-wider text-turd-cream-dim/60">
              Target server profile
            </span>
            {serversQuery.isPending ? (
              <span className="text-turd-cream-dim">Loading…</span>
            ) : (serversQuery.data ?? []).length === 0 ? (
              <span className="text-turd-cream-dim">
                No server profiles yet. Add one on the{' '}
                <Link to="/servers" className="text-turd-mustard-bright underline">
                  Servers
                </Link>{' '}
                page with RCON port + password.
              </span>
            ) : (
              <select
                value={serverId}
                onChange={(e) => setServerId(e.target.value)}
                className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
              >
                {(serversQuery.data ?? []).map((p: ServerProfile) => {
                  const k = getHostKind(p.host);
                  return (
                    <option key={p.id} value={p.id}>
                      {hostKindIcon(k)} {hostKindLabel(k)} — {p.name} — {p.host}
                      {p.rconPort ? `:${p.rconPort}` : ' (no RCON port)'}
                    </option>
                  );
                })}
              </select>
            )}
            {selectedProfile && hostKind && (
              <span className="mt-0.5 text-[10px] text-turd-cream-dim">
                Selected: <span className="text-turd-cream">{hostKindIcon(hostKind)} {hostKindLabel(hostKind)}</span>
                {' · '}
                <code className="font-mono text-turd-cream">{selectedProfile.host}{selectedProfile.rconPort ? `:${selectedProfile.rconPort}` : ''}</code>
              </span>
            )}
          </label>
          {route === 'rcon' && serverId && (
            <RconStatusBadge serverId={serverId} isLocal={hostKind === 'local'} />
          )}
          {route === 'bridge' && (
            <BridgeStatusBadge
              playerCount={rosterQuery.data?.count}
              isProbing={rosterQuery.isFetching && !rosterQuery.data}
              error={rosterQuery.error ? String((rosterQuery.error as Error).message ?? rosterQuery.error) : null}
              onRefetch={() => rosterQuery.refetch()}
            />
          )}
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <span className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
            Send via
          </span>
          <div className="flex overflow-hidden rounded border border-turd-bronze/40">
            <button
              onClick={() => setRoute('bridge')}
              className={`px-3 py-1 text-[11px] ${
                route === 'bridge'
                  ? 'bg-turd-mustard-bright text-turd-bg-deep'
                  : 'bg-turd-bg-deep/60 text-turd-cream-dim hover:text-turd-cream'
              }`}
              title="Send commands as admin chat lines through TurdCOM — the TurdMOD bridge (no RCON required, works on local server)"
            >
              TurdCOM
            </button>
            <button
              onClick={() => setRoute('rcon')}
              className={`px-3 py-1 text-[11px] ${
                route === 'rcon'
                  ? 'bg-turd-mustard-bright text-turd-bg-deep'
                  : 'bg-turd-bg-deep/60 text-turd-cream-dim hover:text-turd-cream'
              }`}
              title="Send commands via SCUM's RCON channel (requires scum.RconEnabled=True in ServerSettings.ini)"
            >
              RCON
            </button>
          </div>
          {route === 'bridge' && (
            <label
              className={`flex items-center gap-1.5 rounded border px-2 py-1 text-[11px] ${
                bypass
                  ? 'border-turd-mustard-bright/60 bg-turd-mustard-bright/10 text-turd-mustard-bright'
                  : 'border-turd-bronze/40 text-turd-cream-dim'
              }`}
              title="Force: dispatch on the game thread with the CanExecute auth gate bypassed, so the command runs even with NO admin online. Needs ≥1 player online to supply an executor channel."
            >
              <input
                type="checkbox"
                checked={bypass}
                onChange={(e) => setBypass(e.target.checked)}
                className="accent-turd-mustard-bright"
              />
              <span>⚡ Force (no admin online)</span>
            </label>
          )}
          <span className="text-[10px] text-turd-cream-dim/80">
            {route === 'bridge'
              ? bypass
                ? 'TurdCOM + Force: command dispatched via the bridge with the auth gate bypassed — runs with ZERO admins online (needs ≥1 player on as executor).'
                : 'TurdCOM mode: each command is sent as #verb in admin chat via the bridge. Without Force, the executor channel must already be admin.'
              : "RCON mode: each command goes through SCUM's Source-RCON port. Needs scum.RconEnabled / Port / Password in ServerSettings.ini."}
          </span>
        </div>
      </section>

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-5">
        {CATEGORY_ORDER.map((cat) => {
          const meta = CATEGORY_META[cat];
          const count = grouped[cat]?.length ?? 0;
          const isActive = activeCat === cat;
          return (
            <button
              key={cat}
              onClick={() => setActiveCat(cat)}
              className={`flex flex-col items-start gap-1 rounded border p-2 text-left transition-colors ${
                isActive
                  ? 'border-turd-mustard-bright bg-turd-bg-soft'
                  : 'border-turd-bronze/30 bg-turd-bg-mid/30 hover:border-turd-bronze/60'
              }`}
            >
              <span className="text-base leading-none">{meta.icon}</span>
              <span className={`text-[11px] font-medium ${meta.tone}`}>
                {meta.label}
              </span>
              <span className="text-[10px] text-turd-cream-dim">{count} verbs</span>
            </button>
          );
        })}
      </div>

      <section className="rounded glass p-3">
        <div className="mb-3 flex items-center gap-2">
          <span className="text-lg leading-none">
            {CATEGORY_META[activeCat].icon}
          </span>
          <h2 className={`font-display text-sm ${CATEGORY_META[activeCat].tone}`}>
            {CATEGORY_META[activeCat].label}
          </h2>
          <span className="text-[10px] text-turd-cream-dim">
            {filteredVerbs.length} of {grouped[activeCat]?.length ?? 0}
          </span>
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter…"
            className="ml-auto w-48 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-xs text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
          />
        </div>

        <div className="grid grid-cols-1 gap-2 md:grid-cols-2 xl:grid-cols-3">
          {filteredVerbs.map((c) => (
            <CommandCard
              key={c.verb}
              spec={c}
              serverId={serverId}
              disabled={route === 'rcon' && !serverId}
              route={route}
              players={playerNames}
              bypass={bypass}
            />
          ))}
        </div>
        {filteredVerbs.length === 0 && (
          <p className="px-2 py-3 text-xs text-turd-cream-dim">
            No commands match "{search}" in this category.
          </p>
        )}
      </section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// CommandCard — one verb's interactive cell. Renders the right input type
// for each curated hint, falls back to a single free-text args input when
// hints are missing, or to a single fire button when the verb is zero-arg.
// ---------------------------------------------------------------------------

interface CommandCardProps {
  spec: CommandSpec;
  serverId: string;
  disabled: boolean;
  route: RouteMode;
  players: string[];
  bypass: boolean;
}

function CommandCard({ spec, serverId, disabled, route, players, bypass }: CommandCardProps) {
  const hints = spec.hints ?? [];
  const [values, setValues] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {};
    for (const h of hints) initial[h.name] = h.defaultValue ?? '';
    return initial;
  });
  const [freeText, setFreeText] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const builtCommand = useMemo(() => {
    if (freeText.trim()) return freeText.trim().startsWith('#') ? freeText.trim() : `#${freeText.trim()}`;
    if (spec.zeroArg) return `#${spec.verb}`;
    if (hints.length > 0) {
      // Walk the hint values, dropping any trailing empties (optional
      // args at the tail). Keep interior empties to preserve positional
      // alignment — SCUM's parser is positional.
      const rawValues = hints.map((h) => values[h.name] ?? '');
      while (rawValues.length > 0 && rawValues[rawValues.length - 1] === '') {
        rawValues.pop();
      }
      const args = rawValues
        .map((v) => (v.includes(' ') && !v.startsWith('"') ? `"${v}"` : v))
        .join(' ');
      return args ? `#${spec.verb} ${args}` : `#${spec.verb}`;
    }
    return `#${spec.verb}`;
  }, [spec, hints, values, freeText]);

  const fire = async () => {
    setBusy(true);
    setResult(null);
    setErr(null);
    try {
      if (route === 'bridge') {
        // Dispatch the command via runAdminCommand → SCUM's real admin
        // parser (Chat_Server_ProcessAdminCommand on a PlayerRpcChannel).
        // We previously used broadcastChat with chatType=4, which only
        // renders a yellow chat line via MiscStatics::BroadcastChatLine —
        // it never invoked the admin parser, so commands appeared in
        // chat but never executed. The permission check still applies
        // (the channel's owning PC must be admin).
        // ProcessAdminCommand wants the BARE verb — a leading '#' yields
        // "Unrecognized command." (live-confirmed 2026-06-10).
        const params: Record<string, string> = {
          command: builtCommand.replace(/^#+\s*/, ''),
        };
        if (bypass) {
          // Force: dispatch on the game thread + flip the CanExecute auth gate
          // so it runs with NO admin account. Needs an online player as the
          // executor channel. gameThread/bypass MUST be string "1" — the bridge's
          // extract_json_str ignores JSON numbers (live-confirmed 2026-06-18).
          const executor = players[0];
          if (!executor) {
            throw new Error(
              'Force mode needs ≥1 player online (an executor channel). Have someone join, or turn off Force.',
            );
          }
          params.playerName = executor;
          params.gameThread = '1';
          params.bypass = '1';
        }
        const resp = await engineRpc<{
          ok?: boolean;
          error?: string;
          command?: string;
          gatePatched?: boolean;
          seh?: string;
        }>('runAdminCommand', params);
        if (resp?.error) throw new Error(resp.error);
        if (bypass) {
          if (resp?.gatePatched) {
            setResult(`dispatched (forced, as ${players[0]})`);
          } else {
            setResult(
              `dispatched, but auth gate NOT patched (seh ${resp?.seh ?? '?'}) — only ran if the executor is already admin; the gate signature may need re-deriving.`,
            );
          }
        } else {
          setResult(resp?.command ? `dispatched: ${resp.command}` : 'dispatched');
        }
      } else {
        if (!serverId) {
          setErr('No server profile selected.');
          return;
        }
        const resp = await invoke<string>('manager_server_rcon', {
          id: serverId,
          command: builtCommand,
        });
        setResult(resp);
      }
    } catch (e) {
      setErr(String((e as Error)?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2 rounded border border-turd-bronze/30 bg-turd-bg-deep/40 p-2">
      <div className="flex items-baseline gap-2">
        <code className="font-mono text-xs text-turd-mustard-bright">
          #{spec.verb}
        </code>
        {spec.category === 'destructive' && (
          <span className="text-[9px] text-red-400">⚠ destructive</span>
        )}
        {spec.zeroArg && (
          <span className="text-[9px] text-turd-cream-dim/70">no args</span>
        )}
      </div>

      {spec.description && (
        <p className="text-[10px] leading-snug text-turd-cream-dim">
          {spec.description}
        </p>
      )}

      {hints.length > 0 && (
        <div className="flex flex-col gap-1">
          {hints.map((h) => (
            <HintInput
              key={h.name}
              hint={h}
              value={values[h.name] ?? ''}
              onChange={(v) => setValues((p) => ({ ...p, [h.name]: v }))}
              players={players}
            />
          ))}
        </div>
      )}

      {!spec.zeroArg && hints.length === 0 && (
        <label className="flex flex-col gap-1">
          <span className="text-[9px] uppercase tracking-wider text-turd-cream-dim/60">
            args (free text)
          </span>
          <input
            type="text"
            value={freeText}
            onChange={(e) => setFreeText(e.target.value)}
            placeholder={`#${spec.verb} ...`}
            className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
          />
        </label>
      )}

      <div className="rounded bg-turd-bg-deep/60 px-2 py-1 font-mono text-[10px] text-turd-cream">
        {builtCommand}
      </div>

      <button
        onClick={fire}
        disabled={busy || disabled}
        className={`rounded px-2 py-1 text-[11px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
          spec.category === 'destructive'
            ? 'bg-red-700 text-white hover:bg-red-600'
            : 'bg-turd-mustard-bright text-turd-bg-deep hover:bg-turd-mustard'
        }`}
      >
        {busy
          ? 'Sending…'
          : spec.zeroArg
            ? 'Run'
            : route === 'bridge'
              ? 'Send via TurdCOM'
              : 'Send via RCON'}
      </button>

      {err && (
        <p className="rounded bg-red-900/30 px-2 py-1 font-mono text-[10px] text-red-300">
          {err}
        </p>
      )}
      {result !== null && (
        <pre className="max-h-32 overflow-auto rounded bg-turd-bg-deep/60 p-1.5 font-mono text-[10px] text-turd-cream">
          {result || '(empty response)'}
        </pre>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// HintInput — picks the right widget per ArgHint.kind. Falls back to a
// plain text input when kind is unspecified.
// ---------------------------------------------------------------------------

interface HintInputProps {
  hint: ArgHint;
  value: string;
  onChange: (v: string) => void;
  players: string[];
}

function HintInput({ hint, value, onChange, players }: HintInputProps) {
  const labelEl = (
    <span className="text-[10px] font-semibold tracking-wide text-turd-cream-dim/80">
      {formatLabel(hint.name)}
    </span>
  );

  if (hint.kind === 'bool') {
    const isOn = value === '1' || value.toLowerCase() === 'true';
    return (
      <label className="flex items-center justify-between gap-2">
        {labelEl}
        <button
          type="button"
          onClick={() => onChange(isOn ? '0' : '1')}
          className={`rounded px-2 py-0.5 text-[10px] font-medium ${
            isOn
              ? 'bg-turd-green/30 text-turd-green'
              : 'bg-turd-bg-soft text-turd-cream-dim'
          }`}
        >
          {isOn ? 'ON' : 'OFF'}
        </button>
      </label>
    );
  }

  if (hint.kind === 'enum' && hint.options) {
    return (
      <label className="flex flex-col gap-0.5">
        {labelEl}
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-[11px] text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
        >
          {hint.options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
    );
  }

  if (hint.kind === 'player') {
    // Roster-aware input. If the bridge returned any online players,
    // render a dropdown that lists them and offers a custom-text row.
    // Otherwise fall back to a free-text input with a datalist so the
    // user can still type a name and benefit from autocomplete once
    // the bridge becomes available.
    const isCustom = value !== '' && !players.includes(value);
    return (
      <label className="flex flex-col gap-0.5">
        <span className="flex items-baseline justify-between gap-2 text-[10px]">
          <span className="font-semibold tracking-wide text-turd-cream-dim/80">
            {formatLabel(hint.name)}
          </span>
          <span className="text-turd-cream-dim/50">
            {players.length} online
          </span>
        </span>
        <div className="flex gap-1">
          <select
            value={isCustom ? '__custom__' : value}
            onChange={(e) => {
              const v = e.target.value;
              if (v === '__custom__') {
                if (!isCustom) onChange('');
              } else {
                onChange(v);
              }
            }}
            className="flex-1 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-[11px] text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
          >
            <option value="">— pick a player —</option>
            {players.map((p) => (
              <option key={p} value={p}>{p}</option>
            ))}
            <option value="__custom__">✎ Type a name…</option>
          </select>
        </div>
        {(isCustom || players.length === 0) && (
          <input
            type="text"
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={players.length === 0 ? 'No roster yet — type the name' : 'Enter player name…'}
            className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
            list={`player-roster-${hint.name}`}
          />
        )}
        <datalist id={`player-roster-${hint.name}`}>
          {players.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
      </label>
    );
  }

  return (
    <label className="flex flex-col gap-0.5">
      {labelEl}
      <input
        type={hint.kind === 'number' ? 'text' : 'text'}
        inputMode={hint.kind === 'number' ? 'numeric' : 'text'}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={hint.hint}
        className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
      />
    </label>
  );
}

// ---------------------------------------------------------------------------
// RconStatusBadge — live indicator for the selected server. Pings RCON via
// manager_server_test (sends `Echo TurdMODProbe`) on mount + every 15s and
// renders a green / red / yellow dot with latency. Re-probes immediately on
// the Test button or whenever the selected server id changes.
// ---------------------------------------------------------------------------

interface RconStatusBadgeProps {
  serverId: string;
  isLocal: boolean;
}

function RconStatusBadge({ serverId, isLocal }: RconStatusBadgeProps) {
  const probe = useQuery({
    queryKey: ['admin-rcon-probe', serverId],
    queryFn: () => testServer(serverId),
    refetchInterval: 15000,
    refetchOnWindowFocus: true,
    retry: false,
  });

  // Decouple the manual-refresh button state from the 15s auto-poll so
  // the button doesn't flicker every poll cycle.
  const [manualRefreshing, setManualRefreshing] = useState(false);
  async function manualRefresh() {
    setManualRefreshing(true);
    try {
      await probe.refetch();
    } finally {
      setManualRefreshing(false);
    }
  }

  const data = probe.data;
  const isChecking = probe.isFetching;
  const ok = data?.rconOk ?? false;
  const errorRaw = probe.error
    ? String((probe.error as Error)?.message ?? probe.error)
    : data?.error ?? null;
  // Strip the "sftp: ..." prefix so we only show the RCON-relevant part
  // when the SFTP side also failed.
  const rconError = errorRaw
    ? errorRaw.includes('rcon:')
      ? errorRaw.slice(errorRaw.indexOf('rcon:'))
      : errorRaw
    : null;

  let dotColor = 'bg-turd-cream-dim';
  let labelColor = 'text-turd-cream-dim';
  let label = 'RCON unknown';

  if (isChecking && !data) {
    dotColor = 'bg-yellow-400 animate-pulse';
    labelColor = 'text-yellow-300';
    label = 'RCON checking…';
  } else if (ok) {
    dotColor = 'bg-green-400';
    labelColor = 'text-green-300';
    label = `RCON connected${data?.latencyMs != null ? ` · ${data.latencyMs}ms` : ''}`;
  } else if (data) {
    dotColor = 'bg-red-500';
    labelColor = 'text-red-300';
    label = 'RCON unreachable';
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1">
        <span className={`inline-block h-2 w-2 rounded-full ${dotColor}`} />
        <span className={`text-[11px] font-medium ${labelColor}`}>{label}</span>
        <button
          onClick={manualRefresh}
          disabled={manualRefreshing}
          className="ml-1 rounded border border-turd-bronze/30 px-1.5 py-0.5 text-[10px] text-turd-cream-dim hover:border-turd-bronze hover:text-turd-cream disabled:opacity-40"
          title="Re-test RCON now"
        >
          {manualRefreshing ? '…' : '↻'}
        </button>
      </div>
      {!ok && rconError && (
        <div className="max-w-md space-y-1">
          <p className="whitespace-pre-wrap break-words text-[10px] text-red-300/90">
            {rconError}
          </p>
          {(() => {
            const hint = rconErrorHint(rconError, isLocal);
            return hint ? (
              <p className="rounded border border-yellow-700/40 bg-yellow-900/20 px-2 py-1 text-[10px] text-yellow-200/90">
                💡 {hint}
              </p>
            ) : null;
          })()}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// BridgeStatusBadge — mirrors the RCON badge for the bridge route. Uses
// engine status for the lifecycle state (stopped / starting / running /
// crashed) and the getOnlinePlayers poll already running on the page as
// the actual pipe-responsiveness signal.
// ---------------------------------------------------------------------------

interface BridgeStatusBadgeProps {
  playerCount: number | undefined;
  isProbing: boolean;
  error: string | null;
  onRefetch: () => void;
}

function BridgeStatusBadge({ playerCount, isProbing, error, onRefetch }: BridgeStatusBadgeProps) {
  const engine = useEngineStatus();
  const engineKind = engine.data?.kind ?? 'stopped';

  let dot = 'bg-turd-cream-dim';
  let labelColor = 'text-turd-cream-dim';
  let label = 'TurdCOM unknown';
  let hint: string | null = null;

  if (engineKind === 'stopped') {
    dot = 'bg-red-500';
    labelColor = 'text-red-300';
    label = 'TurdCOM offline';
    hint = 'Engine is stopped. Click Start on the Engine page to launch the cppmod, then come back.';
  } else if (engineKind === 'starting') {
    dot = 'bg-yellow-400 animate-pulse';
    labelColor = 'text-yellow-300';
    label = 'TurdCOM starting…';
  } else if (engineKind === 'crashed') {
    dot = 'bg-red-500';
    labelColor = 'text-red-300';
    label = 'TurdCOM crashed';
    hint = 'The engine crashed. Check the Console page for the last lines, then restart from the Engine page.';
  } else if (engineKind === 'running') {
    if (isProbing) {
      dot = 'bg-yellow-400 animate-pulse';
      labelColor = 'text-yellow-300';
      label = 'TurdCOM checking…';
    } else if (error) {
      dot = 'bg-red-500';
      labelColor = 'text-red-300';
      label = 'TurdCOM unreachable';
      hint = 'Engine is running but the pipe is not responding. SCUM may have just launched or the cppmod failed to load.';
    } else {
      dot = 'bg-green-400';
      labelColor = 'text-green-300';
      label = `TurdCOM connected${playerCount != null ? ` · ${playerCount} online` : ''}`;
    }
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1">
        <span className={`inline-block h-2 w-2 rounded-full ${dot}`} />
        <span className={`text-[11px] font-medium ${labelColor}`}>{label}</span>
        <button
          onClick={onRefetch}
          disabled={isProbing}
          className="ml-1 rounded border border-turd-bronze/30 px-1.5 py-0.5 text-[10px] text-turd-cream-dim hover:border-turd-bronze hover:text-turd-cream disabled:opacity-40"
          title="Re-test bridge now"
        >
          {isProbing ? '…' : '↻'}
        </button>
      </div>
      {hint && (
        <p className="max-w-md rounded border border-yellow-700/40 bg-yellow-900/20 px-2 py-1 text-[10px] text-yellow-200/90">
          💡 {hint}
        </p>
      )}
      {error && engineKind === 'running' && (
        <p className="max-w-md whitespace-pre-wrap break-words text-[10px] text-red-300/90">
          {error}
        </p>
      )}
    </div>
  );
}
