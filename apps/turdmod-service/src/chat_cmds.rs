// Chat command handler — rule-based !commands from in-game chat.
// All responses are private (sendChatLineToPlayer, yellow text).
// No AI required except !ask (gracefully fails without Ollama).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::bridge::{fmt_reply, reply, Command, OWNER_NAME, OWNER_STEAM_ID};
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const OWN_PATH: &str = r"C:\TurdMOD\data\vehicle_ownership.json";
const TRANSFER_PATH: &str = r"C:\TurdMOD\data\vehicle_transfers.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const SNAP_DIR: &str = r"C:\TurdMOD\data\vehicle_snapshots\";
const MAX_VEHICLES_PER_PLAYER: usize = 5;
const TEMP_TTL_SECS: u64 = 3 * 24 * 3600;
const MYRIDE_COOLDOWN_SECS: u64 = 60;
const COOLDOWN_PATH: &str = r"C:\TurdMOD\data\vehicle_cooldowns.json";

fn vehicle_value(name: &str) -> (i64, &'static str) {
    match name {
        "Kinglet_Duster" | "Kinglet_Mariner" => (5000, "Aircraft"),
        "Rager" => (3000, "SUV"),
        "Laika" => (2500, "Sedan"),
        "WolfsWagen" => (2000, "Sedan"),
        "Barba" => (2000, "Van"),
        "SidecarBike" => (1500, "Motorcycle"),
        "Tractor" => (1200, "Utility"),
        "Cruiser" => (1000, "Motorcycle"),
        "RIS" => (800, "Scooter"),
        "Dirtbike" => (600, "Motorcycle"),
        "SUP" => (500, "Watercraft"),
        "Dinghy" => (400, "Watercraft"),
        "MountainBike" => (300, "Bicycle"),
        "CityBike" => (200, "Bicycle"),
        _ => (1000, "Vehicle"),
    }
}

pub struct ChatCmds {
    rate: Mutex<HashMap<String, Instant>>,
}

