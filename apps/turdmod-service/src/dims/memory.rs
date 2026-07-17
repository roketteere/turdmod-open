//! DIMs persistent memory — the factual, per-player history the smart characters draw on so they
//! can call players out on real things ("still driving that Laika you flipped at D7?"). Fed from
//! the event bus (deaths/kills/logins/items), persisted to disk so it survives restarts and a
//! returning/recurring character still remembers you. @dep [[dims]] director reads this.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MEM_PATH: &str = r"C:\TurdMOD\npc\memory.json";

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemEvent {
    pub kind: String,   // "death" | "kill" | "login" | "item" | "chat" | ...
    pub detail: String, // human-readable fact, slot-fillable into lines
    pub at: u64,        // unix secs
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerMemory {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub events: Vec<MemEvent>,
    /// npc_id -> rapport (-100..100). Simple relationship signal the director nudges over time.
    #[serde(default)]
    pub rapport: HashMap<String, i32>,
    /// "npc_id|situation" -> recently-said line indices, so a character never repeats the same
    /// line to the same player back-to-back. Capped per key.
    #[serde(default)]
    pub said: HashMap<String, Vec<usize>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MemoryStore {
    #[serde(default)]
    pub players: HashMap<String, PlayerMemory>, // keyed by steam id
}

impl MemoryStore {
    pub fn load() -> Self {
        std::fs::read_to_string(MEM_PATH).ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = PathBuf::from(MEM_PATH).parent() { let _ = std::fs::create_dir_all(p); }
        if let Ok(j) = serde_json::to_string_pretty(self) { let _ = std::fs::write(MEM_PATH, j); }
    }

    pub fn player_mut(&mut self, steam: &str, name: &str) -> &mut PlayerMemory {
        let pm = self.players.entry(steam.to_string()).or_default();
        if !name.is_empty() { pm.name = name.to_string(); }
        pm
    }

    pub fn record(&mut self, steam: &str, name: &str, kind: &str, detail: &str) {
        if steam.is_empty() { return; }
        let pm = self.player_mut(steam, name);
        pm.events.push(MemEvent { kind: kind.into(), detail: detail.into(), at: now_secs() });
        let len = pm.events.len();
        if len > 120 { pm.events.drain(0..len - 120); } // cap history
    }

    /// Most-recent matching events (newest first). `kind=None` = any kind.
    pub fn recent(&self, steam: &str, kind: Option<&str>, n: usize) -> Vec<MemEvent> {
        self.players.get(steam).map(|pm| {
            pm.events.iter().rev()
                .filter(|e| kind.map_or(true, |k| e.kind == k))
                .take(n).cloned().collect()
        }).unwrap_or_default()
    }

    /// Record + cap the line index a character just said to this player (anti-repetition).
    pub fn note_said(&mut self, steam: &str, key: &str, idx: usize) {
        if steam.is_empty() { return; }
        let pm = self.players.entry(steam.to_string()).or_default();
        let v = pm.said.entry(key.to_string()).or_default();
        v.push(idx);
        let len = v.len();
        if len > 5 { v.drain(0..len - 5); }
    }

    pub fn recently_said(&self, steam: &str, key: &str) -> Vec<usize> {
        self.players.get(steam).and_then(|pm| pm.said.get(key)).cloned().unwrap_or_default()
    }
}
