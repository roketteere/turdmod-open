// Rotating PvP zones - every server start, rotate WHICH of the 3 rotating zones
// (A4 War Harbour, D4 Zeljava, Z1 Weapons Factory) are PvP. Varies 2/1/0 active,
// never the same twice in a row, cycles through all 8 combos. Applied PRE-START
// (SCUM stopped) by editing the zone's damage/color/name in SCUM.db; announced on
// boot every 1 min for 5 min. @dep: engine::start_server calls apply_next_rotation().
// @inv: the OTHER 3 PvP zones (Airport, Prison, Refinery) stay PvP always.

use std::time::Duration;
use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

// (name-prefix in SCUM.db, display label). bit i of the rotation mask = ZONES[i] on.
const ZONES: [(&str, &str); 3] = [
    ("A4 War Harbour",   "A4 War Harbour"),
    ("Zeljava Airfield", "D4 Zeljava"),
    ("Weapons Factory",  "Z1 Weapons Factory"),
];
// rotation over the 8 subsets (bit set = PvP). Varied counts, no consecutive repeat.
const SEQ: [u8; 8] = [0b110, 0b001, 0b000, 0b111, 0b010, 0b101, 0b100, 0b011];
const STATE: &str = r"C:\TurdMOD\data\pvp_rotation.json";
const DB: &str = r"C:\SCUMServer\SCUM\Saved\SaveFiles\SCUM.db";
// Announce once per boot: 5 announcements 1 min apart; interval drives the tick.
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(60);

fn load_idx() -> usize {
    std::fs::read_to_string(STATE).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("idx").and_then(|x| x.as_u64()))
        .map(|n| n as usize).unwrap_or(0)
}
fn save(idx: usize, active: &[String]) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    let v = serde_json::json!({ "idx": idx, "active": active });
    if let Ok(j) = serde_json::to_string_pretty(&v) { let _ = std::fs::write(STATE, j); }
}
fn load_active() -> Vec<String> {
    std::fs::read_to_string(STATE).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("active").and_then(|a| a.as_array().cloned()))
        .map(|a| a.into_iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

// PRE-START hook: advance the rotation, edit SCUM.db zones, persist the active set.
pub fn apply_next_rotation() {
    let idx = load_idx();
    let mask = SEQ[idx % SEQ.len()];
    let next = (idx + 1) % SEQ.len();
    let conn = match rusqlite::Connection::open(DB) { Ok(c) => c, Err(_) => return };
    let mut active: Vec<String> = Vec::new();
    for (i, (base, label)) in ZONES.iter().enumerate() {
        let on = (mask >> i) & 1 == 1;
        set_zone(&conn, base, on);
        if on { active.push(label.to_string()); }
    }
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    save(next, &active);
    eprintln!("[pvp_rotation] mask={:03b} active={:?}", mask, active);
}

fn set_zone(conn: &rusqlite::Connection, base: &str, on: bool) {
    let like = format!("{}%", base);
    let cid: Option<i64> = conn.query_row(
        "SELECT id FROM custom_zone_configuration WHERE name LIKE ?1 LIMIT 1",
        rusqlite::params![like], |r| r.get(0)).ok();
    let Some(cid) = cid else { return };
    let allow: i64 = { let mut v = 0i64; for ch in 0..15 { v |= 1 << (ch * 2); } v };
    for at in [1i64, 2, 9, 11] {            // Player, Puppet, BaseBuilding, Vehicle
        let _ = conn.execute(
            "UPDATE custom_zone_configuration_damage_handling_methods SET damage_handling_methods=?1 \
             WHERE custom_zone_configuration_id=?2 AND damage_actor_type=?3",
            rusqlite::params![if on { allow } else { 0 }, cid, at]);
    }
    let (r, g, b, suf) = if on { (0.80, 0.08, 0.11, " - PVP ACTIVE / NO BUILD ZONE") }
                         else   { (0.32, 0.78, 0.08, " - SAFE THIS CYCLE / NO BUILD ZONE") };
    let name = format!("{}{}", base, suf);
    let _ = conn.execute(
        "UPDATE custom_zone_configuration SET color_red=?1,color_green=?2,color_blue=?3,name=?4 WHERE id=?5",
        rusqlite::params![r, g, b, name, cid]);
    let _ = conn.execute(
        "UPDATE custom_zone_region SET name=?1 WHERE name LIKE ?2",
        rusqlite::params![name, like]);
}

async fn bridge_ready() -> bool {
    pipe_rpc::call("ping", None).await.ok()
        .and_then(|v| v.get("pong").and_then(|p| p.as_bool())).unwrap_or(false)
}

fn rotation_msg() -> String {
    let active = load_active();
    if active.is_empty() {
        "[ScummyMap] PvP ROTATION: all 3 rotating zones are SAFE this cycle (no rotating PvP).".to_string()
    } else {
        format!("[ScummyMap] PvP ACTIVE this cycle: {} - fight & raid there! The other rotating zones are safe.", active.join(", "))
    }
}

pub struct PvpRotation { announced: tokio::sync::Mutex<bool> }
impl PvpRotation {
    pub fn new() -> Self { Self { announced: tokio::sync::Mutex::new(false) } }
}

#[async_trait::async_trait]
impl Mod for PvpRotation {
    fn name(&self) -> &'static str { "pvp_rotation" }
    // event-driven (no commands()): announces the current rotation to each player on login.
    fn interval(&self) -> Option<Duration> { Some(ANNOUNCE_INTERVAL) }

    // Announce the rotation ONCE on boot (broadcast, for players already online) — NOT every 60s.
    // (The old loop did a 5-shot boot burst; the bad conversion made it broadcast forever => spam.)
    async fn tick(&self, _ctx: &ModCtx) {
        if !bridge_ready().await { return; }
        { let mut a = self.announced.lock().await; if *a { return; } *a = true; }
        let active = load_active();
        let msg = rotation_msg();
        pipe_rpc::call("sendHudMessage", Some(serde_json::json!({ "text": msg }))).await.ok();
        pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": msg, "channel": "1" }))).await.ok();
        if !active.is_empty() {
            pipe_rpc::call("broadcastRaidBanner", Some(serde_json::json!({ "kind": "allowed" }))).await.ok();
        }
    }

    // Tell each joining player the current PvP zones once, privately (no broadcast spam).
    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "login" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }
        let msg = rotation_msg();
        pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({ "message": msg, "playerName": player, "channel": "1" }))).await.ok();
        Outcome::Handled
    }
}
