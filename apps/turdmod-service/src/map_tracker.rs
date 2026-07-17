// Live player position tracker — polls bridge every 10s, caches for HTTP reads

use crate::pipe_rpc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerPosition {
    pub name: String,
    #[serde(rename = "steamId")]
    pub steam_id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MapSnapshot {
    pub players: Vec<PlayerPosition>,
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
}

pub type SharedSnapshot = Arc<RwLock<MapSnapshot>>;

pub fn start_tracker(phantom_floor: usize) -> SharedSnapshot {
    let snap: SharedSnapshot = Arc::new(RwLock::new(MapSnapshot::default()));
    let s = snap.clone();
    tokio::spawn(async move {
        let mut tick: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            tick = tick.wrapping_add(1);
            match pipe_rpc::call("getPlayerPositions", None).await {
                Ok(val) => {
                    if let Some(mut players) = parse(&val) {
                        // Pad REPORTED population up to the phantom floor (map snapshot
                        // only — real players counted first, never hidden). @dep phantom.rs
                        let phantoms = crate::phantom::fill(&players, phantom_floor, tick);
                        players.extend(phantoms);
                        let mut w = s.write().await;
                        w.players = players;
                        w.updated_at = now();
                    }
                }
                Err(_) => {}
            }
        }
    });
    snap
}

fn parse(val: &serde_json::Value) -> Option<Vec<PlayerPosition>> {
    let arr = val.get("players")?.as_array()?;
    Some(arr.iter().filter_map(|p| {
        Some(PlayerPosition {
            name: p.get("name")?.as_str()?.into(),
            steam_id: p.get("steamId")?.as_str()?.into(),
            x: p.get("x")?.as_f64()?,
            y: p.get("y")?.as_f64()?,
            z: p.get("z")?.as_f64()?,
        })
    }).collect())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
