# TurdMOD Module Catalog

Complete reference for all 61 server mod modules. Every command, every system, every feature.

## Player Commands

Commands available to all players unless marked (admin).

### Economy
| Command | Description |
|---------|-------------|
| `!balance` | Check your coin balance |
| `!daily` | Claim daily 100-coin bonus (24hr cooldown) |
| `!pay <player> <amount>` | Send coins to another player |
| `!top` | Richest players leaderboard |
| `!bank` | Check wallet + bank balances |
| `!deposit <amount>` | Move coins to bank (earns 1%/hr interest) |
| `!withdraw <amount>` | Take coins out of bank |

### Combat & PvP
| Command | Description |
|---------|-------------|
| `!kd` | Your kill/death ratio and streaks |
| `!leaderboard` | Top 5 killers |
| `!topkills` | Same as !leaderboard |
| `!topdeaths` | Most-died players |
| `!duel <player>` | Challenge someone to 1v1 |
| `!accept` | Accept a duel challenge |
| `!decline` | Decline a duel challenge |
| `!duelstats` | Your duel win/loss record |
| `!bounties` | View active bounties |
| `!bounty <player> <amount>` | Place a bounty (costs coins) |
| `!hunt` | Toggle bounty hunter mode (proximity alerts) |
| `!targets` | List active bounties with details |

### Social
| Command | Description |
|---------|-------------|
| `!clan create <name>` | Found a new clan |
| `!clan invite <player>` | Invite player (leader only) |
| `!clan accept` | Accept clan invitation |
| `!clan leave` | Leave your clan |
| `!clan info [name]` | Clan details and members |
| `!clan war <clan>` | Declare war (leader only) |
| `!clan peace <clan>` | End war (leader only) |
| `!clan list` | All clans on server |
| `!rep` | Check your reputation |
| `!rep <player>` | Check someone's reputation |
| `!toprep` | Reputation leaderboard |
| `!thank <player>` | Give rep to someone (+3) |
| `!trade <player> <amount>` | Offer coins for trade |
| `!reject` | Reject a trade offer |

### Gambling
| Command | Description |
|---------|-------------|
| `!coinflip [amount]` | 50/50 coin flip (default 10c) |
| `!dice [amount]` | Roll vs house (default 10c) |
| `!slots [amount]` | Slot machine (min 5c, 3-match = 5x, 7-match = 10x) |
| `!lottery buy` | Buy lottery ticket (50c, max 5/round) |
| `!lottery status` | Current pot, time to draw |

### Companions & Taming
| Command | Description |
|---------|-------------|
| `!tame` | Tame nearest animal (makes it passive) |
| `!companion` | Check your active companion |
| `!dismiss` | Release your companion |

### Vehicles
| Command | Description |
|---------|-------------|
| `!register <type>` | Register a vehicle to your garage (max 3) |
| `!garage` | View your registered vehicles |
| `!spawn <type>` | Spawn a registered vehicle near you |
| `!park` | Despawn your active vehicle |
| `!unregister <type>` | Remove from garage |
| `!insure <type>` | Insure a registered vehicle |
| `!vehicles` | List available vehicle types |

### Teleport
| Command | Description |
|---------|-------------|
| `!tp <name>` | Teleport to a named point |
| `!points` | List all teleport points |

### Quests & Achievements
| Command | Description |
|---------|-------------|
| `!quest` | View your active quests |
| `!quests` | Same as !quest |
| `!claim` | Claim completed quest rewards |
| `!achievements` | View achievement progress |
| `!trophy` | Same as !achievements |
| `!streak` | Daily login streak info |

### Stats & Profile
| Command | Description |
|---------|-------------|
| `!mystats` | Playtime, distance, sessions |
| `!topplayed` | Most hours played |
| `!toptraveled` | Most distance traveled |
| `!title list` | View earned titles |
| `!title set <id>` | Set active chat title |
| `!title clear` | Remove active title |

### Utility
| Command | Description |
|---------|-------------|
| `!help` | Command overview |
| `!rules` | Server rules |
| `!motd` | Message of the day |
| `!report <player> <reason>` | Report a player |
| `!players` | Online player list |
| `!server` | Server status |
| `!ask <question>` | Ask the AI assistant |
| `!care` | Emergency care package (bandage + water + berries) |

### NPCs (DIMs)
| Command | Description |
|---------|-------------|
| `!ziggy <message>` | Talk to Ziggy (arms dealer) |
| `!doc <message>` | Talk to Doc Vera (medic) |
| `!rust <message>` | Talk to Rust (mechanic) |

### Racing
| Command | Description |
|---------|-------------|
| `!race start <name>` | Start a defined race |
| `!race list` | List available races |

### Radio
| Command | Description |
|---------|-------------|
| `!radio on` | Turn on server radio |
| `!radio off` | Turn off radio |
| `!radio skip` | Skip current track |
| `!radio queue <name>` | Add to queue |
| `!radio np` | Now playing |

### Voting
| Command | Description |
|---------|-------------|
| `!vote day/night/storm/clear/restart` | Start a vote |
| `!yes` | Vote yes |
| `!no` | Vote no |

---

## Admin Commands

Require owner Steam ID (YOUR_STEAM_ID_1) or name match.

