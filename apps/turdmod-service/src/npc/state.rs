// Per-player NPC relationship persistence

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

const MAX_EVENTS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelEvent {
    pub ts: String,
    pub kind: String,
    pub delta: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcRelation {
    pub score: i32,
    pub tier: String,
    pub total_interactions: u32,
    pub events: Vec<RelEvent>,
    pub last_interaction: String,
}

impl Default for NpcRelation {
    fn default() -> Self {
        Self { score: 30, tier: "neutral".into(), total_interactions: 0, events: Vec::new(), last_interaction: String::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerRelState {
    pub player_name: String,
    pub npcs: HashMap<String, NpcRelation>,
}

pub struct RelationshipDb {
    dir: PathBuf,
    cache: HashMap<String, PlayerRelState>,
}

impl RelationshipDb {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let mut cache = HashMap::new();

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
                if let Ok(raw) = fs::read_to_string(&path) {
                    if let Ok(state) = serde_json::from_str(&raw) {
                        cache.insert(stem, state);
                    }
                }
            }
        }

        tracing::info!("rel_db: loaded {} player files", cache.len());
        Self { dir, cache }
    }

    pub fn tier_for_score(score: i32) -> &'static str {
        match score {
            0..=25 => "enemy",
            26..=50 => "neutral",
            51..=75 => "acquaintance",
            _ => "friend",
        }
    }

    pub fn get_or_create(&mut self, player_id: &str, player_name: &str, npc_id: &str) -> &mut NpcRelation {
        let state = self.cache.entry(player_id.to_string()).or_insert_with(|| PlayerRelState {
            player_name: player_name.to_string(),
            npcs: HashMap::new(),
        });
        state.player_name = player_name.to_string();
        state.npcs.entry(npc_id.to_string()).or_default()
    }

    pub fn update_score(&mut self, player_id: &str, player_name: &str, npc_id: &str, delta: i32, event_kind: &str) {
        let ts = now_ts();
        let rel = self.get_or_create(player_id, player_name, npc_id);
        rel.score = (rel.score + delta).clamp(0, 100);
        rel.tier = Self::tier_for_score(rel.score).to_string();
        rel.total_interactions += 1;
        rel.last_interaction = ts.clone();
        if rel.events.len() >= MAX_EVENTS { rel.events.remove(0); }
        rel.events.push(RelEvent { ts, kind: event_kind.to_string(), delta });
        self.flush(player_id);
    }

    pub fn get_score(&self, player_id: &str, npc_id: &str) -> Option<(i32, String)> {
        let rel = self.cache.get(player_id)?.npcs.get(npc_id)?;
        Some((rel.score, rel.tier.clone()))
    }

    fn flush(&self, player_id: &str) {
        let Some(state) = self.cache.get(player_id) else { return };
        let path = self.dir.join(format!("{}.json", player_id));
        let tmp = self.dir.join(format!("{}.json.tmp", player_id));
        if let Ok(json) = serde_json::to_string_pretty(state) {
            if fs::write(&tmp, &json).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }
}

fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
