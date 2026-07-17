# turdmod-service

Windows Service + HTTP API for remote SCUM server management. Runs on OVH (or any VPS). Handles engine lifecycle (suspended-start + DLL injection), auto-restart on crash, and runs **47 concurrent mod modules** with **85+ in-game chat commands**.

## Run locally (console mode)

```powershell
cd apps/turdmod-service
cargo run -- --console
```

Reads config from `C:\TurdMOD\service.json`. Falls back to defaults if missing.

## Build release

```powershell
cargo build --release
# Output: target/release/turdmod-service.exe
```

## Deploy to OVH

```powershell
.\scripts\deploy-service.ps1
```

Or manually:
1. SCP `target/release/turdmod-service.exe` to `C:\TurdMOD\` on OVH
2. SCP `config/service/service.json` to `C:\TurdMOD\service.json`
3. On OVH: `.\turdmod-service.exe --install` (registers Windows Service)
4. `sc start TurdMODService`

## Service management

```powershell
turdmod-service.exe --install      # Register with SCM
turdmod-service.exe --uninstall    # Remove from SCM
turdmod-service.exe --console      # Console mode (no SCM, for debugging)
turdmod-service.exe --console --instance sandbox  # Named instance
```

## HTTP API

All endpoints except `/health` and `/map/players` require `Authorization: Bearer <token>`.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Service health (no auth) |
| GET | `/map/players` | Live player positions (no auth) |
| GET | `/status` | Server running state, PID, uptime, restart count |
| POST | `/server/start` | Start SCUMServer with engine injection |
| POST | `/server/stop` | Kill server process |
| POST | `/server/restart` | Stop + start (also runs a restore-campaign pass in the stop-window) |
| GET | `/server/logs?lines=N` | Tail UE4SS.log |
| POST | `/scumdb/migrate` | One-shot restore (skills/attrs/fame/money) from a pre-wipe snapshot `{ "source": "<db>" }` |
| GET | `/restore/campaign` | Post-wipe restore campaign status |
| POST | `/restore/campaign/arm` | Arm auto restore-on-reconnect `{ "snapshot": "<pre-wipe db>" }` |
| POST | `/restore/campaign/disarm` | Stop the campaign |
| POST | `/engine/rpc` | Forward any method to bridge pipe `{ "method": "...", "params": {...} }` |
| POST | `/ollama/generate` | AI text generation via Ollama |
| GET | `/ollama/models` | List available Ollama models |
| POST | `/rcon` | RCON command forwarding |

## Mod Modules (47)

### Core Infrastructure
| Module | Description |
|--------|-------------|
| `server.rs` | axum HTTP routes + CORS + bearer auth middleware |
| `engine.rs` | SCUMServer lifecycle (CREATE_SUSPENDED + DLL inject) |
| `events.rs` | Persistent pipe event subscriber (chat, kill, login, logout) |
| `pipe_rpc.rs` | Named-pipe JSON-RPC client to bridge |
| `config.rs` | Multi-instance config loading |
| `auth.rs` | Bearer token middleware |
| `inject.rs` | DLL injection via CreateRemoteThread |
| `shutdown.rs` | Graceful shutdown via tokio::sync::watch |
| `rcon.rs` | Source RCON client — ⚠️ wrong protocol for direct SCUM (SCUM uses BattlEye RCON/UDP). Use `scripts/rcon_be.py`. See HANDBOOK.md §2.5 |
| `ollama.rs` | Ollama API client for AI features |

### Chat Commands
| Module | Commands | Description |
|--------|----------|-------------|
| `chat_cmds.rs` | `!help !players !server !ask !day !night !fly !weather !possess !unpossess !storm !clear !tp !spawn !stats` | Core admin commands |
| `economy.rs` | `!balance !daily !pay !top !bounty` | In-game currency system with daily bonuses |
| `teleport.rs` | `!setpoint !tp !points !delpoint` | Named teleport points |
| `vehicles.rs` | `!vehicles !vspawn !vlist !vbring !vdestroy` | Vehicle spawning and management |
| `vehicle_registry.rs` | `!register !garage !spawn !park !unregister !insure` | Player vehicle ownership |
| `god_mode.rs` | `!god !hulk !jump` | Persistent god mode + hulk leap |
| `companions.rs` | `!tame !companion !dismiss` | Animal taming via bridge AI writes |
| `kits.rs` | `!kit !kits` | Predefined item loadouts (starter/medic/builder/admin) |
| `duels.rs` | `!duel !accept !decline !duelstats` | 1v1 PvP challenges |
| `gambling.rs` | `!coinflip !dice !slots` | Casino games with economy integration |
| `voting.rs` | `!vote !yes !no` | Player voting (day/night/storm/restart) |
| `clans.rs` | `!clan create/invite/accept/leave/info/war/peace/list` | Faction system with clan wars |
| `reputation.rs` | `!rep !toprep !thank` | Player karma (Outlaw → Legend) |
| `rules.rs` | `!rules !motd !report !reports !warn` | Server rules + player reports |
| `trading.rs` | `!trade !accept !reject` | Player-to-player coin transfers |
| `leaderboard.rs` | `!kd !leaderboard !topkills !topdeaths` | Kill/death tracking |
| `quests.rs` | `!quest !quests !claim` | Daily missions with economy rewards |
| `metabolism.rs` | `!heal !feed !cure !xp !money` | Admin health/economy commands |
| `jail.rs` | `!jail !unjail !jailstatus` | Player jail with auto-release |
| `horde.rs` | `!horde !purge` | Wave-based zombie PvE events |
| `warzone.rs` | `!warzone !endwarzone !wzstatus` | Timed PvP events with kill rewards |
| `airdrop.rs` | `!airdrop !care` | Supply drops + emergency care packages |
| `lottery.rs` | `!lottery buy/status` | 30-min draw cycle, pot winner takes all |
| `scoreboard.rs` | `!mystats !topplayed !toptraveled` | All-time persistent stats |

### Background Systems (no chat commands)
| Module | Description |
|--------|-------------|
| `base_protection.rs` | Raid window management via RaidTimes.json |
| `admin_log.rs` | JSONL audit logging of all admin actions |
| `vac_screening.rs` | Steam API VAC ban check on player login |
| `game_events.rs` | Scheduled server announcements |
| `welcome.rs` | Auto-welcome new players with !help hint |
| `announcements.rs` | Join/leave/kill broadcast messages |
| `player_db.rs` | Steam ID tracking, sessions, playtime |
| `zilla_protection.rs` | Offline base protection (5-min sweep) |
| `map_tracker.rs` | Live position polling for map overlay |
| `analytics.rs` | JSONL position + event logger for heatmaps |
| `scheduler.rs` | 6-hour auto-restart + weather cycle |
| `safe_zones.rs` | Auto god-mode inside admin-defined zones |
| `bounty_board.rs` | Auto-claim bounties on kill events |
| `weather_alerts.rs` | Immersive storm approach/clearing alerts |
| `spawn_loadout.rs` | Auto-give items on login (newbie vs veteran) |
| `afk.rs` | 15-min warn, 20-min kick for idle players |
| `npc/` | DIMs NPC system (Ziggy, Doc Vera, Rust) |

### FriendlyPuppets (auto-loop in main.rs)
Every 30 seconds: `setZombiePassive` + `setAnimalPassive` — all creatures passive by default. Players provoke via combat (bridge `provokeZombies` handler).

## Configuration (`C:\TurdMOD\service.json`)

```json
{
  "port": 9090,
  "token": "your-secret-token",
  "scum_server_exe": "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\SCUMServer.exe",
  "scum_server_args": ["-log", "-port=7042", "-QueryPort=7044", "-NoBattlEye"],
  "inject_dlls": [
    "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\turdmod_server_loader.dll",
    "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\UE4SS\\UE4SS.dll"
  ],
  "auto_restart": true,
  "restart_delay_secs": 10
}
```

## Data files

All module state persists to `C:\TurdMOD\data\`:

| File | Module |
|------|--------|
| `economy.json` | economy, gambling, lottery, trading, bounty_board, warzone |
| `teleports.json` | teleport |
| `vehicle_registry.json` | vehicle_registry |
| `leaderboard.json` | leaderboard |
| `reputation.json` | reputation |
| `quests.json` | quests |
| `clans.json` | clans |
| `safe_zones.json` | safe_zones |
| `kits.json` | kits |
| `spawn_loadouts.json` | spawn_loadout |
| `scoreboard.json` | scoreboard |
| `reports.jsonl` | rules |
| `analytics/positions.jsonl` | analytics |
| `analytics/events.jsonl` | analytics |

## Preconditions

- Windows Server (elevated)
- SCUMServer.exe + UE4SS + TurdMODEngineBridge DLL deployed
- OVH firewall: TCP 9090 (API), UDP 7042 (game), TCP 7044 (Steam query)
- Token set to something non-default before production use
- Ollama (optional): CPU mode for AI features (!ask, DIMs NPCs)
