//! DIMs — smart-script NPC director. Replaces the old per-line-Ollama loop with deterministic
//! selection over AI-authored character packs + persistent per-player memory + live world data, so
//! characters feel like real, non-repeating people who remember you and can act on the world.
//! Registered on the spine ([[registry]]); event-driven (sees chat/login/kill/death).
//!
//! Runtime is cheap (no LLM per line): match trigger -> roll chance + cooldown -> pick a line NOT
//! recently said to THIS player -> fill slots from memory/world -> send -> optionally act. The
//! richness is authored OFFLINE via the AI-CLI bridge into character packs. @dep [[world]].

pub mod character;
pub mod memory;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::registry::{Mod, ModCtx, Outcome};
use character::CharacterPack;
use memory::MemoryStore;

const CHARS_DIR: &str = r"C:\TurdMOD\npc\characters";

pub struct Dims {
    chars: Vec<CharacterPack>,
    mem: Arc<Mutex<MemoryStore>>,
    cooldowns: Arc<Mutex<HashMap<String, Instant>>>, // key: id|situation|steam
}

impl Dims {
    pub fn new() -> Self {
        Self {
            chars: load_characters(),
            mem: Arc::new(Mutex::new(MemoryStore::load())),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn load_characters() -> Vec<CharacterPack> {
    let dir = std::path::Path::new(CHARS_DIR);
    let _ = std::fs::create_dir_all(dir);
    let mut packs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(e.path()) {
                    if let Ok(p) = serde_json::from_str::<CharacterPack>(&s) { packs.push(p); }
                }
            }
        }
    }
    if packs.is_empty() {
        let rust = character::seed_rust();
        if let Ok(j) = serde_json::to_string_pretty(&rust) {
            let _ = std::fs::write(dir.join("rust.json"), j);
        }
        packs.push(rust);
    }
    tracing::info!("dims: loaded {} character(s): {}", packs.len(),
        packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "));
    packs
}

fn ev_str<'a>(ev: &'a GameEvent, k: &str) -> &'a str {
    ev.data.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

/// Pick a line index not in `recent`; fall back to any if all are recent. Pseudo-random via clock
/// (no rand dep) — combined with the not-recent filter this rotates through the pool.
fn pick_index(n: usize, recent: &[usize]) -> Option<usize> {
    if n == 0 { return None; }
    let avail: Vec<usize> = (0..n).filter(|i| !recent.contains(i)).collect();
    let seed = memory::now_secs() as usize;
    Some(if avail.is_empty() { seed % n } else { avail[seed % avail.len()] })
}

fn fill(template: &str, slots: &HashMap<&str, String>) -> String {
    let mut s = template.to_string();
    for (k, v) in slots { s = s.replace(&format!("{{{}}}", k), v); }
    s
}

impl Dims {
    async fn ingest(&self, ev: &GameEvent) -> bool {
        let steam = ev_str(ev, "steam");
        let player = ev_str(ev, "player");
        if steam.is_empty() { return false; }
        let (kind, detail) = match ev.event.as_str() {
            "login" => ("login", "logged in".to_string()),
            "death" => {
                let by = ev_str(ev, "killer");
                ("death", if by.is_empty() { "died".into() } else { format!("died to {}", by) })
            }
            "kill" => {
                let v = ev_str(ev, "victim");
                ("kill", if v.is_empty() { "got a kill".into() } else { format!("killed {}", v) })
            }
            _ => return false,
        };
        let mut m = self.mem.lock().await;
        m.record(steam, player, kind, &detail);
        m.save();
        true
    }

    async fn react(&self, ev: &GameEvent) -> bool {
        let player = ev_str(ev, "player").to_string();
        let steam = ev_str(ev, "steam").to_string();
        let text_lc = ev_str(ev, "text").to_ascii_lowercase();
        let kind = ev.event.clone();

        for ch in &self.chars {
            if ch.status == "retired" { continue; }
            for (ti, tr) in ch.triggers.iter().enumerate() {
                if tr.on != kind { continue; }
                if tr.on == "chat" {
                    match &tr.phrase {
                        Some(p) if text_lc.contains(&p.to_lowercase()) => {}
                        _ => continue,
                    }
                }
                // chance roll (clock-seeded pseudo-random)
                if tr.chance < 1.0 {
                    let r = (memory::now_secs().wrapping_add(ti as u64) % 100) as f64 / 100.0;
                    if r > tr.chance { continue; }
                }
                // per (character|situation|player) cooldown
                let ckey = format!("{}|{}|{}", ch.id, tr.situation, steam);
                {
                    let mut cd = self.cooldowns.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = cd.get(&ckey) {
                        if now.duration_since(*prev) < Duration::from_secs(tr.cooldown_s) { continue; }
                    }
                    cd.insert(ckey, now);
                }
                // pick a non-repeated line
                let said_key = format!("{}|{}", ch.id, tr.situation);
                let recent = { self.mem.lock().await.recently_said(&steam, &said_key) };
                let Some(idx) = pick_index(tr.lines.len(), &recent) else { continue; };
                // slots from event + memory
                let mut slots: HashMap<&str, String> = HashMap::new();
                slots.insert("player", if player.is_empty() { "survivor".into() } else { player.clone() });
                slots.insert("killer", ev_str(ev, "killer").to_string());
                slots.insert("victim", ev_str(ev, "victim").to_string());
                let fact = {
                    let m = self.mem.lock().await;
                    m.recent(&steam, None, 5).into_iter()
                        .find(|e| e.kind != "login").map(|e| e.detail).unwrap_or_default()
                };
                slots.insert("fact", fact);
                let line = fill(&tr.lines[idx], &slots);
                // deliver
                if tr.scope == "private" && !player.is_empty() {
                    crate::pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({
                        "message": line, "playerName": player, "channel": "4"
                    }))).await.ok();
                } else {
                    crate::pipe_rpc::call("broadcastChat", Some(serde_json::json!({
                        "text": line, "channel": "6"
                    }))).await.ok();
                }
                { self.mem.lock().await.note_said(&steam, &said_key, idx); }
                // optional in-world action (spawn item, etc.)
                if let Some(act) = &tr.action {
                    let mut params = act.params.clone();
                    if params.is_object() && params.get("playerName").is_none() && !player.is_empty() {
                        params["playerName"] = serde_json::json!(player);
                    }
                    crate::pipe_rpc::call(&act.command, Some(params)).await.ok();
                }
                tracing::info!("[dims] {} reacted ({}/{}) to {}", ch.name, tr.situation, idx, kind);
                return true; // one character reaction per event — keeps them from talking over each other
            }
        }
        false
    }
}

#[async_trait::async_trait]
impl Mod for Dims {
    fn name(&self) -> &'static str { "dims" }
    // event-driven (no command claims) -> receives every event and self-filters.
    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        let mut did = false;
        match ev.event.as_str() {
            "login" | "death" | "kill" => {
                did |= self.ingest(ev).await;
                did |= self.react(ev).await;
            }
            "chat" => did |= self.react(ev).await,
            _ => return Outcome::Ignored,
        }
        if did { Outcome::Handled } else { Outcome::Ignored }
    }
}
