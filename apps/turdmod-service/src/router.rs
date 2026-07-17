//! The router — the interpreter that executes a parsed `Command` by routing it to the
//! correct bridge. `chat_cmds` parses input into a `Command` and hands it here; this
//! module owns execution + the reply. Phase 2 covers the simple EnginePipe commands;
//! stateful (FileState), AI (Ollama), and server-mgmt (RCON) commands migrate in later
//! phases. `dispatch` returns true if it owns the action, false to fall through to
//! `chat_cmds`' legacy match.

use crate::bridge::{engine, is_admin, reply, Command};

pub async fn dispatch(cmd: &Command) -> bool {
    match cmd.action.as_str() {
        "players"   => { route_players(cmd).await; true }
        "server"    => { route_server(cmd).await; true }
        "day"       => { route_settime(cmd, 8, "Day").await; true }
        "night"     => { route_settime(cmd, 20, "Night").await; true }
        "time"      => { route_time(cmd).await; true }
        "fly"       => { route_fly(cmd).await; true }
        "weather"   => { route_weather(cmd).await; true }
        "storm"     => { route_weather_set(cmd, 1.0, "STORM incoming!").await; true }
        "clear"     => { route_weather_set(cmd, 0.0, "Skies cleared.").await; true }
        "possess"   => { route_possess(cmd).await; true }
        "unpossess" => { route_unpossess(cmd).await; true }
        // "tp" is owned solely by teleport.rs (teleport_loop) — points + built-in
        // traders. The old route_tp here was broken (wrong bridge param) and fired a
        // spurious "X teleported to you" on every !tp. Removed to end the double-handling.
        "spawn"     => { route_spawn(cmd).await; true }
        "stats"     => { route_stats(cmd).await; true }
        "kick"      => { route_rcon_kick(cmd).await; true }
        "ban"       => { route_rcon_ban(cmd).await; true }
        "say" | "announce" => { route_rcon_say(cmd).await; true }
        _ => false,
    }
}

async fn deny(cmd: &Command) {
    reply("[Server] Admin only.", &cmd.player).await;
}

async fn route_players(cmd: &Command) {
    match engine("getOnlinePlayers", serde_json::json!({})).await {
        Ok(resp) => {
            let names: Vec<&str> = resp.get("players").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|p| p.get("name").and_then(|n| n.as_str())).collect())
                .unwrap_or_default();
            let n = names.len();
            let list = if names.is_empty() { "none".into() } else { names.join(", ") };
            reply(&format!("[Server] Players online: {n} — {list}"), &cmd.player).await;
        }
        Err(_) => reply("[Server] Could not fetch players.", &cmd.player).await,
    }
}

async fn route_server(cmd: &Command) {
    match engine("getOnlinePlayers", serde_json::json!({})).await {
        Ok(resp) => {
            let count = resp.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            reply(&format!("[Server] Players: {count} | TurdMOD Engine active"), &cmd.player).await;
        }
        Err(_) => reply("[Server] Status unavailable.", &cmd.player).await,
    }
}

// Snap the in-game clock INSTANTLY via #SetTime <hour> (zero-admin bypass).
// Replaces the old SetTimeSpeed ramp — no gradual transition. @dep run_admin_bypass.
async fn route_settime(cmd: &Command, hour: i64, label: &str) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    if crate::auto_announce::run_admin_bypass(&format!("SetTime {}", hour)).await {
        reply(&format!("[Server] {} set ({:02}:00).", label, hour), &cmd.player).await;
    } else {
        reply("[Server] SetTime failed — need a player online.", &cmd.player).await;
    }
}

async fn route_time(cmd: &Command) {
    match cmd.args.trim().parse::<i64>() {
        Ok(h) if (0..=24).contains(&h) => {
            let label = format!("Time {:02}:00", h);
            route_settime(cmd, h, &label).await;
        }
        _ => reply("[Server] Usage: !time <0-24> (e.g. !time 18)", &cmd.player).await,
    }
}

async fn route_fly(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    let target = if cmd.args.is_empty() { cmd.player.clone() } else { cmd.args.clone() };
    let on = !cmd.args.eq_ignore_ascii_case("off");
    let params = serde_json::json!({ "playerName": target, "enabled": on });
    match engine("setFlyingMode", params).await {
        Ok(_) => reply(&format!("[Server] Flying {} for {}", if on { "ON" } else { "OFF" }, target), &cmd.player).await,
        Err(e) => reply(&format!("[Server] Fly failed: {}", e), &cmd.player).await,
    }
}

async fn route_weather(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    if cmd.args.is_empty() {
        reply("[Server] Usage: !weather <0..1> (0=clear, 1=storm)", &cmd.player).await;
        return;
    }
    match cmd.args.trim().parse::<f64>() {
        Ok(sev) if (0.0..=1.0).contains(&sev) => {
            let _ = engine("setWeather", serde_json::json!({ "severity": sev })).await;
            let _ = engine("forceWeatherSnapshot", serde_json::json!({})).await;
            reply(&format!("[Server] Weather severity set to {:.1}", sev), &cmd.player).await;
        }
        _ => reply("[Server] severity must be 0..1", &cmd.player).await,
    }
}

async fn route_weather_set(cmd: &Command, sev: f64, msg: &str) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    let _ = engine("setWeather", serde_json::json!({ "severity": sev })).await;
    let _ = engine("forceWeatherSnapshot", serde_json::json!({})).await;
    reply(&format!("[Server] {}", msg), &cmd.player).await;
}

