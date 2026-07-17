// Supply drops - periodic random supply events announced server-wide.
// ~Every 45 min a drop is announced at a random grid; first player within CLAIM_RADIUS
// (checked via ctx.map) claims the loot, else it expires after a 10-min window.
// Sequential loop -> a small state machine driven by a 10s interval tick.

use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const DROP_INTERVAL: Duration = Duration::from_secs(2700); // 45 min between drops
const CLAIM_WINDOW: Duration = Duration::from_secs(600);   // 10 min to claim
const INITIAL_DELAY: Duration = Duration::from_secs(300);  // 5 min after boot
const CLAIM_RADIUS: f64 = 2000.0; // 20m in UU
const TICK: Duration = Duration::from_secs(10);
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";

const MAP_MIN: f64 = -300000.0;
const MAP_MAX: f64 = 300000.0;

struct ActiveDrop {
    x: f64,
    y: f64,
    grid: String,
    reward_coins: i64,
    items: Vec<&'static str>,
}

const DROP_ITEMS: &[&str] = &[
    "BP_Item_Ammo_762x39_C",
    "BP_Item_Bandage_Military_C",
    "BP_Item_Antibiotics_C",
    "BP_Item_MRE_C",
    "BP_Item_Water_Bottle_C",
];

fn random_coord() -> f64 {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as f64;
    MAP_MIN + (seed % ((MAP_MAX - MAP_MIN) as u128) as f64)
}

fn grid_ref(x: f64, y: f64) -> String {
    let col = ((x - MAP_MIN) / ((MAP_MAX - MAP_MIN) / 8.0)) as u8;
    let row = ((y - MAP_MIN) / ((MAP_MAX - MAP_MIN) / 8.0)) as u8;
    let col_letter = (b'A' + col.min(7)) as char;
    format!("{}{}", col_letter, row.min(7) + 1)
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
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

enum Phase {
    Waiting(Instant),                      // next drop fires at this time
    Active { drop: ActiveDrop, expires: Instant },
}

pub struct SupplyDrops { phase: Mutex<Phase> }
impl SupplyDrops {
    pub fn new() -> Self { Self { phase: Mutex::new(Phase::Waiting(Instant::now() + INITIAL_DELAY)) } }
}

#[async_trait::async_trait]
impl Mod for SupplyDrops {
    fn name(&self) -> &'static str { "supply_drops" }
    fn interval(&self) -> Option<Duration> { Some(TICK) }

    async fn tick(&self, ctx: &ModCtx) {
        // Single ticker, no other accessor of `phase` — safe to hold across the awaits below.
        let mut phase = self.phase.lock().await;
        match &mut *phase {
            Phase::Waiting(at) => {
                if Instant::now() >= *at {
                    let x = random_coord();
                    let y = random_coord();
                    let grid = grid_ref(x, y);
                    broadcast(&format!("[SUPPLY DROP] Incoming at grid {}! First to reach it claims the loot!", grid)).await;
                    *phase = Phase::Active {
                        drop: ActiveDrop { x, y, grid, reward_coins: 150, items: DROP_ITEMS.to_vec() },
                        expires: Instant::now() + CLAIM_WINDOW,
                    };
                }
            }
            Phase::Active { drop, expires } => {
                let snapshot = ctx.map.read().await.clone();
                let mut claimed_by: Option<(String, String)> = None; // (steam, name)
                for p in &snapshot.players {
                    let dx = p.x - drop.x;
                    let dy = p.y - drop.y;
                    if (dx * dx + dy * dy).sqrt() < CLAIM_RADIUS {
                        claimed_by = Some((p.steam_id.clone(), p.name.clone()));
                        break;
                    }
                }
                if let Some((steam, name)) = claimed_by {
                    credit(&steam, drop.reward_coins);
                    for item in &drop.items {
                        let params = serde_json::json!({ "playerName": name, "className": item });
                        pipe_rpc::call("placeItemInInventory", Some(params)).await.ok();
                    }
                    broadcast(&format!("[SUPPLY DROP] {} claimed the drop at {}! +{} coins + {} items!",
                        name, drop.grid, drop.reward_coins, drop.items.len())).await;
                    *phase = Phase::Waiting(Instant::now() + DROP_INTERVAL);
                } else if Instant::now() >= *expires {
                    broadcast(&format!("[SUPPLY DROP] Drop at {} expired - nobody claimed it.", drop.grid)).await;
                    *phase = Phase::Waiting(Instant::now() + DROP_INTERVAL);
                }
            }
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}
