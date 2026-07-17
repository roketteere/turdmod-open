//! Live world perception cache — the shared "what's happening in-game right now" that smart-NPC /
//! quest / event mods read instead of each issuing their own bridge polls. One poller folds bridge
//! state into one cache; mods read it through `registry::ModCtx.world`.
//!
//! v1 tracks online players (getOnlinePlayers). The catalog's List*/PlayerInfo/ShowOtherPlayer-
//! Locations admin verbs fold in here next (positions, squads, vehicles) so the NPC brains get a
//! full WorldSnapshot without N redundant polls. @dep [[registry]].

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Default, Clone)]
pub struct WorldData {
    pub online: Vec<String>,
    pub count: u64,
}

#[derive(Default)]
pub struct WorldCache {
    inner: RwLock<WorldData>,
}

impl WorldCache {
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }
    pub async fn snapshot(&self) -> WorldData { self.inner.read().await.clone() }
    pub async fn online(&self) -> Vec<String> { self.inner.read().await.online.clone() }
    async fn set(&self, online: Vec<String>, count: u64) {
        let mut g = self.inner.write().await;
        g.online = online;
        g.count = count;
    }
}

/// Poll the bridge for live world state on an interval, folding it into the cache. Spawn once.
pub fn spawn_world_poll(world: Arc<WorldCache>) {
    tokio::spawn(async move {
        loop {
            if let Ok(r) = crate::pipe_rpc::call("getOnlinePlayers", Some(serde_json::json!({}))).await {
                let online: Vec<String> = r.get("players").and_then(|v| v.as_array())
                    .map(|a| a.iter()
                        .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(String::from))
                        .collect())
                    .unwrap_or_default();
                let count = r.get("count").and_then(|v| v.as_u64()).unwrap_or(online.len() as u64);
                world.set(online, count).await;
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}
