// Scavenger hunt - admin hides objectives, players find them for rewards.
// Admin: !hunt create/add/start/end. Players: !hunt find/status. (!scav alias.)
// Command-only; reads ctx.map for the player's position on add/find.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const FIND_RADIUS: f64 = 1500.0; // 15m
const REWARD_PER_FIND: i64 = 100;

#[derive(Clone)]
struct HuntObjective {
    id: u32,
    name: String,
    x: f64,
    y: f64,
    hint: String,
}

#[derive(Clone)]
struct ActiveHunt {
    #[allow(dead_code)]
    name: String,
    objectives: Vec<HuntObjective>,
    found_by: HashMap<u32, String>, // objective_id -> finder name
}

#[derive(Default)]
struct HuntState {
    active: Option<ActiveHunt>,
    building: Vec<HuntObjective>,
    next_id: u32,
}

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

fn credit(steam: &str, amount: i64) {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let bal = state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    state["players"][steam]["balance"] = serde_json::json!(bal + amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

fn grid_ref(x: f64, y: f64) -> String {
    let col = ((x + 400000.0) / 100000.0) as u8;
    let row = ((y + 400000.0) / 100000.0) as u8;
    format!("{}{}", (b'A' + col.min(7)) as char, row.min(7) + 1)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct ScavengerHunt { state: Mutex<HuntState>, rate: Mutex<HashMap<String, Instant>> }
impl ScavengerHunt {
    pub fn new() -> Self {
        Self { state: Mutex::new(HuntState { next_id: 1, ..Default::default() }), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for ScavengerHunt {
    fn name(&self) -> &'static str { "scavenger_hunt" }
    fn commands(&self) -> &'static [&'static str] { &["!hunt", "!scav"] }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!hunt" | "!scav") { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
        match sub.as_str() {
            "create" => {
                if !is_owner(&steam, &player) { reply("[Hunt] Admin only.", &player).await; return Outcome::Handled; }
                self.state.lock().await.building.clear();
                reply("[Hunt] Building hunt. Go to each objective and !hunt add <name> [hint]", &player).await;
                Outcome::Handled
            }
            "add" => {
                if !is_owner(&steam, &player) { reply("[Hunt] Admin only.", &player).await; return Outcome::Handled; }
                let pos = { let snap = ctx.map.read().await; snap.players.iter().find(|p| p.name == player).map(|p| (p.x, p.y)) };
                let Some((px, py)) = pos else { return Outcome::Handled; };
                let hint = if parts.len() > 3 { parts[3..].join(" ") } else { "somewhere on the map".to_string() };
                let msg = {
                    let mut st = self.state.lock().await;
                    let id = st.next_id;
                    let name = parts.get(2).map(|s| s.to_string()).unwrap_or_else(|| format!("obj{}", id));
                    st.building.push(HuntObjective { id, name: name.clone(), x: px, y: py, hint });
                    st.next_id += 1;
                    format!("[Hunt] Objective '{}' added ({} total)", name, st.building.len())
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "start" => {
                if !is_owner(&steam, &player) { reply("[Hunt] Admin only.", &player).await; return Outcome::Handled; }
                let objs = {
                    let mut st = self.state.lock().await;
                    if st.building.is_empty() { None }
                    else {
                        let objs = st.building.clone();
                        st.active = Some(ActiveHunt { name: "Scavenger Hunt".into(), objectives: objs.clone(), found_by: HashMap::new() });
                        st.building.clear();
                        Some(objs)
                    }
                };
                match objs {
                    None => reply("[Hunt] No objectives set.", &player).await,
                    Some(objs) => {
                        broadcast(&format!("[SCAVENGER HUNT] Started! {} objectives to find! {}c each!", objs.len(), REWARD_PER_FIND)).await;
                        for obj in &objs {
                            broadcast(&format!("[Hunt] #{} '{}' - Hint: {} (grid ~{})", obj.id, obj.name, obj.hint, grid_ref(obj.x, obj.y))).await;
                        }
                    }
                }
                Outcome::Handled
            }
            "find" | "check" => {
                let pos = { let snap = ctx.map.read().await; snap.players.iter().find(|p| p.name == player).map(|p| (p.x, p.y)) };
                let Some((px, py)) = pos else { return Outcome::Handled; };
                let mut msgs: Vec<(bool, String)> = Vec::new(); // (is_broadcast, msg)
                let no_hunt;
                {
                    let mut st = self.state.lock().await;
                    if st.active.is_none() {
                        no_hunt = true;
                    } else {
                        no_hunt = false;
                        let complete = {
                            let hunt = st.active.as_mut().unwrap();
                            let mut found_any = false;
                            for obj in &hunt.objectives {
                                if hunt.found_by.contains_key(&obj.id) { continue; }
                                let dx = px - obj.x;
                                let dy = py - obj.y;
                                if (dx * dx + dy * dy).sqrt() < FIND_RADIUS {
                                    hunt.found_by.insert(obj.id, player.clone());
                                    credit(&steam, REWARD_PER_FIND);
                                    msgs.push((true, format!("[Hunt] {} found '{}'! +{} coins! ({}/{} found)",
                                        player, obj.name, REWARD_PER_FIND, hunt.found_by.len(), hunt.objectives.len())));
                                    found_any = true;
                                }
                            }
                            if !found_any { msgs.push((false, "[Hunt] Nothing here. Keep looking!".to_string())); }
                            hunt.found_by.len() == hunt.objectives.len()
                        };
                        if complete {
                            msgs.push((true, "[SCAVENGER HUNT] All objectives found! Hunt complete!".to_string()));
                            st.active = None;
                        }
                    }
                }
                if no_hunt {
                    reply("[Hunt] No active hunt.", &player).await;
                } else {
                    for (is_bc, m) in &msgs {
                        if *is_bc { broadcast(m).await } else { reply(m, &player).await }
                    }
                }
                Outcome::Handled
            }
            "status" => {
                let msg = {
                    let st = self.state.lock().await;
                    match &st.active {
                        Some(hunt) => format!("[Hunt] {} objectives remaining", hunt.objectives.len() - hunt.found_by.len()),
                        None => "[Hunt] No active hunt.".to_string(),
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "end" => {
                if !is_owner(&steam, &player) { reply("[Hunt] Admin only.", &player).await; return Outcome::Handled; }
                let was_active = {
                    let mut st = self.state.lock().await;
                    if st.active.is_some() { st.active = None; true } else { false }
                };
                if was_active { broadcast("[Hunt] Scavenger hunt cancelled by admin.").await; }
                Outcome::Handled
            }
            _ => {
                reply("[Hunt] Commands: find/status | Admin: create/add/start/end", &player).await;
                Outcome::Handled
            }
        }
    }
}