impl ChatCmds {
    pub fn new() -> Self {
        Self { rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for ChatCmds {
    fn name(&self) -> &'static str { "chat_cmds" }
    // Event-driven: sees every chat event, self-filters on '!' prefix and dispatches all arms.
    fn commands(&self) -> &'static [&'static str] { &[] }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }

        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&player) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(player.clone(), now);
        }

        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        tracing::info!("chat_cmds: {} from {}", cmd, player);

        // Permission gate — read perms, build denial String, drop guard before any await.
        let cmd_key = cmd.trim_start_matches('!').to_string();
        let denial_msg: Option<String> = {
            let state = ctx.perms.read().await;
            crate::permissions::cmd_denial(&state, &steam, &cmd_key)
        };
        if let Some(msg) = denial_msg {
            reply(&msg, &player).await;
            return Outcome::Handled;
        }

        // Router first: it owns the migrated engine-pipe commands.
        let command = Command {
            action: cmd.trim_start_matches('!').to_string(),
            args: args.clone(),
            player: player.clone(),
            steam: steam.clone(),
        };
        if crate::router::dispatch(&command).await {
            return Outcome::Handled;
        }

        match cmd.as_str() {
            "!help" | "!commands" | "!cmds" => {
                // Paginated by category — `!help <category>` drills in. Only commands actually wired
                // are listed. @inv: keep in sync with the match arms below + router.rs + the
                // permissions/elevated/teleport mods. @dep: chat_cmds match arms, router::dispatch.
                let cat = args.split_whitespace().next().unwrap_or("").to_lowercase();
                match cat.as_str() {
                    "economy" | "econ" | "money" => {
                        reply("[Help: Economy] !bal  !top  !claim daily", &player).await;
                        reply("[Help: Economy] !insure [vehicle]  !insurance  !claim insurance  !value", &player).await;
                    }
                    "vehicles" | "vehicle" | "car" | "cars" => {
                        reply("[Help: Vehicles] !register  !garage  !release (#)  !transfer (#) <player>  !unregister", &player).await;
                        reply("[Help: Vehicles] !myride list|out|in|status|locate|name|share   !repos (repo history)", &player).await;
                    }
                    "progress" | "stats" | "rank" => {
                        reply("[Help: Progress] !leaderboard  !topkills  !kd  !rep  !stats", &player).await;
                    }
                    "events" | "event" | "fun" => {
                        reply("[Help: Events/Fun] !ask <question>  !ziggy  !doc  !rust   (server events are announced in chat)", &player).await;
                    }
                    "info" => {
                        reply("[Help: Info] !players  !server  !rules  !mods  !help <category>", &player).await;
                        reply("[Help: Info] Link Discord: run /link in our Discord, then !link <code> here (Verified role + 250 coins)", &player).await;
                    }
                    "admin" => {
                        reply("[Help: Admin] !day !night !time <0-24> !weather <0-1> !storm !clear  !spawn !fly !kick !ban", &player).await;
                        reply("[Help: Admin] !announce <msg>  !tp <a0..z3|player>  !!forcerepo  !elevate / !unelevate / !elevated", &player).await;
                        reply("[Help: Admin] !perm grant|mod|set|tier|list|player  (per-player + per-command permissions)", &player).await;
                    }
                    _ => {
                        reply("[ScummyMap] Commands -- type !help <category>:  info  economy  vehicles  progress  events  admin", &player).await;
                        reply("[ScummyMap] e.g. !help vehicles   |   New here? Link Discord: /link there, then !link <code> here.", &player).await;
                        reply("[ScummyMap] Full guide + live map at www.ScummyMap.com", &player).await;
                    }
                }
                Outcome::Handled
            }

            "!test" => {
                reply(&format!("[Debug] cmd='{}' args='{}' player='{}' steam='{}'", cmd, args, player, steam), &player).await;
                Outcome::Handled
            }

            "!ask" => {
                if args.is_empty() {
                    reply("[ScumPilot] Usage: !ask <your question>", &player).await;
                    return Outcome::Handled;
                }
                let q = args.clone();
                let p = player.clone();
                tokio::spawn(async move {
                    let req = crate::ollama::PromptReq {
                        model: Some("scumpilot-fast".into()),
                        prompt: q,
                        system: None,
                    };
                    match crate::ollama::generate(&req).await {
                        Ok(resp) => {
                            let answer = resp.response.trim().to_string();
                            let msg = if answer.len() > 200 {
                                format!("[ScumPilot] {}...", &answer[..197])
                            } else {
                                format!("[ScumPilot] {}", answer)
                            };
                            reply(&msg, &p).await;
                        }
                        Err(e) => {
                            tracing::warn!("chat_cmds: ollama error: {}", e);
                            reply("[ScumPilot] AI unavailable on this server.", &p).await;
                        }
                    }
                });
                Outcome::Handled
            }

            "!register" => {
                let db_p = crate::scumdb::db_path(None);
                let profiles = crate::scumdb::profile_ids_for(db_p, &steam, &player).unwrap_or_default();
                if profiles.is_empty() {
                    fmt_reply(&player, "Ownership", "FAILED", &[
                        "Could not resolve your profile yet.", "Reconnect and try again.",
                    ]).await;
                    return Outcome::Handled;
                }
                let owned = crate::scumdb::vehicles_owned_locked_by(db_p, &profiles).unwrap_or_default();
                if owned.is_empty() {
                    fmt_reply(&player, "Ownership", "NO LOCKED VEHICLE", &[
                        "Put a LOCK on a vehicle first - that claims it to you in-game.",
                        "Then !register adds your locked vehicles to your garage.",
                    ]).await;
                    return Outcome::Handled;
                }
                let mut state = read_json(OWN_PATH);
                if state.get("vehicles").is_none() { state["vehicles"] = serde_json::json!([]); }
                let existing: std::collections::HashSet<i64> = state["vehicles"].as_array()
                    .map(|a| a.iter().filter(|v| v["owner"].as_str() == Some(&player))
                        .filter_map(|v| v["entity_id"].as_i64()).collect())
                    .unwrap_or_default();
                let mut owned_count = state["vehicles"].as_array()
                    .map(|a| a.iter().filter(|v| v["owner"].as_str() == Some(&player)).count())
                    .unwrap_or(0);
                let now = now_secs();
                let (mut added_perm, mut added_temp) = (0usize, 0usize);
                for ov in &owned {
                    if existing.contains(&ov.entity_id) { continue; }
                    let is_temp = owned_count >= MAX_VEHICLES_PER_PLAYER;
                    let vname = ov.class.replace("_ES", "");
                    let mut entry = serde_json::json!({
                        "entity_id": ov.entity_id,
                        "vehicle": vname,
                        "owner": player,
                        "owner_profile": ov.owner_profile,
                        "registered_at": now.to_string(),
                        "lock_asset": ov.lock_asset,
                        "temp": is_temp,
                        "spawned": true,
                        "original_owner": player,
                        "transfer_history": [],
                    });
                    if is_temp {
                        entry["expires_at"] = serde_json::json!((now + TEMP_TTL_SECS).to_string());
                        added_temp += 1;
                    } else { added_perm += 1; }
                    state["vehicles"].as_array_mut().unwrap().push(entry);
                    owned_count += 1;
                }
                write_json(OWN_PATH, &state);
                if added_perm + added_temp == 0 {
                    fmt_reply(&player, "Ownership", "ALREADY REGISTERED", &[
                        &format!("All {} locked vehicle(s) already in your garage.", owned.len()),
                        "Use !garage to see them.",
                    ]).await;
                } else {
                    let arr = state["vehicles"].as_array();
                    let tot_perm = arr.map(|a| a.iter().filter(|v| v["owner"].as_str()==Some(&player) && !v["temp"].as_bool().unwrap_or(false)).count()).unwrap_or(0);
                    let tot_temp = arr.map(|a| a.iter().filter(|v| v["owner"].as_str()==Some(&player) && v["temp"].as_bool().unwrap_or(false)).count()).unwrap_or(0);
                    let mut lines = vec![
                        format!("Registered {} new ({} perm, {} temp).", added_perm + added_temp, added_perm, added_temp),
                        format!("Garage: {}/{} permanent + {} temp.", tot_perm, MAX_VEHICLES_PER_PLAYER, tot_temp),
                    ];
                    if added_temp > 0 {
                        lines.push(format!("Temp = over the {}-limit: !transfer to sell within 3 days or it's repo'd.", MAX_VEHICLES_PER_PLAYER));
                    }
                    lines.push("!garage for IDs + expiry.".to_string());
                    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
                    fmt_reply(&player, "Ownership", "REGISTERED", &refs).await;
                }
                Outcome::Handled
            }

            "!garage" | "!myrides" => {
                let db = read_json(OWN_PATH);
                let mine: Vec<serde_json::Value> = db["vehicles"].as_array().cloned().unwrap_or_default()
                    .into_iter().filter(|v| v["owner"].as_str() == Some(&player)).collect();
                if mine.is_empty() {
                    fmt_reply(&player, "Garage", "EMPTY", &[
                        "No registered vehicles.", "Lock a vehicle then !register.",
                    ]).await;
                    return Outcome::Handled;
                }
                let now = now_secs();
                let perm = mine.iter().filter(|v| !v["temp"].as_bool().unwrap_or(false)).count();
                reply(&format!("[Garage] {} vehicle(s) - {}/{} permanent slots used:",
                    mine.len(), perm.min(MAX_VEHICLES_PER_PLAYER), MAX_VEHICLES_PER_PLAYER), &player).await;
                for (i, v) in mine.iter().enumerate() {
                    let id = v["entity_id"].as_i64().unwrap_or(0);
                    let name = v["vehicle"].as_str().unwrap_or("?");
                    let status = if v["temp"].as_bool().unwrap_or(false) {
                        let exp = v["expires_at"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                        if exp > now {
                            let l = exp - now;
                            format!("TEMP - {}d {}h left", l / 86400, (l % 86400) / 3600)
                        } else { "TEMP - EXPIRED (repo pending)".to_string() }
                    } else { "permanent".to_string() };
                    reply(&format!("[Garage] ({}) {} - {} [id {}]", i + 1, name, status, id), &player).await;
                }
                reply("[Garage] !transfer (#) <player> to give one away, !release (#) to free a slot.", &player).await;
                Outcome::Handled
            }

            "!link" | "!verify" => {
                let code = args.split_whitespace().next().unwrap_or("").to_string();
                if code.is_empty() {
                    reply("[Verify] Run /link in our Discord to get a code, then type: !link <code>", &player).await;
                    return Outcome::Handled;
                }
                let db_p = crate::scumdb::db_path(None);
                let st = if !steam.is_empty() { steam.clone() } else {
                    crate::scumdb::profile_id_for_name(db_p, &player).ok().flatten()
                        .and_then(|pid| crate::scumdb::steam_for_profile_id(db_p, pid).ok().flatten())
                        .unwrap_or_default()
                };
                if st.is_empty() {
                    reply("[Verify] Could not resolve your Steam ID yet - reconnect and try !link again.", &player).await;
                    return Outcome::Handled;
                }
                match crate::discord_verify::redeem(&code, &st, &player) {
                    Ok(discord_id) => {
                        let mut econ = read_json(ECON_PATH);
                        let bal = econ["players"][&st]["balance"].as_i64().unwrap_or(0);
                        econ["players"][&st]["balance"] = serde_json::json!(bal + 250);
                        write_json(ECON_PATH, &econ);
                        match crate::discord_verify::assign_verified(&discord_id).await {
                            Ok(_) => reply("[Verify] \u{2705} Linked! Verified role granted in Discord + 250 coins. Welcome to ScummyMap!", &player).await,
                            Err(e) => reply(&format!("[Verify] Linked + 250 coins. Discord role pending ({}) - an admin can add Verified.", e), &player).await,
                        }
                    }
                    Err(e) => reply(&format!("[Verify] {}", e), &player).await,
                }
                Outcome::Handled
            }

            "!repos" | "!repo" => {
                let hist: Vec<serde_json::Value> = std::fs::read_to_string(r"C:\TurdMOD\data\repo_history.json")
                    .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                if hist.is_empty() {
                    fmt_reply(&player, "Car Repo", "EMPTY", &["No vehicles repossessed yet.", "Over-limit temps get repo'd after 3 days."]).await;
                    return Outcome::Handled;
                }
                let recent: Vec<&serde_json::Value> = hist.iter().rev().take(10).collect();
                reply(&format!("[Repo] Last {} repossessed (of {} total):", recent.len(), hist.len()), &player).await;
                for r in recent {
                    let id = r["entity_id"].as_i64().unwrap_or(0);
                    let name = r["vehicle"].as_str().unwrap_or("?");
                    let owner = r["owner"].as_str().unwrap_or("?");
                    reply(&format!("[Repo] #{} {} (was {}'s)", id, name, owner), &player).await;
                }
                Outcome::Handled
            }

            "!!forcerepo" | "!!repo" => {
                let is_admin = steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME;
                if !is_admin {
                    reply("[Car Repo] Admin only. Use !repos to view repo history.", &player).await;
                    return Outcome::Handled;
                }
                let db = read_json(OWN_PATH);
                let now = now_secs();
                let mut at_risk: Vec<(String, String, i64)> = Vec::new();
                if let Some(arr) = db["vehicles"].as_array() {
                    for v in arr {
                        if !v["temp"].as_bool().unwrap_or(false) { continue; }
                        let owner = v["owner"].as_str().unwrap_or("?").to_string();
                        let name = v["vehicle"].as_str().unwrap_or("vehicle").to_string();
                        let exp = v["expires_at"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                        at_risk.push((owner, name, exp as i64 - now as i64));
                    }
                }
                at_risk.sort_by_key(|(_, _, l)| *l);
                let expired_n = at_risk.iter().filter(|(_, _, l)| *l <= 0).count();

                let head = format!("\u{26A0} CAR REPO INITIATED (admin override) - {} expired temp vehicle(s) will be REPOSSESSED on the next restart ({} temp total). Keep your garage <=5 or !transfer in time!", expired_n, at_risk.len());
                pipe_rpc::call("sendHudMessage", Some(serde_json::json!({ "text": head.clone() }))).await.ok();
                pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": head }))).await.ok();

                if at_risk.is_empty() {
                    pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": "[Car Repo] No temp vehicles registered - nothing to repossess." }))).await.ok();
                } else {
                    for (owner, name, left) in at_risk.iter().take(15) {
                        let status = if *left <= 0 {
                            "EXPIRED - repo next restart".to_string()
                        } else {
                            let l = *left as u64;
                            format!("{}d {}h left", l / 86400, (l % 86400) / 3600)
                        };
                        pipe_rpc::call("broadcastChat", Some(serde_json::json!({
                            "text": format!("[Car Repo] {} - {} - {}", owner, name, status) }))).await.ok();
                    }
                    if at_risk.len() > 15 {
                        pipe_rpc::call("broadcastChat", Some(serde_json::json!({
                            "text": format!("[Car Repo] ...and {} more temp vehicle(s).", at_risk.len() - 15) }))).await.ok();
                    }
                }
                reply(&format!("[Car Repo] Broadcast sent. {} temp ({} expired) flagged for next-restart repo.", at_risk.len(), expired_n), &player).await;
                Outcome::Handled
            }

            "!announce" | "!broadcast" => {
                let text = args.trim();
                if text.is_empty() {
                    reply("[Announce] Usage: !announce <message> - shows the big banner to everyone.", &player).await;
                    return Outcome::Handled;
                }
                let announce_text = text.to_string();
                let ok = pipe_rpc::call("runAdminCommand", Some(serde_json::json!({
                    "command": format!("Announce {}", announce_text),
                    "playerName": &player,
                }))).await.is_ok();
                pipe_rpc::call("sendHudMessage", Some(serde_json::json!({ "text": format!("\u{1F4E2} {}", announce_text) }))).await.ok();
                pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": format!("\u{1F4E2} [ScummyMap] {}", announce_text) }))).await.ok();
                reply(
                    if ok { "[Announce] Banner + HUD + chat sent to all players." } else { "[Announce] Banner call failed (HUD+chat still sent)." },
                    &player,
                ).await;
                Outcome::Handled
            }

            "!myride" => {
                let sub = args.split_whitespace().next().unwrap_or("help").to_lowercase();
                let sub_args = args.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim().to_string();
                let mut own_db = read_json(OWN_PATH);
                let vehicles_arr: Vec<serde_json::Value> = own_db["vehicles"].as_array().cloned().unwrap_or_default();
                let is_admin = steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME;

                if matches!(sub.as_str(), "out" | "in") && !is_admin {
                    let cd = read_json(COOLDOWN_PATH);
                    let last_use: u64 = cd[&player]["myride"].as_str()
                        .and_then(|s| s.parse().ok()).unwrap_or(0);
                    let now = now_secs();
                    if now - last_use < MYRIDE_COOLDOWN_SECS {
                        let remaining = MYRIDE_COOLDOWN_SECS - (now - last_use);
                        fmt_reply(&player, "MyRide", "COOLDOWN", &[
                            &format!("Wait {}s before using !myride {} again.", remaining, sub),
                        ]).await;
                        return Outcome::Handled;
                    }
                }

                match sub.as_str() {
                    "list" => {
                        let owned: Vec<&serde_json::Value> = vehicles_arr.iter()
                            .filter(|v| v["owner"].as_str() == Some(&player)).collect();
                        if owned.is_empty() {
                            fmt_reply(&player, "MyRide", "NO VEHICLES", &["Sit in a vehicle and !register."]).await;
                        } else {
                            let out_count = owned.iter().filter(|v| v["spawned"].as_bool().unwrap_or(true)).count();
                            let stowed = owned.len() - out_count;
                            let mut lines: Vec<String> = vec![
                                format!("{}/{} slots used | {} out, {} stowed",
                                    owned.len(), MAX_VEHICLES_PER_PLAYER, out_count, stowed),
                            ];
                            for v in &owned {
                                let name = v["vehicle"].as_str().unwrap_or("?");
                                let nick = v["nickname"].as_str().unwrap_or("-");
                                let (_, vtype) = vehicle_value(name);
                                let ins = if v["insured"].as_bool().unwrap_or(false) { "INS" } else { "" };
                                let state = if v["spawned"].as_bool().unwrap_or(true) { "OUT" } else { "STOWED" };
                                lines.push(format!("  {} ({}) [{}] {} {}", name, vtype, state, ins, if nick != "-" { format!("\"{}\"", nick) } else { String::new() }));
                            }
                            let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
                            fmt_reply(&player, "MyRide", "YOUR VEHICLES", &refs).await;
                        }
                    }
                    "out" => {
                        let owned: Vec<&serde_json::Value> = vehicles_arr.iter()
                            .filter(|v| v["owner"].as_str() == Some(&player)).collect();
                        if owned.is_empty() {
                            fmt_reply(&player, "MyRide", "FAILED", &["No vehicle registered."]).await;
                            return Outcome::Handled;
                        }
                        let already_out = owned.iter().find(|v| v["spawned"].as_bool().unwrap_or(true));
                        if let Some(out_v) = already_out {
                            let out_name = out_v["vehicle"].as_str().unwrap_or("?");
                            fmt_reply(&player, "MyRide", "ALREADY OUT", &[
                                &format!("Your {} is already spawned.", out_name),
                                "Use !myride in to stow it first.",
                                "Only 1 vehicle out at a time.",
                            ]).await;
                            return Outcome::Handled;
                        }
                        let rec = if sub_args.is_empty() {
                            owned.iter().find(|v| !v["spawned"].as_bool().unwrap_or(true)).cloned()
                        } else {
                            owned.iter().find(|v| {
                                let vn = v["vehicle"].as_str().unwrap_or("");
                                let nick = v["nickname"].as_str().unwrap_or("");
                                vn.eq_ignore_ascii_case(&sub_args) || nick.eq_ignore_ascii_case(&sub_args)
                            }).cloned()
                        };
                        let rec = match rec {
                            Some(r) => r.clone(),
                            None => { fmt_reply(&player, "MyRide", "NOT FOUND", &[&format!("No stowed vehicle '{}'.", sub_args)]).await; return Outcome::Handled; }
                        };
                        let vname = rec["vehicle"].as_str().unwrap_or("Tractor");
                        let bpc = format!("BPC_{}", vname);
                        let params = serde_json::json!({"command": format!("SpawnVehicle {}", bpc), "playerName": &player});
                        let ok = pipe_rpc::call("runAdminCommand", Some(params)).await.is_ok();
                        if !ok {
                            fmt_reply(&player, "MyRide", "SPAWN FAILED", &["Move to flat ground and try again."]).await;
                            return Outcome::Handled;
                        }
                        let vname_owned = vname.to_string();
                        if let Some(arr) = own_db["vehicles"].as_array_mut() {
                            if let Some(v) = arr.iter_mut().find(|v| v["owner"].as_str() == Some(&player) && v["vehicle"].as_str() == Some(vname_owned.as_str())) {
                                v["spawned"] = serde_json::json!(true);
                                v["spawned_at"] = serde_json::json!(now_secs().to_string());
                            }
                        }
                        write_json(OWN_PATH, &own_db);
                        set_cooldown(&player, "myride");
                        let snap = load_vehicle_snapshot(&player, &vname_owned);
                        let item_count = snap["items"].as_array().map(|a| a.len()).unwrap_or(0);
                        if item_count > 0 {
                            for item in snap["items"].as_array().unwrap() {
                                let cls = item["class"].as_str().unwrap_or("");
                                if cls.is_empty() || cls.contains("Container") { continue; }
                                let spawn_name = cls.replace("_ES", "").replace("_C", "");
                                let p = serde_json::json!({"command": format!("SpawnItem {}", spawn_name), "playerName": &player});
                                pipe_rpc::call("runAdminCommand", Some(p)).await.ok();
                                tokio::time::sleep(Duration::from_millis(200)).await;
                            }
                        }
                        fmt_reply(&player, "MyRide", "VEHICLE OUT", &[
                            &format!("{} spawned nearby.", vname_owned),
                            &format!("{} item(s) restored.", item_count),
                        ]).await;
                    }
                    "in" => {
                        let rec = if !sub_args.is_empty() {
                            vehicles_arr.iter().find(|v| {
                                v["owner"].as_str() == Some(&player)
                                && v["spawned"].as_bool().unwrap_or(true)
                                && (v["vehicle"].as_str().map(|n| n.eq_ignore_ascii_case(&sub_args)).unwrap_or(false)
                                    || v["nickname"].as_str().map(|n| n.eq_ignore_ascii_case(&sub_args)).unwrap_or(false))
                            }).cloned()
                        } else {
                            vehicles_arr.iter().find(|v| {
                                v["owner"].as_str() == Some(&player) && v["spawned"].as_bool().unwrap_or(true)
                            }).cloned()
                        };
                        let rec = match rec {
                            Some(r) => r,
                            None => { fmt_reply(&player, "MyRide", "NOTHING OUT", &["You have no spawned vehicles to stow."]).await; return Outcome::Handled; }
                        };
                        let vname = rec["vehicle"].as_str().unwrap_or("Vehicle").to_string();
                        let db_p = crate::scumdb::db_path(None);

                        let eid = rec["entity_id"].as_i64().or_else(|| {
                            let pattern = format!("%{}%", vname);
                            crate::scumdb::query_entities(db_p, &pattern, 1).ok()
                                .and_then(|rows| rows.first().map(|r| r.id))
                        });

                        if let Some(id) = eid {
                            if let Ok(snap) = crate::scumdb::snapshot_vehicle(db_p, id) {
                                let snap_json = serde_json::to_value(&snap).unwrap_or_default();
                                save_vehicle_snapshot(&player, &vname, &snap_json);
                            }
                            if rec["entity_id"].is_null() {
                                if let Some(arr) = own_db["vehicles"].as_array_mut() {
                                    if let Some(v) = arr.iter_mut().find(|v| v["owner"].as_str() == Some(&player) && v["vehicle"].as_str() == Some(vname.as_str())) {
                                        v["entity_id"] = serde_json::json!(id);
                                    }
                                }
                            }
                        }

                        let bpc = format!("BPC_{}", vname);
                        let destroyed = if let Some(_id) = eid {
                            let actors_res = pipe_rpc::call("getNearbyActors", Some(serde_json::json!({
                                "playerName": &player, "classFilter": &bpc, "radius": 50000
                            }))).await;
                            let ptr = actors_res.ok().and_then(|v| {
                                v["actors"].as_array()?.first()?.get("ptr")?.as_str().map(|s| s.to_string())
                            });
                            if let Some(p) = ptr {
                                pipe_rpc::call("callActorFunction", Some(serde_json::json!({
                                    "ptr": p, "functionName": "K2_DestroyActor"
                                }))).await.is_ok()
                            } else { false }
                        } else { false };

                        if destroyed {
                            if let Some(arr) = own_db["vehicles"].as_array_mut() {
                                if let Some(v) = arr.iter_mut().find(|v| v["owner"].as_str() == Some(&player) && v["vehicle"].as_str() == Some(vname.as_str())) {
                                    v["spawned"] = serde_json::json!(false);
                                }
                            }
                            write_json(OWN_PATH, &own_db);
                            set_cooldown(&player, "myride");
                            fmt_reply(&player, "MyRide", "VEHICLE STOWED", &[
                                &format!("{} despawned + snapshot saved.", vname),
                                "Use !myride out to retrieve it.",
                            ]).await;
                        } else {
                            fmt_reply(&player, "MyRide", "FAILED", &[
                                "Could not find vehicle alias in DB.",
                                "Vehicle may need to be re-registered.",
                            ]).await;
                        }
                    }
                    "status" => {
                        let rec = match vehicles_arr.iter().find(|v| v["owner"].as_str() == Some(&player)) {
                            Some(r) => r.clone(),
                            None => { fmt_reply(&player, "MyRide", "NO VEHICLE", &["Register one first."]).await; return Outcome::Handled; }
                        };
                        let vname = rec["vehicle"].as_str().unwrap_or("?");
                        let snap = load_vehicle_snapshot(&player, vname);
                        let ins = if rec["insured"].as_bool().unwrap_or(false) { "Yes" } else { "No" };
                        let items = snap["items"].as_array().map(|a| a.len()).unwrap_or(0);
                        let state = if rec["spawned"].as_bool().unwrap_or(true) { "OUT" } else { "STOWED" };
                        let snap_at = snap["snapshot_at"].as_u64().map(|t| format!("{}", t)).unwrap_or("never".into());
                        fmt_reply(&player, "MyRide", "STATUS", &[
                            &format!("Vehicle: {} [{}]", vname, state),
                            &format!("Insured: {}", ins),
                            &format!("Items: {}", items),
                            &format!("Last snapshot: {}", snap_at),
                        ]).await;
                    }
                    "locate" => {
                        let rec = match vehicles_arr.iter().find(|v| v["owner"].as_str() == Some(&player)) {
                            Some(r) => r.clone(),
                            None => { fmt_reply(&player, "MyRide", "NO VEHICLE", &["Register one first."]).await; return Outcome::Handled; }
                        };
                        let vname = rec["vehicle"].as_str().unwrap_or("?");
                        let snap = load_vehicle_snapshot(&player, vname);
                        let sector = if let (Some(x), Some(y)) = (snap["x"].as_f64(), snap["y"].as_f64()) {
                            coords_to_sector(x, y)
                        } else { "unknown".into() };
                        fmt_reply(&player, "MyRide", "LOCATE", &[
                            &format!("{} last seen in sector: {}", vname, sector),
                        ]).await;
                    }
                    "name" => {
                        if sub_args.is_empty() { reply("Usage: !myride name <nickname>", &player).await; return Outcome::Handled; }
                        if let Some(arr) = own_db["vehicles"].as_array_mut() {
                            if let Some(v) = arr.iter_mut().find(|v| v["owner"].as_str() == Some(&player)) {
                                v["nickname"] = serde_json::json!(&sub_args);
                                write_json(OWN_PATH, &own_db);
                                fmt_reply(&player, "MyRide", "RENAMED", &[&format!("Nickname set to '{}'.", sub_args)]).await;
                            } else { reply("No vehicle registered.", &player).await; }
                        }
                    }
                    "share" => {
                        if sub_args.is_empty() { reply("Usage: !myride share <playerName>", &player).await; return Outcome::Handled; }
                        if let Some(arr) = own_db["vehicles"].as_array_mut() {
                            if let Some(v) = arr.iter_mut().find(|v| v["owner"].as_str() == Some(&player)) {
                                if !v.get("shared_with").is_some() { v["shared_with"] = serde_json::json!([]); }
                                let shared = v["shared_with"].as_array_mut().unwrap();
                                if shared.iter().any(|s| s.as_str() == Some(sub_args.as_str())) {
                                    reply(&format!("{} already has access.", sub_args), &player).await;
                                } else {
                                    shared.push(serde_json::json!(&sub_args));
                                    write_json(OWN_PATH, &own_db);
                                    fmt_reply(&player, "MyRide", "SHARED", &[&format!("{} can now access your vehicle.", sub_args)]).await;
                                }
                            } else { reply("No vehicle registered.", &player).await; }
                        }
                    }
                    _ => {
                        fmt_reply(&player, "MyRide", "HELP", &[
                            "!myride list       - your vehicles",
                            "!myride out [name] - spawn vehicle (1 at a time)",
                            "!myride in         - stow + snapshot",
                            "!myride status     - vehicle info",
                            "!myride locate     - sector location",
                            "!myride name <n>   - set nickname",
                            "!myride share <p>  - grant access",
                        ]).await;
                    }
                }
                Outcome::Handled
            }

            "!transfer" => {
                let toks: Vec<&str> = args.split_whitespace().collect();
                let (sel_idx, target): (Option<usize>, String) = match toks.first() {
                    Some(first) => {
                        let cleaned = first.trim_start_matches('(').trim_end_matches(')');
                        match cleaned.parse::<usize>() {
                            Ok(n) if n >= 1 => (Some(n), toks.get(1..).map(|s| s.join(" ")).unwrap_or_default()),
                            _ => (None, args.clone()),
                        }
                    }
                    None => (None, String::new()),
                };
                if target.trim().is_empty() {
                    reply("[Ownership] Usage: !transfer (#) <player>  (number from !garage)", &player).await;
                    reply("==END==", &player).await;
                } else {
                    let state = read_json(OWN_PATH);
                    let chosen = state["vehicles"].as_array().and_then(|a| {
                        let owned: Vec<&serde_json::Value> = a.iter()
                            .filter(|v| v["owner"].as_str() == Some(&player)).collect();
                        match sel_idx { Some(n) => owned.get(n - 1).copied(), None => owned.first().copied() }
                    });
                    match chosen {
                        None => {
                            reply("[Ownership] No vehicle at that number. !garage to check.", &player).await;
                            reply("==END==", &player).await;
                        }
                        Some(v) => {
                            let v_name = v["vehicle"].as_str().unwrap_or("Vehicle").to_string();
                            let v_id = v["entity_id"].as_i64().unwrap_or(0);
                            let mut transfers = read_json(TRANSFER_PATH);
                            transfers[target.to_lowercase()] = serde_json::json!({
                                "from": player,
                                "vehicle": v_name,
                                "entity_id": v_id,
                                // 5 min: buyer needs time to do the lock dance (seller pulls lock,
                                // buyer puts THEIR lock on) before !yes verifies their ownership.
                                "expires": (now_secs() + 300).to_string()
                            });
                            write_json(TRANSFER_PATH, &transfers);

                            reply("[Ownership] ==TRANSFER REQUEST==", &player).await;
                            reply(&format!("Selling your {} to {}. REMOVE YOUR LOCK so they can claim it.", v_name, target), &player).await;
                            reply("They have 5 min to lock it + !yes.", &player).await;
                            reply("==END==", &player).await;

                            reply(&format!("[Ownership] {} is selling you their {}.", player, v_name), &target).await;
                            reply("After they remove their lock, put YOUR lock on it, then !yes. (5 min)", &target).await;
                        }
                    }
                }
                Outcome::Handled
            }

            "!yes" => {
                let transfers = read_json(TRANSFER_PATH);
                let key = player.to_lowercase();
                if let Some(t) = transfers.get(&key) {
                    let expires: u64 = t["expires"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
                    if now_secs() > expires {
                        reply("[Ownership] Transfer expired.", &player).await;
                    } else {
                        let from = t["from"].as_str().unwrap_or("").to_string();
                        let vehicle = t["vehicle"].as_str().unwrap_or("").to_string();
                        let v_id = t["entity_id"].as_i64();

                        // The buyer must now hold THEIR lock on the vehicle — SCUM's lock IS the
                        // ownership, so this is the real "it's yours" signal. Verify the vehicle's
                        // current SCUM owner profile is the accepter's before reassigning the garage.
                        let db_p = crate::scumdb::db_path(None);
                        let buyer_owns = match v_id {
                            Some(id) => {
                                let ol = crate::scumdb::vehicle_owner_lock(db_p, id).ok();
                                let mine = crate::scumdb::profile_ids_for(db_p, &steam, &player).unwrap_or_default();
                                ol.map(|l| l.has_lock
                                    && l.owning_profile_id.map(|p| mine.contains(&p)).unwrap_or(false))
                                    .unwrap_or(false)
                            }
                            None => false,
                        };
                        if !buyer_owns {
                            reply(&format!("[Ownership] Have {} remove their lock, then put YOUR lock on the {} (that claims it to you), then !yes.", from, vehicle), &player).await;
                            reply("==END==", &player).await;
                            return Outcome::Handled; // leave the request pending until it expires
                        }

                        let mut state = read_json(OWN_PATH);
                        if let Some(arr) = state["vehicles"].as_array_mut() {
                            if let Some(v) = arr.iter_mut().find(|v| v["owner"].as_str() == Some(&from)
                                && match v_id { Some(id) => v["entity_id"].as_i64() == Some(id),
                                                None => v["vehicle"].as_str() == Some(&vehicle) }) {
                                v["owner"] = serde_json::json!(player);
                                v["registered_at"] = serde_json::json!(now_secs().to_string());
                            }
                        }
                        write_json(OWN_PATH, &state);

                        let mut transfers2 = read_json(TRANSFER_PATH);
                        if let Some(obj) = transfers2.as_object_mut() { obj.remove(&key); }
                        write_json(TRANSFER_PATH, &transfers2);

                        reply("[Ownership] ==TRANSFER ACCEPTED==", &player).await;
                        reply(&format!("You now own the {}.", vehicle), &player).await;
                        reply("==END==", &player).await;

                        reply("[Ownership] Transfer complete.", &from).await;
                        reply(&format!("{} now owns your {}.", player, vehicle), &from).await;
                        reply("==END==", &from).await;
                    }
                }
                Outcome::Handled
            }

            "!no" => {
                let transfers = read_json(TRANSFER_PATH);
                let key = player.to_lowercase();
                if let Some(t) = transfers.get(&key) {
                    let from = t["from"].as_str().unwrap_or("").to_string();
                    let mut transfers2 = read_json(TRANSFER_PATH);
                    if let Some(obj) = transfers2.as_object_mut() { obj.remove(&key); }
                    write_json(TRANSFER_PATH, &transfers2);

                    reply("[Ownership] Transfer declined.", &player).await;
                    reply(&format!("[Ownership] {} declined the transfer.", player), &from).await;
                }
                Outcome::Handled
            }

            "!release" => {
                let n: usize = args.split_whitespace().next()
                    .map(|s| s.trim_start_matches('(').trim_end_matches(')'))
                    .and_then(|s| s.parse().ok()).unwrap_or(0);
                if n == 0 {
                    reply("[Garage] Usage: !release (#) - the number from !garage.", &player).await;
                    reply("==END==", &player).await;
                    return Outcome::Handled;
                }
                let mut state = read_json(OWN_PATH);
                let target = state["vehicles"].as_array().and_then(|a| {
                    a.iter().filter(|v| v["owner"].as_str() == Some(&player)).nth(n - 1)
                        .map(|v| (v["entity_id"].as_i64().unwrap_or(0),
                                  v["vehicle"].as_str().unwrap_or("vehicle").to_string()))
                });
                match target {
                    Some((eid, name)) => {
                        // SCUM's lock IS the ownership assignment. Require the player to physically
                        // remove the lock before releasing — release just confirms the vehicle is
                        // unlocked/unowned. (No bridge needed: scumdb reads the live lock state.)
                        let db_p = crate::scumdb::db_path(None);
                        let locked = crate::scumdb::vehicle_owner_lock(db_p, eid)
                            .map(|l| l.has_lock).unwrap_or(false);
                        if locked {
                            reply("[Garage] Vehicle lock is ON - remove your lock first, then !release to free it (or !transfer to sell).", &player).await;
                            reply("Just removed it? Give it ~5s to save, then run !release again.", &player).await;
                            reply("==END==", &player).await;
                            return Outcome::Handled;
                        }
                        if let Some(arr) = state["vehicles"].as_array_mut() {
                            arr.retain(|v| v["entity_id"].as_i64() != Some(eid));
                        }
                        write_json(OWN_PATH, &state);
                        reply("[Garage] ==RELEASED==", &player).await;
                        reply(&format!("({}) {} released - slot freed. !garage renumbers automatically.", n, name), &player).await;
                        reply("==END==", &player).await;
                    }
                    None => {
                        reply(&format!("[Garage] No vehicle ({}) in your garage. !garage to check.", n), &player).await;
                        reply("==END==", &player).await;
                    }
                }
                Outcome::Handled
            }

            "!unregister" => {
                let mut state = read_json(OWN_PATH);
                let before = state["vehicles"].as_array().map(|a| a.len()).unwrap_or(0);
                if let Some(arr) = state["vehicles"].as_array_mut() {
                    arr.retain(|v| v["owner"].as_str() != Some(&player));
                }
                let after = state["vehicles"].as_array().map(|a| a.len()).unwrap_or(0);
                if after < before {
                    write_json(OWN_PATH, &state);
                    reply("[Ownership] ==SUCCESS==", &player).await;
                    reply(&format!("All {} vehicle(s) unregistered - no longer claimed. (!release (#) drops just one.)", before - after), &player).await;
                } else {
                    reply("[Ownership] No vehicle to unregister.", &player).await;
                }
                reply("==END==", &player).await;
                Outcome::Handled
            }

            "!balance" | "!bal" => {
                let econ = read_json(ECON_PATH);
                let bal = econ["players"].as_object()
                    .and_then(|p| p.values().find(|v| v["name"].as_str() == Some(&player)))
                    .and_then(|v| v["balance"].as_i64()).unwrap_or(0);
                fmt_reply(&player, "Economy", "Balance", &[&format!("{} coins", bal)]).await;
                Outcome::Handled
            }

            "!daily" => {
                reply("[ScummyMap] !daily moved to !claim daily", &player).await;
                Outcome::Handled
            }

            "!top" => {
                let econ = read_json(ECON_PATH);
                if let Some(p) = econ["players"].as_object() {
                    let mut entries: Vec<(&str, i64)> = p.values()
                        .map(|v| (v["name"].as_str().unwrap_or("?"), v["balance"].as_i64().unwrap_or(0)))
                        .collect();
                    entries.sort_by(|a, b| b.1.cmp(&a.1));
                    entries.truncate(5);
                    reply("[Economy] ==TOP 5==", &player).await;
                    for (i, (n, b)) in entries.iter().enumerate() {
                        reply(&format!("  {}. {} - {}c", i + 1, n, b), &player).await;
                    }
                } else {
                    reply("[Economy] No data yet.", &player).await;
                }
                reply("==END==", &player).await;
                Outcome::Handled
            }

            "!insure" => {
                let mounted = check_mounted(&player).await;
                let state = read_json(OWN_PATH);
                let owned: Vec<&serde_json::Value> = state["vehicles"].as_array()
                    .map(|a| a.iter().filter(|v| v["owner"].as_str() == Some(&player)).collect())
                    .unwrap_or_default();

                if owned.is_empty() {
                    reply("[Insurance] ==FAILED==", &player).await;
                    reply("You don't own any vehicles.", &player).await;
                    reply("Sit in a vehicle and !register first.", &player).await;
                    reply("==END==", &player).await;
                    return Outcome::Handled;
                }

                let target_vehicle = if mounted.is_some() {
                    let v_name = find_vehicle_near(&player).await;
                    if !owned.iter().any(|v| v["vehicle"].as_str().unwrap_or("").eq_ignore_ascii_case(&v_name)) {
                        reply("[Insurance] ==FAILED==", &player).await;
                        reply("You don't own this vehicle.", &player).await;
                        reply("!register it first.", &player).await;
                        reply("==END==", &player).await;
                        return Outcome::Handled;
                    }
                    v_name
                } else if !args.is_empty() {
                    let target = args.clone();
                    if !owned.iter().any(|v| {
                        v["vehicle"].as_str().unwrap_or("").eq_ignore_ascii_case(&target)
                    }) {
                        reply("[Insurance] ==FAILED==", &player).await;
                        reply(&format!("You don't own a {}.", target), &player).await;
                        reply("==END==", &player).await;
                        return Outcome::Handled;
                    }
                    owned.iter().find(|v| v["vehicle"].as_str().unwrap_or("").eq_ignore_ascii_case(&target))
                        .and_then(|v| v["vehicle"].as_str()).unwrap_or("Vehicle").to_string()
                } else if owned.len() == 1 {
                    owned[0]["vehicle"].as_str().unwrap_or("Vehicle").to_string()
                } else {
                    reply("[Insurance] Which vehicle?", &player).await;
                    for v in &owned {
                        let name = v["vehicle"].as_str().unwrap_or("?");
                        let insured = v.get("insured").and_then(|i| i.as_bool()).unwrap_or(false);
                        let tag = if insured { " [INSURED]" } else { "" };
                        reply(&format!("  {}{}", name, tag), &player).await;
                    }
                    reply("Type: !insure <vehicle_name>", &player).await;
                    reply("==END==", &player).await;
                    return Outcome::Handled;
                };

                if owned.iter().any(|v| {
                    v["vehicle"].as_str().unwrap_or("").eq_ignore_ascii_case(&target_vehicle) &&
                    v.get("insured").and_then(|i| i.as_bool()).unwrap_or(false)
                }) {
                    reply("[Insurance] ==ALREADY INSURED==", &player).await;
                    reply(&format!("{} already has insurance.", target_vehicle), &player).await;
                    reply("==END==", &player).await;
                    return Outcome::Handled;
                }

                let (value, vtype) = vehicle_value(&target_vehicle);
                let cost = value / 10;

                let mut econ = read_json(ECON_PATH);
                let bal = econ["players"].as_object()
                    .and_then(|p| p.values().find(|v| v["name"].as_str() == Some(&player)))
                    .and_then(|v| v["balance"].as_i64()).unwrap_or(0);

                if bal < cost {
                    reply("[Insurance] ==FAILED==", &player).await;
                    reply(&format!("{} insurance costs {} coins.", target_vehicle, cost), &player).await;
                    reply(&format!("Your balance: {} coins. Need {} more.", bal, cost - bal), &player).await;
                    reply("==END==", &player).await;
                    return Outcome::Handled;
                }

                if let Some(p) = econ["players"].as_object_mut() {
                    if let Some(entry) = p.values_mut().find(|v| v["name"].as_str() == Some(&player)) {
                        entry["balance"] = serde_json::json!(bal - cost);
                    }
                }
                write_json(ECON_PATH, &econ);

                let mut own_state = read_json(OWN_PATH);
                if let Some(arr) = own_state["vehicles"].as_array_mut() {
                    if let Some(v) = arr.iter_mut().find(|v| {
                        v["owner"].as_str() == Some(&player) &&
                        v["vehicle"].as_str().unwrap_or("").eq_ignore_ascii_case(&target_vehicle)
                    }) {
                        v["insured"] = serde_json::json!(true);
                        v["insured_at"] = serde_json::json!(now_secs().to_string());
                        v["insured_value"] = serde_json::json!(value);
                    }
                }
                write_json(OWN_PATH, &own_state);

                reply("[Insurance] ==SUCCESS==", &player).await;
                reply(&format!("{} ({}) insured!", target_vehicle, vtype), &player).await;
                reply(&format!("Cost: {} coins (10% of {} value)", cost, value), &player).await;
                reply(&format!("Balance: {} coins", bal - cost), &player).await;
                reply("If destroyed, use !claim to get a replacement.", &player).await;
                reply("==END==", &player).await;
                Outcome::Handled
            }

            "!insurance" => {
                let state = read_json(OWN_PATH);
                let owned: Vec<&serde_json::Value> = state["vehicles"].as_array()
                    .map(|a| a.iter().filter(|v| v["owner"].as_str() == Some(&player)).collect())
                    .unwrap_or_default();

                if owned.is_empty() {
                    reply("[Insurance] ==NO VEHICLES==", &player).await;
                    reply("You don't own any vehicles.", &player).await;
                    reply("Sit in a vehicle and !register first.", &player).await;
                    reply("==END==", &player).await;
                    return Outcome::Handled;
                }

                reply("[Insurance] ==YOUR VEHICLES==", &player).await;
                for v in &owned {
                    let name = v["vehicle"].as_str().unwrap_or("?");
                    let insured = v.get("insured").and_then(|i| i.as_bool()).unwrap_or(false);
                    let (value, vtype) = vehicle_value(name);
                    let cost = value / 10;
                    if insured {
                        reply(&format!("  {} ({}) [INSURED]", name, vtype), &player).await;
                    } else {
                        reply(&format!("  {} ({}) - NOT INSURED ({}c to insure)", name, vtype, cost), &player).await;
                    }
                }
                if owned.iter().any(|v| !v.get("insured").and_then(|i| i.as_bool()).unwrap_or(false)) {
                    reply("Use !insure to insure a vehicle.", &player).await;
                }
                reply("==END==", &player).await;
                Outcome::Handled
            }

            "!claim" => {
                let sub = args.split_whitespace().next().unwrap_or("").to_lowercase();
                let sub_args = args.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim().to_string();

                match sub.as_str() {
                    "insurance" => {
                        let state = read_json(OWN_PATH);
                        let insured: Vec<&serde_json::Value> = state["vehicles"].as_array()
                            .map(|a| a.iter().filter(|v| {
                                v["owner"].as_str() == Some(&player) &&
                                v.get("insured").and_then(|i| i.as_bool()).unwrap_or(false)
                            }).collect())
                            .unwrap_or_default();
                        if insured.is_empty() {
                            fmt_reply(&player, "Claim", "NO INSURED VEHICLES", &["Use !insure first."]).await;
                            return Outcome::Handled;
                        }
                        if sub_args.is_empty() {
                            if insured.len() == 1 {
                                // Auto-select sole vehicle
                            } else {
                                let mut lines: Vec<String> = vec!["Specify which vehicle:".into()];
                                for v in &insured {
                                    let name = v["vehicle"].as_str().unwrap_or("?");
                                    let (val, _) = vehicle_value(name);
                                    lines.push(format!("  {} - deductible: {}c", name, val / 5));
                                }
                                lines.push("!claim insurance <vehicle_name>".into());
                                let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
                                fmt_reply(&player, "Claim", "INSURANCE CLAIM", &refs).await;
                                return Outcome::Handled;
                            }
                        }
                        let target_name = if sub_args.is_empty() {
                            insured[0]["vehicle"].as_str().unwrap_or("").to_string()
                        } else { sub_args.clone() };
                        let rec = match insured.iter().find(|v| {
                            v["vehicle"].as_str().map(|n| n.eq_ignore_ascii_case(&target_name)).unwrap_or(false)
                        }) {
                            Some(r) => *r,
                            None => { fmt_reply(&player, "Claim", "NOT FOUND", &[&format!("No insured vehicle '{}'.", target_name)]).await; return Outcome::Handled; }
                        };
                        let v_name = rec["vehicle"].as_str().unwrap_or("Vehicle").to_string();
                        let (value, _) = vehicle_value(&v_name);
                        let deductible = value / 5;
                        let mut econ = read_json(ECON_PATH);
                        let ek = econ["players"].as_object()
                            .and_then(|p| p.iter().find(|(_, v)| v["name"].as_str() == Some(&player)).map(|(k, _)| k.clone()))
                            .unwrap_or_default();
                        let bal = econ["players"][&ek]["balance"].as_i64().unwrap_or(0);
                        if bal < deductible {
                            fmt_reply(&player, "Claim", "INSUFFICIENT FUNDS", &[
                                &format!("Need {}c, you have {}c.", deductible, bal),
                            ]).await;
                            return Outcome::Handled;
                        }
                        econ["players"][&ek]["balance"] = serde_json::json!(bal - deductible);
                        write_json(ECON_PATH, &econ);
                        let bpc = format!("BPC_{}", v_name);
                        let params = serde_json::json!({"command": format!("SpawnVehicle {}", bpc), "playerName": &player});
                        let ok = pipe_rpc::call("runAdminCommand", Some(params)).await.is_ok();
                        let mut own_state = read_json(OWN_PATH);
                        if let Some(arr) = own_state["vehicles"].as_array_mut() {
                            if let Some(v) = arr.iter_mut().find(|v| {
                                v["owner"].as_str() == Some(&player) && v["vehicle"].as_str().map(|n| n.eq_ignore_ascii_case(&v_name)).unwrap_or(false)
                            }) {
                                v["insured"] = serde_json::json!(false);
                                v["claimed_at"] = serde_json::json!(now_secs().to_string());
                            }
                        }
                        write_json(OWN_PATH, &own_state);
                        if ok {
                            fmt_reply(&player, "Claim", "INSURANCE PAID", &[
                                &format!("{} - {}c deducted.", v_name, deductible),
                                &format!("Balance: {}c", bal - deductible),
                                "Replacement spawned. Re-insure with !insure.",
                            ]).await;
                        } else {
                            let mut econ2 = read_json(ECON_PATH);
                            econ2["players"][&ek]["balance"] = serde_json::json!(bal);
                            write_json(ECON_PATH, &econ2);
                            fmt_reply(&player, "Claim", "SPAWN FAILED", &["Deductible refunded. Try on flat ground."]).await;
                        }
                    }
                    "daily" => {
                        let mut econ = read_json(ECON_PATH);
                        let ek = econ["players"].as_object()
                            .and_then(|p| p.iter().find(|(_, v)| v["name"].as_str() == Some(&player)).map(|(k, _)| k.clone()))
                            .unwrap_or_else(|| player.to_lowercase());
                        let last: u64 = econ["players"][&ek]["last_daily"].as_str()
                            .and_then(|s| s.parse().ok()).unwrap_or(0);
                        let now = now_secs();
                        if now - last < 86400 {
                            let remaining = 86400u64.saturating_sub(now - last);
                            fmt_reply(&player, "Claim", "ALREADY CLAIMED", &[
                                &format!("Next in: {}h {}m", remaining / 3600, (remaining % 3600) / 60),
                            ]).await;
                        } else {
                            let streak: u64 = econ["players"][&ek]["login_streak"].as_u64().unwrap_or(0);
                            let new_streak = if now - last < 172800 { streak + 1 } else { 1 };
                            let bonus = match new_streak { 2 => 20, 3 => 30, 4 => 50, 5 => 75, s if s >= 7 => 150, 6 => 100, _ => 0 };
                            let bal = econ["players"][&ek]["balance"].as_i64().unwrap_or(0);
                            let reward = 100 + bonus;
                            econ["players"][&ek]["balance"] = serde_json::json!(bal + reward);
                            econ["players"][&ek]["last_daily"] = serde_json::json!(now.to_string());
                            econ["players"][&ek]["login_streak"] = serde_json::json!(new_streak);
                            econ["players"][&ek]["name"] = serde_json::json!(&player);
                            write_json(ECON_PATH, &econ);
                            let streak_line = if bonus > 0 { format!("Day {} streak: +{}c bonus", new_streak, bonus) }
                                else { format!("Day {} streak", new_streak) };
                            fmt_reply(&player, "Claim", "DAILY BONUS", &[
                                "+100c base",
                                &streak_line,
                                &format!("Total: +{}c | Balance: {}c", reward, bal + reward),
                            ]).await;
                        }
                    }
                    "bounty" => {
                        fmt_reply(&player, "Claim", "BOUNTY", &["Bounty system coming soon."]).await;
                    }
                    "prize" => {
                        fmt_reply(&player, "Claim", "NO PRIZES", &["No unclaimed prizes. Join events to earn prizes!"]).await;
                    }
                    _ => {
                        let econ = read_json(ECON_PATH);
                        let ek = econ["players"].as_object()
                            .and_then(|p| p.iter().find(|(_, v)| v["name"].as_str() == Some(&player)).map(|(k, _)| k.clone()))
                            .unwrap_or_default();
                        let last_daily: u64 = econ["players"][&ek]["last_daily"].as_str()
                            .and_then(|s| s.parse().ok()).unwrap_or(0);
                        let daily_ready = now_secs() - last_daily >= 86400;
                        let own = read_json(OWN_PATH);
                        let ins_count = own["vehicles"].as_array()
                            .map(|a| a.iter().filter(|v| v["owner"].as_str() == Some(&player) && v["insured"].as_bool().unwrap_or(false)).count())
                            .unwrap_or(0);
                        let daily_str = if daily_ready { "!claim daily     - READY" } else { "!claim daily     - on cooldown" };
                        let ins_str = format!("!claim insurance - {} insured vehicle(s)", ins_count);
                        fmt_reply(&player, "Claim", "CLAIM CENTER", &[
                            daily_str,
                            &ins_str,
                            "!claim bounty    - coming soon",
                            "!claim prize     - no active prizes",
                        ]).await;
                    }
                }
                Outcome::Handled
            }

            "!carboard" => {
                let sub = args.split_whitespace().next().unwrap_or("").to_lowercase();
                match sub.as_str() {
                    "stolen" => {
                        let v_name = find_vehicle_near(&player).await;
                        if v_name == "Vehicle" {
                            fmt_reply(&player, "Carboard", "NOT FOUND", &["No vehicle nearby."]).await;
                            return Outcome::Handled;
                        }
                        let mut state = read_json(OWN_PATH);
                        let matched = state["vehicles"].as_array_mut().and_then(|arr| {
                            arr.iter_mut().find(|v| v["vehicle"].as_str().map(|n| n.eq_ignore_ascii_case(&v_name)).unwrap_or(false))
                        });
                        match matched {
                            None => { fmt_reply(&player, "Carboard", "UNREGISTERED", &[&format!("{} is not registered.", v_name)]).await; }
                            Some(rec) => {
                                let owner = rec["owner"].as_str().unwrap_or("?").to_string();
                                if owner.eq_ignore_ascii_case(&player) {
                                    fmt_reply(&player, "Carboard", "FAILED", &["Can't flag your own vehicle."]).await;
                                } else {
                                    rec["stolen_flag"] = serde_json::json!(true);
                                    write_json(OWN_PATH, &state);
                                    fmt_reply(&player, "Carboard", "STOLEN FLAG SET", &[
                                        &format!("Vehicle: {}", v_name),
                                        &format!("Owner: {}", owner),
                                        "Owner will be notified.",
                                    ]).await;
                                    reply(&format!("[Carboard] Your {} flagged stolen by {}.", v_name, player), &owner).await;
                                }
                            }
                        }
                    }
                    name if !name.is_empty() => {
                        let state = read_json(OWN_PATH);
                        let found = state["vehicles"].as_array().and_then(|arr| {
                            arr.iter().find(|v| {
                                let vn = v["vehicle"].as_str().unwrap_or("");
                                let nick = v["nickname"].as_str().unwrap_or("");
                                vn.eq_ignore_ascii_case(name) || nick.eq_ignore_ascii_case(name)
                            })
                        });
                        match found {
                            None => { fmt_reply(&player, "Carboard", "NOT FOUND", &[&format!("No record for '{}'.", args)]).await; }
                            Some(rec) => { show_carboard(&player, rec).await; }
                        }
                    }
                    _ => {
                        let v_name = find_vehicle_near(&player).await;
                        if v_name == "Vehicle" {
                            fmt_reply(&player, "Carboard", "NO VEHICLE", &[
                                "Walk up to a vehicle and try again.",
                                "Or: !carboard <vehicle_name>",
                            ]).await;
                            return Outcome::Handled;
                        }
                        let state = read_json(OWN_PATH);
                        let found = state["vehicles"].as_array().and_then(|arr| {
                            arr.iter().find(|v| v["vehicle"].as_str().map(|n| n.eq_ignore_ascii_case(&v_name)).unwrap_or(false))
                        });
                        match found {
                            None => {
                                fmt_reply(&player, "Carboard", &format!("{} - UNCLAIMED", v_name), &[
                                    "This vehicle is unclaimed.",
                                    "Sit in it and !register to claim.",
                                ]).await;
                            }
                            Some(rec) => { show_carboard(&player, rec).await; }
                        }
                    }
                }
                Outcome::Handled
            }

            "!value" => {
                if args.is_empty() {
                    reply("[Dealer] ==VEHICLE VALUES==", &player).await;
                    let vehicles = vec![
                        "Kinglet_Duster", "Kinglet_Mariner", "Rager", "Laika",
                        "WolfsWagen", "Barba", "SidecarBike", "Tractor",
                        "Cruiser", "RIS", "Dirtbike", "SUP", "Dinghy",
                        "MountainBike", "CityBike",
                    ];
                    for v in vehicles {
                        let (value, vtype) = vehicle_value(v);
                        let insurance = value / 10;
                        reply(&format!("  {} ({}) - {}c | Insure: {}c", v, vtype, value, insurance), &player).await;
                    }
                    reply("==END==", &player).await;
                } else {
                    let (value, vtype) = vehicle_value(&args);
                    let insurance = value / 10;
                    reply(&format!("[Dealer] {} ({}) - Value: {}c | Insurance: {}c", args, vtype, value, insurance), &player).await;
                    reply("==END==", &player).await;
                }
                Outcome::Handled
            }

            "!kd" => {
                let state = read_json(r"C:\TurdMOD\data\leaderboard.json");
                let found = state.get("players").and_then(|p| p.as_object())
                    .and_then(|p| p.values().find(|v| {
                        v.get("name").and_then(|n| n.as_str()).unwrap_or("") == player
                    }));
                match found {
                    Some(s) => {
                        let k = s.get("kills").and_then(|v| v.as_u64()).unwrap_or(0);
                        let d = s.get("deaths").and_then(|v| v.as_u64()).unwrap_or(0);
                        let kd = if d == 0 { k as f64 } else { k as f64 / d as f64 };
                        reply(&format!("[Stats] K:{} D:{} K/D:{:.2}", k, d, kd), &player).await;
                    }
                    None => reply("[Stats] No kills/deaths recorded yet.", &player).await,
                }
                Outcome::Handled
            }

            "!rep" => {
                let state = read_json(r"C:\TurdMOD\data\reputation.json");
                let target = if args.is_empty() { &player } else { &args };
                let found = state.get("players").and_then(|p| p.as_object())
                    .and_then(|p| p.values().find(|v| {
                        v.get("name").and_then(|n| n.as_str()).unwrap_or("").eq_ignore_ascii_case(target)
                    }));
                match found {
                    Some(r) => {
                        let score = r.get("score").and_then(|s| s.as_i64()).unwrap_or(0);
                        let tier = match score {
                            i if i < -50 => "Outlaw",
                            i if i < 0 => "Shady",
                            i if i < 25 => "Neutral",
                            i if i < 75 => "Trusted",
                            i if i < 150 => "Hero",
                            _ => "Legend",
                        };
                        reply(&format!("[Rep] {} - Score: {} ({})", target, score, tier), &player).await;
                    }
                    None => reply(&format!("[Rep] {} - Neutral (no history)", target), &player).await,
                }
                Outcome::Handled
            }

            "!mods" => {
                let mods = ["Economy", "Banking", "Leaderboard", "Duels", "Clans",
                    "Reputation", "Voting", "Gambling", "Lottery", "Trading",
                    "Companions", "Vehicle Ownership", "Teleport", "Quests",
                    "Achievements", "Kits", "Racing", "Radio", "MechMan",
                    "Horde", "Warzone", "Boss Fight", "Airdrop", "Safe Zones",
                    "Jail", "Permissions", "Supply Drops", "NPC Contracts"];
                reply(&format!("[ScummyMap]====={} MODs Installed=====", mods.len()), &player).await;
                for chunk in mods.chunks(4) {
                    let line = chunk.join(" | ");
                    reply(&format!("  {}", line), &player).await;
                }
                reply("==END==", &player).await;
                Outcome::Handled
            }

            "!leaderboard" | "!topkills" => {
                let state = read_json(r"C:\TurdMOD\data\leaderboard.json");
                if let Some(p) = state.get("players").and_then(|p| p.as_object()) {
                    let mut entries: Vec<(&str, u64)> = p.values()
                        .map(|v| {
                            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            let kills = v.get("kills").and_then(|k| k.as_u64()).unwrap_or(0);
                            (name, kills)
                        }).collect();
                    entries.sort_by(|a, b| b.1.cmp(&a.1));
                    entries.truncate(5);
                    let lines: Vec<String> = entries.iter().enumerate()
                        .map(|(i, (n, k))| format!("{}. {} ({}K)", i + 1, n, k)).collect();
                    reply(&format!("[Leaderboard] {}", lines.join(" | ")), &player).await;
                } else {
                    reply("[Leaderboard] No data yet.", &player).await;
                }
                Outcome::Handled
            }

            "!rules" => {
                reply("[Rules] 1. No cheating", &player).await;
                reply("[Rules] 2. No hate speech", &player).await;
                reply("[Rules] 3. English in global chat", &player).await;
                reply("[Rules] 4. Respect admin decisions", &player).await;
                reply("==END==", &player).await;
                Outcome::Handled
            }

            "!ziggy" | "!doc" | "!vera" | "!rust" => {
                let npc_name = match cmd.as_str() {
                    "!ziggy" => "Ziggy",
                    "!doc" | "!vera" => "Doc Vera",
                    "!rust" => "Rust",
                    _ => "Unknown",
                };
                let q = if args.is_empty() {
                    format!("A player named {} just said hello to you", player)
                } else {
                    format!("{} says: {}", player, args)
                };
                let npc = npc_name.to_string();
                let p = player.clone();
                tokio::spawn(async move {
                    let backstory = match npc.as_str() {
                        "Ziggy" => "You are Ziggy, a street-smart arms dealer in a zombie apocalypse. Clipped speech, suspicious of strangers.",
                        "Doc Vera" => "You are Doc Vera, an exhausted ER surgeon in a zombie apocalypse. Clinical, dry wit.",
                        "Rust" => "You are Rust, a paranoid mechanic who talks to his tools. Knows where every car part is.",
                        _ => "You are a survivor.",
                    };
                    let req = crate::ollama::PromptReq {
                        model: Some("scumpilot-fast".into()),
                        prompt: q,
                        system: Some(format!("{} Respond in 1-2 sentences. Stay in character.", backstory)),
                    };
                    match crate::ollama::generate(&req).await {
                        Ok(resp) => {
                            let msg = format!("[{}] {}", npc, resp.response.trim());
                            let params = serde_json::json!({ "text": msg });
                            crate::pipe_rpc::call("broadcastChat", Some(params)).await.ok();
                        }
                        Err(_) => {
                            let params = serde_json::json!({ "text": format!("[{}] ...", npc) });
                            crate::pipe_rpc::call("broadcastChat", Some(params)).await.ok();
                        }
                    }
                });
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}

fn read_json(path: &str) -> serde_json::Value {
    std::fs::read_to_string(path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"players":{},"vehicles":[]}))
}

fn write_json(path: &str, val: &serde_json::Value) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(val) {
        let tmp = format!("{}.tmp", path);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

async fn check_mounted(player: &str) -> Option<String> {
    let params = serde_json::json!({"playerName": player, "classFilter": "Prisoner_C", "radius": 100});
    let resp = pipe_rpc::call("getNearbyActors", Some(params)).await.ok()?;
    let ptr_str = resp.get("actors")?.as_array()?.first()?.get("ptr")?.as_str()?;
    let ptr_val = u64::from_str_radix(ptr_str.trim_start_matches("0x"), 16).ok()?;
    let params = serde_json::json!({"addr": format!("0x{:X}", ptr_val + 7464), "size": "8"});
    let resp = pipe_rpc::call("readMemory", Some(params)).await.ok()?;
    let hex = resp.get("bytesHex")?.as_str()?;
    if hex.len() < 16 { return None; }
    let mut bytes = [0u8; 8];
    for i in 0..8 { bytes[i] = u8::from_str_radix(&hex[i*2..i*2+2], 16).ok()?; }
    let slot = u64::from_le_bytes(bytes);
    if slot == 0 { None } else { Some(format!("0x{:X}", slot)) }
}

async fn find_vehicle_near(player: &str) -> String {
    for cls in &["BPC_Tractor", "BPC_WolfsWagen", "BPC_Laika", "BPC_Rager",
                 "BPC_Cruiser", "BPC_RIS", "BPC_SidecarBike", "BPC_Dirtbike",
                 "BPC_MountainBike", "BPC_CityBike", "BPC_Kinglet_Duster",
                 "BPC_Kinglet_Mariner", "BPC_Dinghy", "BPC_SUP", "BPC_Barba"] {
        let p = serde_json::json!({"playerName": player, "classFilter": cls, "radius": 1000});
        if let Ok(r) = pipe_rpc::call("getNearbyActors", Some(p)).await {
            if r.get("count").and_then(|c| c.as_u64()).unwrap_or(0) > 0 {
                return r.get("actors").and_then(|a| a.as_array())
                    .and_then(|a| a.first()).and_then(|a| a.get("class"))
                    .and_then(|c| c.as_str()).unwrap_or("Vehicle")
                    .replace("BPC_", "").replace("_C", "");
            }
        }
    }
    "Vehicle".to_string()
}

fn set_cooldown(player: &str, action: &str) {
    let mut cd = read_json(COOLDOWN_PATH);
    cd[player][action] = serde_json::json!(now_secs().to_string());
    write_json(COOLDOWN_PATH, &cd);
}

fn coords_to_sector(x: f64, y: f64) -> String {
    let row_letters = ["D", "C", "B", "A", "Z"];
    let cell = 1_524_000.0 / 5.0;
    let col = ((619_200.0 - x) / cell).floor() as usize;
    let row = ((619_200.0 - y) / cell).floor() as usize;
    format!("{}{}", row_letters[row.min(4)], 4 - col.min(4))
}

fn save_vehicle_snapshot(owner: &str, vehicle: &str, snap: &serde_json::Value) {
    let _ = std::fs::create_dir_all(SNAP_DIR);
    let safe = owner.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let path = format!("{}{}__{}.json", SNAP_DIR, safe, vehicle);
    if let Ok(json) = serde_json::to_string_pretty(snap) {
        let tmp = format!("{}.tmp", path);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
            tracing::info!("vehicle snapshot: {}", path);
        }
    }
}

fn load_vehicle_snapshot(owner: &str, vehicle: &str) -> serde_json::Value {
    let safe = owner.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let path = format!("{}{}__{}.json", SNAP_DIR, safe, vehicle);
    std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}

async fn show_carboard(player: &str, rec: &serde_json::Value) {
    let v_name = rec["vehicle"].as_str().unwrap_or("?");
    let (value, vtype) = vehicle_value(v_name);
    let owner = rec["owner"].as_str().unwrap_or("?");
    let orig = rec["original_owner"].as_str().unwrap_or(owner);
    let ptype = match rec["purchase_type"].as_str().unwrap_or("found") {
        "trader" => "New from Trader", "player" => "Player-sold", _ => "Found",
    };
    let insured = if rec["insured"].as_bool().unwrap_or(false) { "Yes" } else { "No" };
    let stolen = rec["stolen_flag"].as_bool().unwrap_or(false);
    let reg = rec["registered_at"].as_str().unwrap_or("?");
    let stl = if stolen { " [STOLEN]" } else { "" };

    let transfers: Vec<String> = rec["transfer_history"].as_array()
        .map(|arr| arr.iter().map(|t| {
            format!("  {} -> {}", t["from"].as_str().unwrap_or("?"), t["to"].as_str().unwrap_or("?"))
        }).collect())
        .unwrap_or_default();
    let xfer = if transfers.is_empty() { "none".into() } else { format!("{} transfer(s)", transfers.len()) };

    let own_str = format!("Owner: {}", owner);
    let orig_str = format!("Original: {} ({})", orig, ptype);
    let ins_str = format!("Insured: {}", insured);
    let val_str = format!("Value: {}c ({})", value, vtype);
    let reg_str = format!("Registered: {}", reg);
    let xfer_str = format!("Transfers: {}", xfer);
    let title = format!("CARBOARD - {}{}", v_name, stl);
    fmt_reply(player, "Carboard", &title, &[&own_str, &orig_str, &ins_str, &val_str, &reg_str, &xfer_str]).await;
    for t in &transfers {
        reply(t, player).await;
    }
}
