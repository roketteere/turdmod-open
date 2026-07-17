// Player analytics — position history, session tracking, death locations.
// Writes JSONL files to data/analytics/ for offline heatmap generation.
// Reads map_tracker snapshots (via ctx.map) on a 60s tick + records game events.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::registry::{Mod, ModCtx, Outcome};

fn analytics_dir() -> PathBuf {
    let base = std::env::var("TURDMOD_DATA_DIR")
        .unwrap_or_else(|_| "data".into());
    let dir = PathBuf::from(base).join("analytics");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn now_iso() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", d)
}

pub struct Analytics {
    positions_path: PathBuf,
    events_path: PathBuf,
    last_positions: Mutex<HashMap<String, (f64, f64, f64)>>,
}
impl Analytics {
    pub fn new() -> Self {
        let dir = analytics_dir();
        Self {
            positions_path: dir.join("positions.jsonl"),
            events_path: dir.join("events.jsonl"),
            last_positions: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Analytics {
    fn name(&self) -> &'static str { "analytics" }
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(60)) }

    // Sample positions every 60s (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        let snapshot = ctx.map.read().await.clone();
        let mut last = self.last_positions.lock().await;
        for p in &snapshot.players {
            let moved = match last.get(&p.name) {
                Some((lx, ly, lz)) => {
                    let dx = p.x - lx;
                    let dy = p.y - ly;
                    let dz = p.z - lz;
                    (dx * dx + dy * dy + dz * dz).sqrt() > 50.0
                }
                None => true,
            };
            if moved {
                last.insert(p.name.clone(), (p.x, p.y, p.z));
                let line = serde_json::json!({
                    "t": now_iso(), "type": "position",
                    "player": p.name, "steam": p.steam_id,
                    "x": p.x, "y": p.y, "z": p.z
                });
                append_jsonl(&self.positions_path, &line);
            }
        }
    }

    // Record kill/death/login/logout (the old select! event branch).
    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" | "death" | "login" | "logout" => {
                let line = serde_json::json!({ "t": now_iso(), "type": ev.event, "data": ev.data });
                append_jsonl(&self.events_path, &line);
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}

fn append_jsonl(path: &PathBuf, val: &serde_json::Value) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(line) = serde_json::to_string(val) {
            writeln!(f, "{}", line).ok();
        }
    }
}