### Server Control
| Command | Description |
|---------|-------------|
| `!day` | Set time to noon |
| `!night` | Set time to midnight |
| `!weather <0..1>` | Set weather severity |
| `!storm` | Max weather |
| `!clear` | Clear skies |
| `!fly [off]` | Toggle flying mode |
| `!possess <class>` | Possess any pawn (Dropship, Sentry2) |
| `!unpossess` | Return to your body |
| `!tp <player>` | Teleport player to you |
| `!spawn <class>` | Spawn entity near you |
| `!stats` | Server performance stats |
| `!setpoint <name> <x> <y> <z>` | Create teleport point |
| `!delpoint <name>` | Delete teleport point |

### Player Management
| Command | Description |
|---------|-------------|
| `!god` | Toggle persistent god mode |
| `!hulk` | Toggle hulk leap mode |
| `!jump` | Hulk leap (when hulk mode on) |
| `!heal [player]` | Heal a player |
| `!feed [player]` | Give food items |
| `!cure [player]` | Give medical items |
| `!xp <player> <amount>` | Grant fame XP |
| `!money <player> <amount>` | Grant in-game currency |
| `!jail <player> [min] [reason]` | Jail a player |
| `!unjail <player>` | Release from jail |
| `!jailstatus` | List inmates |
| `!warn <player> [message]` | Send warning DM |
| `!reports` | View player reports |

### Events
| Command | Description |
|---------|-------------|
| `!warzone [name]` | Start PvP warzone event (10 min) |
| `!endwarzone` | End warzone early |
| `!wzstatus` | Warzone status |
| `!boss` | Trigger 5-wave zombie boss fight |
| `!horde <waves>` | Start horde survival (max 10 waves) |
| `!purge` | Pacify all zombies + animals |
| `!airdrop [player]` | Supply drop (random or targeted) |
| `!event create <name> <min>` | Schedule an event |
| `!event list` | Upcoming events |
| `!event cancel <name>` | Cancel event |

### Vehicles (Admin)
| Command | Description |
|---------|-------------|
| `!vspawn <type>` | Spawn any vehicle |
| `!vlist` | List all spawned vehicles |
| `!vbring <id>` | Teleport vehicle to you |
| `!vdestroy <id>` | Destroy a vehicle |

### Zones
| Command | Description |
|---------|-------------|
| `!addzone <name> <x> <y> <radius>` | Create safe zone |
| `!delzone <name>` | Delete safe zone |
| `!zones` | List safe zones |

### Raid Protection
| Command | Description |
|---------|-------------|
| `!raidstatus` | Current raid window |
| `!setraidtimes <start> <end>` | Set daily raid hours |
| `!raidoff` | Disable raiding |

### Racing (Admin)
| Command | Description |
|---------|-------------|
| `!race create <name>` | Start building a race |
| `!race checkpoint` | Add checkpoint at position |
| `!race finish` | Finalize race definition |

### Territory
| Command | Description |
|---------|-------------|
| `!territory claim [name]` | Claim zone for clan |
| `!territory list` | All territories |
| `!territory unclaim <name>` | Release territory |

---

## Background Systems (No Commands)

These modules run automatically with no player interaction.

| Module | What it does |
|--------|-------------|
| **FriendlyPuppets** | Zombies + animals passive by default (30s reapply loop) |
| **Welcome** | Auto-greets new players with !help hint |
| **Announcements** | Broadcasts join/leave/kill events |
| **Analytics** | JSONL position + event logger (60s interval) |
| **Scoreboard** | Tracks playtime + distance per player |
| **Scheduler** | 6-hour auto-restart with countdown warnings |
| **Weather Cycle** | Rotating clear→building→storm→clearing pattern |
| **Weather Alerts** | "Storm approaching" / "Skies clearing" broadcasts |
| **Supply Drops** | Random loot drop every 45 min with grid location |
| **Spawn Protection** | 30s god mode on login |
| **Spawn Loadout** | Auto-give starter/veteran items on login |
| **Login Streak** | Escalating daily rewards (25c base, +25c/day, 250c weekly) |
| **Death Recap** | Sends killer info to victim after death |
| **Bounty Board** | Auto-claims bounties when target is killed |
| **Safe Zones** | Auto god-mode inside admin-defined zones |
| **AFK Detection** | 15min warn, 20min kick for idle players |
| **VAC Screening** | Checks Steam API on login for VAC bans |
| **Admin Log** | JSONL audit trail of all admin actions |
| **Chat Filter** | Profanity + toxicity detection, 3-strike mute |
| **Map Tracker** | 10s position polling for live map overlay |
| **Zilla Protection** | Offline base protection sweep every 5 min |
| **Achievements** | Auto-unlock milestones (13 achievements) with economy rewards |

---

## Economy Flow

```
Daily login     → +25-500c (streak bonus)
Quest complete  → +25-200c (daily missions)
Kill bounty     → +bounty amount
Territory income → +25c/territory/10min (to clan)
Supply drop     → +150c + items
Warzone kills   → +50c/kill + 100c survival
Boss fight      → +500c to all survivors
Achievements    → +50-1000c per unlock
Lottery win     → full pot

Spending:
Bounties, gambling, trading, lottery tickets, kit cooldowns
Banking earns 1%/hr interest (max 100k)
```

## Reputation Tiers

| Score | Title |
|-------|-------|
| < -50 | Outlaw |
| -49 to -1 | Shady |
| 0 to 24 | Neutral |
| 25 to 74 | Trusted |
| 75 to 149 | Hero |
| 150+ | Legend |

Kills: -10 rep. `!thank`: +3 rep.
