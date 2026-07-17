// Light metadata schema for the Server Settings editor.
//
// The page renders every key from the loaded ServerSettings.ini — this
// schema only ENRICHES presentation (better label, description, min/max,
// dropdown options) for the keys most admins actually edit. Anything not
// in KEY_META falls back to humaniseKey + type inference from value.

export interface KeyMeta {
  label?: string;
  description?: string;
  /** Practical guidance — typical/recommended values, edge cases. */
  howToUse?: string;
  /** Why you'd change this — positive framing. */
  benefit?: string;
  /** Trade-offs — performance, gameplay, irreversibility. */
  sideEffect?: string;
  min?: number;
  max?: number;
  options?: Array<{ value: string; label: string }>;
}

export const KEY_META: Record<string, KeyMeta> = {
  // ── General — Server identity ────────────────────────────────────────────
  '[General].scum.ServerName': {
    label: 'Server Name',
    description: 'Display name shown in the server browser.',
    howToUse:
      'Keep it under ~60 characters so it fits in the browser list. Include your community brand and a hint of playstyle (e.g. "TurdMOD Official — PvE/PvP Zones").',
    benefit:
      "It's the FIRST thing players see. A clear, memorable name brings more clicks than a generic one.",
    sideEffect:
      "Avoid pipes (|), forward-slashes-pair (//), or semicolons — SCUM's INI parser truncates values at those characters and your name will silently lose the tail.",
  },
  '[General].scum.ServerDescription': {
    label: 'Server Description',
    description: 'Short tagline shown in the server browser.',
    howToUse:
      'List your top selling points in 1–2 sentences: active mods, wipe schedule, KOS rules, Discord link.',
    benefit:
      'Players filter by description keywords ("PvE", "noob friendly", "active admins") — a good one converts browser-scrollers into joiners.',
    sideEffect:
      'Same parser quirk as Server Name — avoid `|`, `//`, `;` in the value.',
  },
  '[General].scum.ServerPassword': {
    label: 'Server Password',
    description: 'Password required to connect. Empty = public server.',
    howToUse:
      'Leave blank for a public community server. Set a password for private/whitelisted servers or for closed-beta testing periods.',
    benefit:
      'Hard gate against unwanted players. Combined with `AdminUsers.ini` you can run a tightly-controlled private server.',
    sideEffect:
      "Players can't preview your server in the browser without it. Lose discoverability, gain control.",
  },
  '[General].scum.MaxPlayers': {
    label: 'Max Players',
    description: 'Maximum concurrent player slots on your server.',
    howToUse:
      'Community servers usually run 32–64. Going above 64 needs a beefy box (16+ CPU cores, 32GB+ RAM, fast SSD). Below 16 feels empty.',
    benefit:
      'Higher cap = larger community feel, more PvP encounters, more bases to discover.',
    sideEffect:
      'Each player adds CPU + bandwidth + memory. Exceeding what your hardware can handle causes server stutter, rubber-banding, and disconnects — worse than a smaller, smooth server.',
    min: 1,
    max: 100,
  },
  '[General].scum.ServerPlaystyle': {
    label: 'Server Playstyle',
    description: 'Advertised playstyle category shown in the browser filter.',
    howToUse:
      'Set to whichever playstyle dominates your server. PvE servers usually pair this with `HumanToHumanDamageMultiplier=0` to enforce it.',
    benefit:
      'Browser filter — players actively filter by playstyle, so matching this to your actual ruleset brings the RIGHT players.',
    sideEffect:
      "Tag-only — it doesn't enforce any rules by itself. You still have to configure damage multipliers, raid protection, and zones to match what the tag promises.",
    options: [
      { value: 'PVP', label: 'PVP' },
      { value: 'PVE', label: 'PVE' },
      { value: 'Mixed', label: 'Mixed' },
    ],
  },

  // ── General — Welcome + MOTD ─────────────────────────────────────────────
  '[General].scum.WelcomeMessage': {
    label: 'Welcome Message',
    description: 'Shown to players on first connect.',
    howToUse:
      "Use `\\n` for line breaks. List your top server rules + Discord + key commands. Keep it skimmable — players hit Esc fast.",
    benefit:
      "First impression that scales. Sets expectations so you don't have to repeat them in chat.",
    sideEffect:
      'Avoid `|`, `//`, `;` — INI parser truncation again. URLs need to be wrapped in quotes (`"https://..."`) or URL-encoded.',
  },
  '[General].scum.MessageOfTheDay': {
    label: 'Message of the Day',
    description: 'Broadcast to all players periodically (see MOTD Cooldown).',
    howToUse:
      'Use for current events, double-XP weekends, planned restart times. Update it weekly to keep it fresh.',
    benefit:
      'Reaches active players without spamming Discord. Auto-rotates without your input.',
    sideEffect:
      'Repeats on the same cooldown forever — if you forget to update it, players see stale info and start tuning out.',
  },
  '[General].scum.MessageOfTheDayCooldown': {
    label: 'MOTD Cooldown (minutes)',
    description: 'How often the MOTD is broadcast, in minutes.',
    howToUse:
      'Default 15 is too frequent for most. 30–60 strikes a balance: visible without spamming.',
    benefit:
      'Predictable rhythm. Players know "the announcement comes around every 30 minutes".',
    sideEffect:
      'Min 10, max 720. Too short = spammy and players mute it. Too long = many players never see it.',
    min: 10,
    max: 720,
  },

  // ── General — Performance ────────────────────────────────────────────────
  '[General].scum.MinServerTickRate': {
    label: 'Min Server Tick Rate',
    description: 'Minimum simulation tick rate the server tries to maintain.',
    howToUse:
      'Default 5 is fine for most servers. Raise toward 15–20 only if you have spare CPU AND you notice combat feeling laggy under load.',
    benefit:
      'Higher floor = combat feels more responsive even under heavy load.',
    sideEffect:
      'Raising it eats CPU. Combined with high player counts on under-spec hardware = server stutter.',
    min: 1,
    max: 60,
  },
  '[General].scum.MaxServerTickRate': {
    label: 'Max Server Tick Rate',
    description: 'Maximum tick rate the server will ever run at.',
    howToUse:
      'Default 30 is the SCUM-supported sweet spot. Going higher rarely helps because the game logic is locked to a ~30Hz simulation.',
    benefit:
      'Caps CPU use during low-population periods (server idles cheaper).',
    sideEffect:
      'Lowering this below ~20 makes combat feel laggy. Leave at 30 unless you really need the savings.',
    min: 1,
    max: 60,
  },
  '[General].scum.MaxPing': {
    label: 'Max Ping (ms)',
    description: 'Players whose ping exceeds this value are kicked.',
    howToUse:
      "200 is the SCUM default and works for most. North-American-only servers can set 150. Global servers should set 300+ so far-away players don't get kicked every join.",
    benefit:
      'Keeps competitive combat fair — laggy players are harder to hit and have advantage in PvP.',
    sideEffect:
      "Too aggressive = legitimate players get kicked on bad network days. Make sure `MaxPingCheckEnabled=True` first or this setting does nothing.",
    min: 50,
    max: 2000,
  },
  '[General].scum.LogoutTimer': {
    label: 'Logout Timer (seconds)',
    description: 'How long a player must stand still before logging out safely.',
    howToUse:
      'Default 60 punishes combat loggers. PvE servers can lower to 30. Hard PvP servers sometimes go 120+.',
    benefit:
      "Stops 'combat logging' — players quitting mid-fight to avoid death.",
    sideEffect:
      'Higher value = players who really need to log off get stuck. Lower value = combat logging becomes viable.',
    min: 0,
    max: 300,
  },

  // ── General — Permissions ────────────────────────────────────────────────
  '[General].scum.AllowFirstPerson': {
    label: 'Allow First Person',
    description: 'Allow players to use first-person view.',
    howToUse:
      "Almost always True. Some hardcore-realism servers set this False + AllowThirdPerson=False to force a specific view.",
    benefit:
      "Player choice. Most players have a strong preference for one view or the other.",
    sideEffect:
      "If both this AND AllowThirdPerson are False, players can't see at all — set at least one to True.",
  },
  '[General].scum.AllowThirdPerson': {
    label: 'Allow Third Person',
    description: 'Allow players to use third-person view.',
    howToUse:
      'Set to False for hardcore servers — first-person-only is a popular niche. Otherwise leave True.',
    benefit:
      'Third person is more accessible to casual players (easier orientation, less motion sickness).',
    sideEffect:
      'Some PvP players consider third-person "peek-around-corners" exploit-y. Disabling it tightens the PvP meta.',
  },
  '[General].scum.FameGainMultiplier': {
    label: 'Fame Gain Multiplier',
    description: 'Multiplier applied to all fame point gains.',
    howToUse:
      "Default 1.0 = vanilla. Boost servers run 2.0–5.0 to make leveling fast. Hardcore servers stay at 1.0 or lower.",
    benefit:
      'Lets new players catch up to long-running characters faster. Reduces grind.',
    sideEffect:
      "Too high (5.0+) trivializes progression — players hit max fame in a few sessions and lose long-term goals.",
    min: 0,
  },

  // ── General — Wipes (destructive!) ───────────────────────────────────────
  '[General].scum.PartialWipe': {
    label: 'Partial Wipe',
    description: 'Trigger a partial wipe on next server restart.',
    howToUse:
      "Set True, restart the server, then set back to False. Wipes character inventories + bases but preserves character creation data.",
    benefit:
      'Periodic wipes refresh the world and bring players back. Common cadence: monthly or quarterly.',
    sideEffect:
      "IRREVERSIBLE on restart. Always announce wipes to your community in advance — surprise wipes lose you players permanently. Don't forget to set back to False after the wipe.",
  },
  '[General].scum.GoldWipe': {
    label: 'Gold Wipe',
    description: 'Wipe all currency/gold on next server restart.',
    howToUse:
      'Use to reset trader economy without wiping bases. Set True → restart → set back to False.',
    benefit:
      'Gold accumulates and breaks economy over time. A periodic gold wipe restores price tension.',
    sideEffect:
      "Players lose ALL gold including bank deposits. Pair with `EconomyOverride.json` resets if you've heavily customized trader prices.",
  },
  '[General].scum.FullWipe': {
    label: 'Full Wipe',
    description: 'Full world wipe on next server restart.',
    howToUse:
      'Nuclear option — wipes EVERYTHING (characters, bases, gold, vehicles, trader stock). Set True → restart → set back to False.',
    benefit:
      'Total reset. Use for the start of a new server season or after a catastrophic exploit/bug.',
    sideEffect:
      "TRULY irreversible. Announce DAYS in advance with a Discord countdown, MOTD, and in-game notification. Surprise full-wipes are how communities die.",
  },

  // ── World ─────────────────────────────────────────────────────────────────
  '[World].scum.MaxAllowedDrones': {
    label: 'Max Allowed Drones',
    description: 'Maximum number of player drones active in the world at once.',
    howToUse:
      'Default 0 disables drones entirely (most servers). Raise to 5–20 to allow drone gameplay.',
    benefit:
      'Drones add a tech/scout layer to PvP. Recon before raiding, base surveillance, etc.',
    sideEffect:
      'Active drones cost CPU. Heavy abuse (everyone running drones) tanks server performance.',
    min: 0,
  },
  '[World].scum.PuppetHealthMultiplier': {
    label: 'Puppet (Zombie) Health Multiplier',
    description: 'Multiplier applied to zombie/puppet health pool.',
    howToUse:
      'Default 1.0 is vanilla. PvE/hardcore servers go 1.5–3.0 to make zombies a real threat. PvP servers often drop to 0.5–0.8 so zombies don\'t interfere with player fights.',
    benefit:
      "Tunes how much of a threat zombies are. Higher = better PvE feel; lower = zombies feel like 'background noise'.",
    sideEffect:
      "Higher health means more ammo per kill, which makes ammo scarce and the economy more brutal. Tune `ZombieDamageMultiplier` together for a balanced feel.",
    min: 0,
  },
  '[World].scum.ArmedNPCDifficultyLevel': {
    label: 'Armed NPC Difficulty',
    description: 'Difficulty tier for armed NPC encounters (mechs, bandits).',
    howToUse:
      'Default 2 is "Medium". 0 = trivial, 4 = brutal. Pair with the ArmedNPC*Multiplier keys for fine control.',
    benefit:
      "Highest-quality loot is usually behind armed-NPC encounters. Tuning this controls the 'gear gating' for your server.",
    sideEffect:
      'Higher difficulty needs higher player skill and gear — new players hit a wall. Lower difficulty trivializes the high-tier loot route.',
    min: 0,
  },
  '[World].scum.StartTimeOfDay': {
    label: 'Start Time of Day',
    description: 'In-game time-of-day when the server first starts (HH:MM:SS).',
    howToUse:
      'Default 08:00:00 = morning start. Set to 20:00:00 for a night-start "horror" feel.',
    benefit:
      'Sets the opening mood. Morning start is friendly; night start signals a hardcore atmosphere.',
    sideEffect:
      "Only used at the very first server launch. After that, time advances by `TimeOfDaySpeed` and persists across restarts.",
  },
  '[World].scum.TimeOfDaySpeed': {
    label: 'Time of Day Speed',
    description: 'How fast in-game time passes. 1.0 = real-time.',
    howToUse:
      'Default 3.84 means a SCUM day passes in about 6.25 real hours. Faster (5.0+) for busier day/night cycles. Slower (2.0) for more time per phase.',
    benefit:
      'Faster cycles give more variety per play session. Players see day, night, dawn, dusk in one sitting.',
    sideEffect:
      'Very fast cycles (10.0+) make planting/cooking/long activities annoying. Very slow cycles can lock a session into the same time-of-day forever.',
    min: 0,
  },
  '[World].scum.NighttimeDarkness': {
    label: 'Nighttime Darkness',
    description: 'How dark the world gets at night. 0 = vanilla, higher = darker.',
    howToUse:
      'Default 0 = vanilla SCUM nights (visibility is fine). Increase to 0.3–0.6 for atmosphere, beyond 0.7 = pitch black.',
    benefit:
      'Darker nights make flashlights, NVGs, and base lighting valuable — adds gameplay layers.',
    sideEffect:
      'Too dark and players just log out at night instead of playing. Combat in pitch-black favors NVG haves over have-nots.',
    min: 0,
  },
  '[World].scum.EnableFog': {
    label: 'Enable Fog',
    description: 'Toggle atmospheric fog throughout the world.',
    howToUse:
      'Default True is more atmospheric. Disable for clearest sightlines (competitive PvP servers sometimes prefer this).',
    benefit:
      'Fog adds atmosphere and increases tension in scouting/combat.',
    sideEffect:
      'Fog can hide players in tall grass, which favors stealth/snipers over close-quarters players. Turning it off levels the playing field but loses the SCUM "look".',
  },
  '[World].scum.CargoDropCooldownMinimum': {
    label: 'Cargo Drop Cooldown Min (minutes)',
    description: 'Minimum time between cargo drop events.',
    howToUse:
      'Default 90 + Max 120 = drops every 1.5–2 hours. Active servers can drop to 30/60 for more drops. PvE servers can extend to 180/240.',
    benefit:
      'Cargo drops are scheduled PvP events that draw players to one spot — great for getting people fighting.',
    sideEffect:
      'Too frequent (30 min) and they become routine, no excitement. Too rare (4 hours+) and only the hardcore catch them.',
    min: 0,
  },
  '[World].scum.CargoDropCooldownMaximum': {
    label: 'Cargo Drop Cooldown Max (minutes)',
    description: 'Maximum time between cargo drop events.',
    howToUse:
      'Set higher than the Minimum value. The actual cooldown is randomized between Min and Max.',
    benefit:
      'Randomized timing keeps drops unpredictable — players have to actually scan the sky / map, not just set a timer.',
    sideEffect:
      'Wide gap between Min and Max (e.g. 30→240) creates inconsistent server pacing — feast or famine.',
    min: 0,
  },

  // ── Damage ────────────────────────────────────────────────────────────────
  '[Damage].scum.HumanToHumanDamageMultiplier': {
    label: 'PVP Damage Multiplier',
    description: 'Multiplier for player-to-player damage (all weapons).',
    howToUse:
      'Default 1.0 = vanilla. PvE servers set 0 to completely disable PvP. Hardcore PvP servers can run 2.0–3.0 for "everyone dies in one shot" feel.',
    benefit:
      "Single biggest lever for tuning PvP intensity. 0 = pure PvE. 3.0+ = full hardcore.",
    sideEffect:
      "Setting to 0 doesn't stop other forms of grief (vehicle ramming, base damage). For a true PvE server combine with `MinesAndTraps=False` + raid protection.",
    min: 0,
  },
  '[Damage].scum.ZombieDamageMultiplier': {
    label: 'Zombie Damage Multiplier',
    description: 'Multiplier for damage dealt by zombies/puppets to players.',
    howToUse:
      'Default 1.0 is vanilla. Hardcore servers run 1.5–2.0. Casual/PvE-focused servers sometimes drop to 0.5 so PvE feels less brutal.',
    benefit:
      'Tunes the PvE threat level. Pair with PuppetHealthMultiplier for full control over zombie difficulty.',
    sideEffect:
      'Very high values + high zombie density = "you cannot leave town without dying", which discourages exploration.',
    min: 0,
  },
  '[Damage].scum.ItemDecayDamageMultiplier': {
    label: 'Item Decay Multiplier',
    description: 'Multiplier for how fast items decay over time.',
    howToUse:
      "Default 1.0 = vanilla. PvE servers often go 0.2–0.5 (gear lasts longer, less grind). Hardcore wipe servers stay at 1.0 or push to 1.5.",
    benefit:
      "Lower decay = less time spent on maintenance, more time on play. Higher decay = stronger economy because gear is consumable.",
    sideEffect:
      'Setting to 0 makes items last forever — completely breaks the trader economy because nobody needs to buy replacements.',
    min: 0,
  },

  // ── Features — Raid protection ───────────────────────────────────────────
  '[Features].scum.RaidProtectionType': {
    label: 'Raid Protection Type',
    description: 'When and how raid protection (block base damage) activates.',
    howToUse:
      "0 = no protection (vanilla). 1 = global schedule (uses `RaidTimes.json`). 2 = per-flag (each player's flag has its own protection window).",
    benefit:
      'Prevents offline raiding. Most community servers run Type 1 with scheduled raid windows (e.g. weekends only).',
    sideEffect:
      "Type 2 (per-flag) requires more admin support — players need to be educated on how to set their flag's protection. Type 0 means anyone can raid anytime, which kills casual playerbases.",
    options: [
      { value: '0', label: '0 — Off (raid anytime)' },
      { value: '1', label: '1 — Global schedule (uses RaidTimes.json)' },
      { value: '2', label: '2 — Per-flag (player-controlled)' },
    ],
  },
  '[Features].scum.QuestsEnabled': {
    label: 'Quests Enabled',
    description: 'Enable or disable the entire quest/trader-mission system.',
    howToUse:
      'Default True. Only disable for super-hardcore / "no handholding" servers.',
    benefit:
      'Quests are the main onboarding path for new players. Disabling them removes a major progression loop.',
    sideEffect:
      'Disabling kills a huge chunk of content. Trader economy still works but the gameplay loop becomes pure scavenge/raid.',
  },
  '[Features].scum.EnableNewPlayerProtection': {
    label: 'New Player Protection',
    description: 'Give new players a grace period before they can be PvP-attacked.',
    howToUse:
      'Default True with 120-minute duration (see below). Almost always leave True — new-player griefing is the #1 retention killer.',
    benefit:
      'Lets new players learn the game without being instantly killed by veterans. Massive retention boost.',
    sideEffect:
      'Some veterans exploit the protection (run errands as a "noob alt"). Tune the duration carefully to balance.',
  },
  '[Features].scum.NewPlayerProtectionDuration': {
    label: 'New Player Protection Duration (in-game hours)',
    description: 'How long new-player protection lasts in real-time hours.',
    howToUse:
      'Default 120 hours of in-game protection. Realistic settings are 24–72 — long enough to learn, short enough that vets can\'t exploit.',
    benefit:
      'Calibrates the "training wheels" period. Shorter = vets feel safer; longer = newcomers feel safer.',
    sideEffect:
      "Counted in real-time hours of logged-in protection, not calendar hours. A player who plays 2h/day takes a long time to lose 120 hours of protection.",
  },
};

// scum.MaxAllowedPuppets       → "Max Allowed Puppets"
// scum.BCUTerminalCooldown     → "BCU Terminal Cooldown"
// scum.DoorLockability.Garage  → "Door Lockability.Garage" (dots stay)
export function humaniseKey(raw: string): string {
  const stripped = raw.startsWith('scum.') ? raw.slice(5) : raw;
  return stripped.replace(/([a-z\d])([A-Z])/g, '$1 $2');
}
