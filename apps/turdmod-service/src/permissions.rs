// Permissions - global mod enable/disable + per-player access tiers.
// Every mod checks permissions::can(steam, "mod_name") before executing.
//
// Tiers: free < premium < vip < admin
// Admin commands: !perm grant <player> <tier>, !perm mod <mod_name> on/off,
//   !perm set <player> <mod_name> on/off, !perm list, !perm player <player>
//
// Mods call: permissions::can(&state, steam, "vehicle_ownership") -> bool
// Premium pass = player has "premium" tier or higher

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\permissions.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

// Tier ladder (low -> high). Donor levels (Bronze..Diamond) are the "premium
// levels" - each strictly includes the perks below it. VIP = friends/family,
// above all paid tiers. Admin = staff. Mirrors the Discord donor roles so the
// in-game tier and the Discord role stay in sync. Custom per-player command
// overrides sit on top (mod_overrides). @inv: discriminant order == perk order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Tier {
    Free = 0,
    Bronze = 1,
    Silver = 2,
    Gold = 3,
    Diamond = 4,
    Vip = 5,
    Admin = 6,
}

impl Tier {
    fn from_str(s: &str) -> Option<Tier> {
        match s.to_lowercase().as_str() {
            "free" => Some(Tier::Free),
            "bronze" => Some(Tier::Bronze),
            "silver" => Some(Tier::Silver),
            "gold" => Some(Tier::Gold),
            "diamond" => Some(Tier::Diamond),
            "vip" => Some(Tier::Vip),
            "admin" => Some(Tier::Admin),
            // aliases - old "premium"/"donor" map to the lowest donor level.
            "premium" | "donor" => Some(Tier::Bronze),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Tier::Free => "Free",
            Tier::Bronze => "Bronze",
            Tier::Silver => "Silver",
            Tier::Gold => "Gold",
            Tier::Diamond => "Diamond",
            Tier::Vip => "VIP",
            Tier::Admin => "Admin",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerPerms {
    pub name: String,
    pub tier: Tier,
    pub mod_overrides: HashMap<String, bool>, // mod_name -> allowed
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModConfig {
    pub enabled: bool,
    pub required_tier: Tier,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermState {
    pub players: HashMap<String, PlayerPerms>,  // steam -> perms
    pub mods: HashMap<String, ModConfig>,       // mod_name -> config
}

impl Default for PermState {
    fn default() -> Self {
        let mut mods = HashMap::new();
        let free_mods = vec![
            "economy", "teleport", "leaderboard", "voting", "quests",
            "achievements", "reputation", "rules", "radio", "scoreboard",
            "login_streak", "death_recap", "weather_alerts", "mod_list",
            "companions", "duels", "trading", "referral", "team_balance",
        ];
        let premium_mods = vec![
            "vehicle_ownership", "kits", "banking", "auction", "player_shops",
            "fast_travel", "map_markers", "vehicle_interactions", "convoy",
            "fishing_tournament", "scavenger_hunt", "npc_contracts",
        ];
        let vip_mods = vec![
            "gambling", "lottery", "supply_drops", "skill_boost",
        ];
        let admin_mods = vec![
            "god_mode", "metabolism", "jail", "warzone", "horde",
            "boss_fight", "airdrop", "safe_zones", "building_zones",
            "loot_multiplier", "mechman", "spawn_protection",
        ];

        // Gated CHAT COMMANDS, keyed by the command word (sans '!') so the
        // central gate in chat_cmds checks can(steam, <cmd>) and admins control
        // them via `!perm tier <cmd> <level>` / `!perm set <player> <cmd> on/off`.
        // Anything NOT listed defaults to Free (open to everyone). @dep: chat_cmds gate.
        // NOTE: tp/teleport/setpoint/points are gated inside teleport (its own
        // subscriber), NOT here - listing them here too would double-gate + double-message.
        let admin_cmds = vec![
            "announce", "broadcast", "spawn", "heal", "god",
            "kick", "ban", "jail", "money", "xp", "weather", "day", "night",
            "startobj", "addpvp", "forcerepo",
        ];

        for m in free_mods { mods.insert(m.into(), ModConfig { enabled: true, required_tier: Tier::Free }); }
        for m in premium_mods { mods.insert(m.into(), ModConfig { enabled: true, required_tier: Tier::Bronze }); }
        for m in vip_mods { mods.insert(m.into(), ModConfig { enabled: true, required_tier: Tier::Vip }); }
        for m in admin_mods { mods.insert(m.into(), ModConfig { enabled: true, required_tier: Tier::Admin }); }
        for m in admin_cmds { mods.insert(m.into(), ModConfig { enabled: true, required_tier: Tier::Admin }); }

        let mut players = HashMap::new();
        players.insert(crate::owner::primary_id().into(), PlayerPerms {
            name: crate::owner::name().into(),
            tier: Tier::Admin,
            mod_overrides: HashMap::new(),
        });

        PermState { players, mods }
    }
}

pub type SharedPerms = std::sync::Arc<tokio::sync::RwLock<PermState>>;

/// Persist the current state. Public so the HTTP layer (TMM Permissions page)
/// can save after editing the same live SharedPerms the in-game `!perm` mutates.
pub fn save_state(state: &PermState) { save(state); }

/// Tier ladder names, low -> high, for UI dropdowns. @inv: matches Tier order.
pub fn tier_names() -> &'static [&'static str] {
    &["Free", "Bronze", "Silver", "Gold", "Diamond", "VIP", "Admin"]
}

fn load() -> PermState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &PermState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

/// Check if a player can use a mod. Called by every module.
pub fn can(state: &PermState, steam: &str, mod_name: &str) -> bool {
    if crate::owner::is_owner_steam(steam) { return true; }

    let mod_cfg = state.mods.get(mod_name);
    if let Some(cfg) = mod_cfg {
        if !cfg.enabled { return false; }
    }

    let player = state.players.get(steam);

    if let Some(p) = player {
        if let Some(override_val) = p.mod_overrides.get(mod_name) {
            return *override_val;
        }
        if let Some(cfg) = mod_cfg {
            return p.tier >= cfg.required_tier;
        }
    }

    if let Some(cfg) = mod_cfg {
        return Tier::Free >= cfg.required_tier;
    }

    true // unknown mod = allowed
}

/// Gate helper for the chat-command interpreter. Returns `Some(msg)` (a
/// player-facing denial line) when `steam` may NOT use command `cmd`, or `None`
/// when allowed. `cmd` is the command word without the leading '!' (e.g. "tp").
pub fn cmd_denial(state: &PermState, steam: &str, cmd: &str) -> Option<String> {
    if can(state, steam, cmd) { return None; }
    if let Some(cfg) = state.mods.get(cmd) {
        if !cfg.enabled {
            return Some(format!("[Perms] '!{}' is currently disabled.", cmd));
        }
        return Some(format!(
            "[Perms] '!{}' requires {} tier or higher - ask an admin or check your perks.",
            cmd, cfg.required_tier.name()
        ));
    }
    Some(format!("[Perms] '!{}' is locked for your access level.", cmd))
}

/// A player's effective tier (owner Steam IDs always resolve to Admin).
pub fn player_tier(state: &PermState, steam: &str) -> Tier {
    if crate::owner::is_owner_steam(steam) { return Tier::Admin; }
    state.players.get(steam).map(|p| p.tier).unwrap_or(Tier::Free)
}

fn is_admin(steam: &str) -> bool {
    crate::owner::is_owner_steam(steam)
}

async fn say(player: &str, lines: &[&str]) {
    for line in lines {
        let params = serde_json::json!({ "message": *line, "playerName": player, "channel": "4" });
        pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub fn start() -> SharedPerms {
    std::sync::Arc::new(tokio::sync::RwLock::new(load()))
}

pub struct Permissions {
    rate: Mutex<HashMap<String, Instant>>,
}

impl Permissions {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Permissions {
    fn name(&self) -> &'static str { "permissions" }
    // event-driven: no commands() - handles both "login" (auto-register) and "chat" (!perm)

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                // Auto-register new players as Free tier
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if steam.is_empty() { return Outcome::Ignored; }
                let mut state = ctx.perms.write().await;
                state.players.entry(steam).or_insert_with(|| PlayerPerms {
                    name, tier: Tier::Free, mod_overrides: HashMap::new(),
                });
                Outcome::Handled
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with("!perm") { return Outcome::Ignored; }

                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now);
                }

                if !is_admin(&steam) {
                    say(&player, &["[Perms] Admin only."]).await;
                    return Outcome::Handled;
                }

                let parts: Vec<&str> = text.split_whitespace().collect();
                let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

                match sub.as_str() {
                    "grant" => {
                        if parts.len() < 4 {
                            say(&player, &[
                                "[Perms] Usage: !perm grant <player> <tier>",
                                "Tiers: free / premium / vip / admin",
                            ]).await;
                            return Outcome::Handled;
                        }
                        let target = parts[2];
                        let Some(tier) = Tier::from_str(parts[3]) else {
                            say(&player, &["[Perms] Unknown tier. Use: free / premium / vip / admin"]).await;
                            return Outcome::Handled;
                        };

                        let (msg_success, msg_target_tier) = {
                            let mut state = ctx.perms.write().await;
                            let target_steam: Option<String> = state.players.iter()
                                .find(|(_, p)| p.name.eq_ignore_ascii_case(target))
                                .map(|(s, _)| s.clone());

                            if let Some(ts) = target_steam {
                                state.players.get_mut(&ts).unwrap().tier = tier;
                            } else {
                                state.players.insert(format!("pending_{}", target.to_lowercase()), PlayerPerms {
                                    name: target.into(), tier, mod_overrides: HashMap::new(),
                                });
                            }
                            save(&state);
                            (format!("{} is now {} tier.", target, tier.name()),
                             format!("[Perms] Your access level: {}", tier.name()))
                        };

                        say(&player, &["==SUCCESS==", &msg_success]).await;
                        say(target, &[&msg_target_tier]).await;
                    }

                    "mod" => {
                        if parts.len() < 4 {
                            say(&player, &["[Perms] Usage: !perm mod <mod_name> on/off"]).await;
                            return Outcome::Handled;
                        }
                        let mod_name = parts[2].to_lowercase();
                        let enabled = parts[3].to_lowercase() != "off";

                        let status_msg = {
                            let mut state = ctx.perms.write().await;
                            let cfg = state.mods.entry(mod_name.clone()).or_insert(ModConfig {
                                enabled: true, required_tier: Tier::Free,
                            });
                            cfg.enabled = enabled;
                            save(&state);
                            let status = if enabled { "ENABLED" } else { "DISABLED" };
                            format!("{} is now {}.", mod_name, status)
                        };

                        say(&player, &["==SUCCESS==", &status_msg]).await;
                    }

                    "set" => {
                        if parts.len() < 5 {
                            say(&player, &[
                                "[Perms] Usage: !perm set <player> <mod> on/off",
                                "Overrides tier requirement for one player.",
                            ]).await;
                            return Outcome::Handled;
                        }
                        let target = parts[2];
                        let mod_name = parts[3].to_lowercase();
                        let allowed = parts[4].to_lowercase() != "off";

                        let result_msg = {
                            let mut state = ctx.perms.write().await;
                            let target_steam: Option<String> = state.players.iter()
                                .find(|(_, p)| p.name.eq_ignore_ascii_case(target))
                                .map(|(s, _)| s.clone());

                            if let Some(ts) = target_steam {
                                state.players.get_mut(&ts).unwrap().mod_overrides.insert(mod_name.clone(), allowed);
                                save(&state);
                                let status = if allowed { "GRANTED" } else { "REVOKED" };
                                Ok(format!("{} access to {} - {}.", target, mod_name, status))
                            } else {
                                Err(())
                            }
                        };

                        match result_msg {
                            Ok(msg) => say(&player, &["==SUCCESS==", &msg]).await,
                            Err(_) => say(&player, &["[Perms] Player not found."]).await,
                        }
                    }

                    "tier" => {
                        if parts.len() < 4 {
                            say(&player, &[
                                "[Perms] Usage: !perm tier <mod> <tier>",
                                "Sets required tier for a mod.",
                            ]).await;
                            return Outcome::Handled;
                        }
                        let mod_name = parts[2].to_lowercase();
                        let Some(tier) = Tier::from_str(parts[3]) else {
                            say(&player, &["[Perms] Unknown tier."]).await;
                            return Outcome::Handled;
                        };

                        let success_msg = {
                            let mut state = ctx.perms.write().await;
                            let cfg = state.mods.entry(mod_name.clone()).or_insert(ModConfig {
                                enabled: true, required_tier: Tier::Free,
                            });
                            cfg.required_tier = tier;
                            save(&state);
                            format!("{} now requires {} tier.", mod_name, tier.name())
                        };

                        say(&player, &["==SUCCESS==", &success_msg]).await;
                    }

                    "list" => {
                        let lines = {
                            let state = ctx.perms.read().await;
                            let mut mods: Vec<(&String, &ModConfig)> = state.mods.iter().collect();
                            mods.sort_by_key(|(n, _)| n.clone());
                            mods.chunks(5).map(|chunk| {
                                chunk.iter().map(|(n, c)| {
                                    let status = if c.enabled { "ON" } else { "OFF" };
                                    format!("{}: {} [{}]", n, status, c.required_tier.name())
                                }).collect::<Vec<_>>().join(" | ")
                            }).collect::<Vec<_>>()
                        };
                        say(&player, &["[Perms] Mod Status:"]).await;
                        for line in &lines {
                            say(&player, &[line.as_str()]).await;
                        }
                    }

                    "player" => {
                        if parts.len() < 3 {
                            say(&player, &["[Perms] Usage: !perm player <name>"]).await;
                            return Outcome::Handled;
                        }
                        let target = parts[2];
                        let (found_line, overrides_line) = {
                            let state = ctx.perms.read().await;
                            let found = state.players.iter()
                                .find(|(_, p)| p.name.eq_ignore_ascii_case(target));
                            if let Some((_, p)) = found {
                                let main_line = format!("[Perms] {} - Tier: {}", p.name, p.tier.name());
                                let ovr = if !p.mod_overrides.is_empty() {
                                    let s: Vec<String> = p.mod_overrides.iter()
                                        .map(|(m, a)| format!("{}: {}", m, if *a { "ON" } else { "OFF" }))
                                        .collect();
                                    Some(format!("Overrides: {}", s.join(", ")))
                                } else { None };
                                (main_line, ovr)
                            } else {
                                (format!("[Perms] {} - Free tier (default)", target), None)
                            }
                        };
                        say(&player, &[&found_line]).await;
                        if let Some(ovr) = overrides_line {
                            say(&player, &[&ovr]).await;
                        }
                    }

                    _ => {
                        say(&player, &[
                            "[Perms] Commands:",
                            "  !perm grant <player> <tier>",
                            "  !perm mod <mod> on/off",
                            "  !perm set <player> <mod> on/off",
                            "  !perm tier <mod> <tier>",
                            "  !perm list - all mods",
                            "  !perm player <name> - check player",
                        ]).await;
                    }
                }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