async fn route_possess(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    if cmd.args.is_empty() {
        reply("[Server] Usage: !possess <ClassName> (e.g. Dropship, Sentry2)", &cmd.player).await;
        return;
    }
    let params = serde_json::json!({ "playerName": cmd.player, "targetClass": cmd.args });
    match engine("possessActor", params).await {
        Ok(_) => reply(&format!("[Server] You now possess {}", cmd.args), &cmd.player).await,
        Err(e) => reply(&format!("[Server] Possess failed: {}", e), &cmd.player).await,
    }
}

async fn route_unpossess(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    match engine("unpossessActor", serde_json::json!({ "playerName": cmd.player })).await {
        Ok(_) => reply("[Server] Returned to your body.", &cmd.player).await,
        Err(e) => reply(&format!("[Server] Unpossess failed: {}", e), &cmd.player).await,
    }
}


async fn route_spawn(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    if cmd.args.is_empty() {
        reply("[Server] Usage: !spawn <className> (e.g. BP_Kinglet_Duster_C)", &cmd.player).await;
        return;
    }
    let params = serde_json::json!({ "className": cmd.args, "playerName": cmd.player });
    match engine("spawnVehicle", params).await {
        Ok(_) => reply(&format!("[Server] Spawned {} near you.", cmd.args), &cmd.player).await,
        Err(e) => reply(&format!("[Server] Spawn failed: {}", e), &cmd.player).await,
    }
}

async fn route_stats(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    match engine("getServerStats", serde_json::json!({})).await {
        Ok(resp) => {
            let uptime = resp.get("uptimeMinutes").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let players = resp.get("playerCount").and_then(|v| v.as_u64()).unwrap_or(0);
            let fps = resp.get("serverFPS").and_then(|v| v.as_f64()).unwrap_or(0.0);
            reply(&format!("[Stats] Up: {:.0}min | Players: {} | FPS: {:.1}", uptime, players, fps), &cmd.player).await;
        }
        Err(_) => reply("[Stats] Bridge unavailable.", &cmd.player).await,
    }
}

// ── Rcon bridge (Phase 3): BattlEye RCON over UDP via rcon_be ──────────────────
// @inv: BE verbs only (kick/ban/say) — NOT SCUM '#' game-admin (HANDBOOK §2.5).
// @inv: BE kick/ban target a player SLOT NUMBER, not a name — resolve via `players`.

/// Run a BE command, reply with the outcome (+ any RCON output) to the caller.
async fn rcon_exec(cmd: &Command, be_cmd: &str, ok_msg: &str) {
    match crate::rcon_be::exec_cfg(be_cmd).await {
        Ok(resp) => {
            let extra = resp.trim();
            let msg = if extra.is_empty() {
                format!("[Server] {}", ok_msg)
            } else {
                format!("[Server] {} — {}", ok_msg, extra)
            };
            reply(&msg, &cmd.player).await;
        }
        Err(e) => reply(&format!("[Server] RCON failed: {}", e), &cmd.player).await,
    }
}

/// Resolve a name (or already-numeric id) to a BE player slot number via `players`.
/// BE `players` rows: `<#> <ip:port> <ping> <guid(status)> <name…>`.
async fn resolve_player_num(who: &str) -> anyhow::Result<String> {
    if !who.is_empty() && who.chars().all(|c| c.is_ascii_digit()) {
        return Ok(who.to_string());
    }
    let out = crate::rcon_be::exec_cfg("players").await?;
    let want = who.to_lowercase();
    for line in out.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.len() < 5 || !t[0].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let name = t[4..].join(" ").to_lowercase();
        if name == want || name.contains(&want) {
            return Ok(t[0].to_string());
        }
    }
    anyhow::bail!("no online player matching '{}'", who)
}

async fn route_rcon_kick(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    let mut it = cmd.args.splitn(2, ' ');
    let who = it.next().unwrap_or("").trim();
    let reason = it.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("kicked by admin");
    if who.is_empty() {
        reply("[Server] Usage: !kick <player|#> [reason]", &cmd.player).await;
        return;
    }
    match resolve_player_num(who).await {
        Ok(num) => rcon_exec(cmd, &format!("kick {} {}", num, reason), &format!("Kicked {}", who)).await,
        Err(e) => reply(&format!("[Server] {}", e), &cmd.player).await,
    }
}

async fn route_rcon_ban(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    let mut it = cmd.args.splitn(2, ' ');
    let who = it.next().unwrap_or("").trim();
    let reason = it.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("banned by admin");
    if who.is_empty() {
        reply("[Server] Usage: !ban <player|#> [reason] (permanent)", &cmd.player).await;
        return;
    }
    match resolve_player_num(who).await {
        // BE: `ban <#> <minutes> <reason>`, 0 minutes = permanent
        Ok(num) => rcon_exec(cmd, &format!("ban {} 0 {}", num, reason), &format!("Banned {}", who)).await,
        Err(e) => reply(&format!("[Server] {}", e), &cmd.player).await,
    }
}

async fn route_rcon_say(cmd: &Command) {
    if !is_admin(&cmd.steam, &cmd.player) { deny(cmd).await; return; }
    if cmd.args.is_empty() {
        reply("[Server] Usage: !say <message> (broadcast to all)", &cmd.player).await;
        return;
    }
    // BE `say -1 <msg>` broadcasts to every player.
    rcon_exec(cmd, &format!("say -1 {}", cmd.args), "Broadcast sent").await;
}
