// Car Repo — LIVE repossession via DestroyVehicle <entity_id> (zero-admin),
// NO server restart, NO scum.db surgery (replaces the old pre-start cascade
// delete). Runs on the mod tick: destroys expired temp vehicles in-world, then
// updates the registry + public !repos history.
//
// destroy_vehicle() is the REUSABLE vehicle-removal primitive — insurance
// swaps, premium new-vehicle purchases, taxi cleanup, and GC all call it.
// @dep: auto_announce::run_admin_bypass (adminless DestroyVehicle). Needs >=1
//       player online as the executor channel; otherwise repossession defers
//       to a later tick (no DB fallback — that was the restart-era path).
// @inv: only `temp` registrations with expires_at < now are repossessed.

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWN: &str = r"C:\TurdMOD\data\vehicle_ownership.json";
const HIST: &str = r"C:\TurdMOD\data\repo_history.json"; // public !repos history
const TICK: Duration = Duration::from_secs(120); // check for expired temps every 2 min

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Destroy a vehicle by its scum.db entity_id via the live zero-admin
/// DestroyVehicle admin verb. No restart, no DB edit. The reusable
/// vehicle-removal primitive (repo / insurance / purchase / taxi / GC).
/// Returns true if the command dispatched without error. Needs a player online.
pub async fn destroy_vehicle(entity_id: i64) -> bool {
    crate::auto_announce::run_admin_bypass(&format!("DestroyVehicle {}", entity_id)).await
}

async fn bridge_ready() -> bool {
    pipe_rpc::call("ping", None).await.ok()
        .and_then(|v| v.get("pong").and_then(|p| p.as_bool())).unwrap_or(false)
}

/// Live repossession pass: destroy expired temp vehicles via DestroyVehicle,
/// drop them from the registry, append to the public history. Returns count.
async fn repo_expired_live() -> usize {
    let now = now_secs();
    let Ok(data) = std::fs::read_to_string(OWN) else { return 0 };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return 0 };
    let Some(arr) = state.get("vehicles").and_then(|v| v.as_array()).cloned() else { return 0 };

    let expired: Vec<(i64, String, String)> = arr.iter().filter_map(|v| {
        if !v.get("temp").and_then(|t| t.as_bool()).unwrap_or(false) { return None; }
        let exp = v.get("expires_at").and_then(|e| e.as_str()).and_then(|s| s.parse::<u64>().ok())?;
        if exp >= now { return None; }
        let eid = v.get("entity_id").and_then(|e| e.as_i64())?;
        let name = v.get("vehicle").and_then(|n| n.as_str()).unwrap_or("vehicle").to_string();
        let owner = v.get("owner").and_then(|o| o.as_str()).unwrap_or("?").to_string();
        Some((eid, name, owner))
    }).collect();
    if expired.is_empty() { return 0; }

    let mut removed: Vec<i64> = Vec::new();
    let mut hist_add: Vec<serde_json::Value> = Vec::new();
    for (eid, name, owner) in &expired {
        // Live destroy. If no executor (empty server) it returns false -> we
        // leave it in the registry and retry on a later tick.
        if destroy_vehicle(*eid).await {
            removed.push(*eid);
            hist_add.push(serde_json::json!({ "entity_id": eid, "vehicle": name, "owner": owner, "at": now }));
            eprintln!("[car_repo] repossessed expired {} (entity {}) via DestroyVehicle", name, eid);
        }
    }
    if removed.is_empty() { return 0; }

    if let Some(vs) = state.get_mut("vehicles").and_then(|v| v.as_array_mut()) {
        vs.retain(|v| !v.get("entity_id").and_then(|e| e.as_i64())
            .map(|id| removed.contains(&id)).unwrap_or(false));
    }
    if let Ok(j) = serde_json::to_string_pretty(&state) { let _ = std::fs::write(OWN, j); }
    let mut hist: Vec<serde_json::Value> = std::fs::read_to_string(HIST).ok()
        .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    hist.extend(hist_add);
    let n = hist.len();
    if n > 100 { hist.drain(0..n - 100); }
    if let Ok(j) = serde_json::to_string_pretty(&hist) { let _ = std::fs::write(HIST, j); }
    removed.len()
}

pub struct VehicleRepo;
impl VehicleRepo {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Mod for VehicleRepo {
    fn name(&self) -> &'static str { "vehicle_repo" }
    fn interval(&self) -> Option<Duration> { Some(TICK) }

    // LIVE: every tick, repossess any expired temp vehicles in-world (no
    // restart) and announce if any were taken.
    async fn tick(&self, _ctx: &ModCtx) {
        if !bridge_ready().await { return; }
        let n = repo_expired_live().await;
        if n > 0 {
            let msg = format!("\u{1F697} CAR REPO: {} expired temp vehicle(s) repossessed. Keep your garage \u{2264}5 or !transfer in time!", n);
            crate::auto_announce::announce(&msg).await;
            pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": msg, "channel": "1" }))).await.ok();
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}
