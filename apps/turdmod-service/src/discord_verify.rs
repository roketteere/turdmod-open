// Discord verification — link in-game SCUM character <-> Discord, auto-assign the
// Verified role. Discord-initiated: the bot's /link slash command calls
// POST /links/issue (server.rs) -> issue() mints a short code. The player types
// `!link <code>` in-game (chat_cmds.rs) -> redeem() writes the link + the service
// assigns the Verified Discord role directly via the bot token (no bot round-trip).
// @inv: bot token / guild / verified-role id live in discord.json (NOT in the repo).
// @dep: chat_cmds::"!link", server::handle_links_issue.

use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const PENDING: &str = r"C:\TurdMOD\data\link_pending.json"; // {CODE: {discord_id,name,issued}}
const LINKS: &str = r"C:\TurdMOD\data\discord_links.json";   // {by_steam:{}, by_discord:{}}
const CFG: &str = r"C:\TurdMOD\data\discord.json";           // {bot_token, guild_id, verified_role_id}
const TTL: u64 = 900; // codes valid 15 min
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // no 0/O/1/I/L confusion

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }
fn read(p: &str) -> Value {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}))
}
fn write(p: &str, v: &Value) {
    if let Ok(j) = serde_json::to_string_pretty(v) {
        let tmp = format!("{}.tmp", p);
        if std::fs::write(&tmp, &j).is_ok() { let _ = std::fs::rename(&tmp, p); }
    }
}

fn gen_code(seed: &str) -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let h = seed.bytes().fold(0u128, |a, b| a.wrapping_mul(131).wrapping_add(b as u128));
    let mut x = n ^ h.rotate_left(17);
    let mut s = String::new();
    for _ in 0..6 { s.push(ALPHABET[(x % ALPHABET.len() as u128) as usize] as char); x /= ALPHABET.len() as u128; }
    s
}

// /link (Discord) -> a fresh code bound to this discord user.
pub fn issue(discord_id: &str, name: &str) -> String {
    let mut p = read(PENDING);
    if !p.is_object() { p = json!({}); }
    // prune expired so the file doesn't grow unbounded
    if let Some(obj) = p.as_object_mut() {
        let n = now();
        obj.retain(|_, v| n.saturating_sub(v.get("issued").and_then(|x| x.as_u64()).unwrap_or(0)) <= TTL);
    }
    let code = gen_code(discord_id);
    p[&code] = json!({ "discord_id": discord_id, "name": name, "issued": now() });
    write(PENDING, &p);
    code
}

// !link <code> (in-game) -> validate + persist the link. Returns the discord_id.
pub fn redeem(code: &str, steam: &str, name: &str) -> Result<String, String> {
    let code = code.trim().to_uppercase();
    let mut p = read(PENDING);
    let entry = p.get(&code).cloned().ok_or("invalid code — run /link in Discord first")?;
    let issued = entry.get("issued").and_then(|x| x.as_u64()).unwrap_or(0);
    if now().saturating_sub(issued) > TTL { return Err("code expired — run /link again".into()); }
    let discord_id = entry.get("discord_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if discord_id.is_empty() { return Err("bad code".into()); }

    let mut links = read(LINKS);
    if !links.get("by_steam").map(|v| v.is_object()).unwrap_or(false) { links["by_steam"] = json!({}); }
    if !links.get("by_discord").map(|v| v.is_object()).unwrap_or(false) { links["by_discord"] = json!({}); }
    links["by_steam"][steam] = json!({ "discord_id": discord_id, "name": name, "linked_at": now(), "tier": "verified" });
    links["by_discord"][&discord_id] = json!(steam);
    write(LINKS, &links);

    if let Some(obj) = p.as_object_mut() { obj.remove(&code); }
    write(PENDING, &p);
    Ok(discord_id)
}

pub fn is_linked_steam(steam: &str) -> bool {
    read(LINKS).get("by_steam").and_then(|s| s.get(steam)).is_some()
}

// Assign the Verified role via the Discord REST API using the bot token in discord.json.
pub async fn assign_verified(discord_id: &str) -> Result<(), String> {
    let cfg = read(CFG);
    let token = cfg.get("bot_token").and_then(|v| v.as_str()).ok_or("discord.json: no bot_token")?;
    let guild = cfg.get("guild_id").and_then(|v| v.as_str()).ok_or("discord.json: no guild_id")?;
    let role = cfg.get("verified_role_id").and_then(|v| v.as_str()).ok_or("discord.json: no verified_role_id")?;
    let url = format!("https://discord.com/api/v10/guilds/{}/members/{}/roles/{}", guild, discord_id, role);
    let res = reqwest::Client::new()
        .put(&url)
        .header("Authorization", format!("Bot {}", token))
        .header("User-Agent", "DiscordBot (https://scummymap.com, 1.0)")
        .header("Content-Length", "0")
        .send().await.map_err(|e| e.to_string())?;
    let st = res.status();
    if st.is_success() { Ok(()) } else { Err(format!("Discord API {}", st.as_u16())) }
}
