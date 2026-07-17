// SCUM AdminUsers.ini permission tokens, grouped for the editor UI.
// These are the exact tokens that go inside the brackets of an AdminUsers.ini
// line: `<steamid>[Token1,Token2,...]`. The full list below is the owner/
// super-admin set (everything SCUM exposes as a per-command grant). Source:
// the live OVH AdminUsers.ini owner entry (YOUR_OWNER_NAME/Zilla), 2026-06-16.
// @inv tokens are CASE- and SPELLING-sensitive — they must match SCUM exactly.

export interface PermGroup {
  label: string;
  perms: string[];
}

export const PERMISSION_GROUPS: PermGroup[] = [
  {
    label: 'God / Cheats',
    perms: [
      'SetGodMode',
      'SetImmortality',
      'SetInfiniteStamina',
      'SetInfiniteAmmo',
      'SetInfiniteOxygen',
      'SetSuperJump',
    ],
  },
  {
    label: 'Player / Prisoner State',
    perms: [
      'SetStamina',
      'SetExhaustion',
      'SetAttributes',
      'SetSkillLevel',
      'SetBodyType',
      'SetGender',
      'Knockout',
      'Sleep',
      'SetBladderVolume',
      'SetStomachVolume',
      'SetHealthToItemInHands',
      'AddBodyEffect',
      'RemoveBodyEffect',
      'DisableBodyEffects',
      'SkipDiseaseIncubationStage',
      'SetMetabolismSimulationSpeed',
    ],
  },
  {
    label: 'Inventory',
    perms: ['SetAllInventoryAccess', 'Inventory', 'EquipParachute', 'SpawnInventoryFullOf'],
  },
  {
    label: 'Teleport',
    perms: ['Teleport', 'TeleportTo', 'TeleportToMe', 'TeleportToVehicle', 'MapTeleport'],
  },
  {
    label: 'Spawning',
    perms: [
      'SpawnItem',
      'SpawnItem2',
      'SpawnVehicle',
      'SpawnAnimal',
      'SpawnRandomAnimal',
      'SpawnZombie',
      'SpawnRandomZombie',
      'SpawnArmedNPC',
      'SpawnAllItems',
      'SpawnRazor',
      'CreateEntity',
    ],
  },
  {
    label: 'Economy / Fame',
    perms: [
      'SetCurrencyBalance',
      'ChangeCurrencyBalance',
      'SetCurrencyBalanceToAll',
      'SetFamePoints',
      'ChangeFamePoints',
      'SetFamePointsToAll',
    ],
  },
  {
    label: 'World / Time / Weather',
    perms: [
      'SetTime',
      'SetTimeSpeed',
      'SetWeatherControllerOverrideActive',
      'SetWeatherControllerOverrideValue',
      'SetDecayTimeDilation',
      'SetFarmingSimulationSpeed',
      'ScheduleCargoDrop',
      'ScheduleWorldEvent',
    ],
  },
  {
    label: 'Moderation',
    perms: ['Kick', 'Ban', 'Unban', 'Mute', 'Unmute', 'Silence', 'Unsilence'],
  },
  {
    label: 'Info / Inspect',
    perms: [
      'ListPlayers',
      'PlayerInfo',
      'Location',
      'GetSteamID',
      'ShowNameplates',
      'ListSquads',
      'ListVehicles',
      'ListItems',
      'SquadInfo',
      'Quests',
    ],
  },
  {
    label: 'Misc / Vehicles / Notify',
    perms: [
      'Announce',
      'SendNotification',
      'Loot',
      'GrantElevatedStatus',
      'RenameVehicle',
      'VehicleCheat',
      'SetMountedVehicleProperty',
      'SetAirplaneMaxVelocity',
    ],
  },
];

// Flat list of every permission token (the full owner set).
export const ALL_PERMISSIONS: string[] = PERMISSION_GROUPS.flatMap((g) => g.perms);
