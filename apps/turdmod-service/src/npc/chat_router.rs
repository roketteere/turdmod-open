// NPC chat routing — dispatches Local chat to NPCs, manages conversations

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::events::GameEvent;
use crate::npc::config::{NpcConfig, NpcDef};
use crate::npc::ollama_queue::OllamaQueue;
use crate::npc::prompt::{build_system_prompt, build_user_prompt};
use crate::npc::state::RelationshipDb;
use crate::pipe_rpc;

const RATE_LIMIT: Duration = Duration::from_secs(5);
const CONV_TTL: Duration = Duration::from_secs(90);
const MAX_HISTORY: usize = 6;

const POS_WORDS: &[&str] = &["thanks", "thank", "help", "friend", "love", "appreciate", "good", "nice", "cool"];
const NEG_WORDS: &[&str] = &["hate", "kill", "die", "stupid", "idiot", "shut", "ugly", "threat"];

struct ConvHandle {
    npc_id: String,
    last_msg: Instant,
    history: VecDeque<(bool, String)>, // (is_player, text)
}

pub struct ChatRouter {
    convs: HashMap<String, ConvHandle>,
    rate: HashMap<String, Instant>,
}

impl ChatRouter {
    pub fn new() -> Self {
        Self { convs: HashMap::new(), rate: HashMap::new() }
    }

    pub async fn handle_chat(
        &mut self,
        ev: &GameEvent,
        cfg: &NpcConfig,
        db: &mut RelationshipDb,
        ollama: &OllamaQueue,
    ) {
        self.cleanup();

        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let channel = ev.data.get("channel").and_then(|v| v.as_str()).unwrap_or("");

        if channel != "Local" || player.is_empty() { return; }

        // !rep meta-command
        if let Some(name) = text.strip_prefix("!rep ") {
            let name = name.trim();
            let npc = cfg.npcs.iter().find(|n| n.id.eq_ignore_ascii_case(name) || n.display_name.eq_ignore_ascii_case(name));
            let pid = if steam.is_empty() { &player } else { &steam };
            let msg = match npc {
                Some(n) => match db.get_score(pid, &n.id) {
                    Some((score, tier)) => format!("[ScummyMap] Standing with {}: {} ({}/100)", n.display_name, tier, score),
                    None => format!("[ScummyMap] No history with {}", n.display_name),
                },
                None => format!("[ScummyMap] Unknown NPC: {}", name),
            };
            let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
            pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
            return;
        }

        // Allow !npc_name commands (strip the ! for trigger matching)
        // Skip other ! commands that aren't DIM triggers
        if text.starts_with('!') {
            let stripped = &text[1..];
            if cfg.find_by_trigger(stripped).is_none() { return; }
        }

        // Rate limit
        let now = Instant::now();
        let pid = if steam.is_empty() { player.clone() } else { steam.clone() };
        if let Some(prev) = self.rate.get(&pid) {
            if now.duration_since(*prev) < RATE_LIMIT { return; }
        }
        self.rate.insert(pid.clone(), now);

        // Resolve NPC
        let npc = self.resolve_npc(&text, &pid, cfg);
        let Some(npc) = npc else { return };

        let npc_id = npc.id.clone();
        let npc_name = npc.display_name.clone();

        // Get relationship
        let rel = db.get_or_create(&pid, &player, &npc_id);
        let score = rel.score;
        let tier = rel.tier.clone();

        // Build prompt + generate
        let system = build_system_prompt(npc, &player, score, &tier);
        let user = build_user_prompt(&player, &text);
        let response = ollama.generate(&system, &user).await;

        // Update conversation
        let handle = self.convs.entry(pid.clone()).or_insert_with(|| ConvHandle {
            npc_id: npc_id.clone(), last_msg: now, history: VecDeque::new(),
        });
        handle.npc_id = npc_id.clone();
        handle.last_msg = now;
        handle.history.push_back((true, text.clone()));
        handle.history.push_back((false, response.clone()));
        while handle.history.len() > MAX_HISTORY { handle.history.pop_front(); }

        // Send NPC reply (white local broadcast — everyone nearby hears)
        let msg = format!("[{}] {}", npc_name, response);
        let params = serde_json::json!({ "text": msg, "channel": "0" });
        pipe_rpc::call("broadcastChat", Some(params)).await.ok();
        tracing::info!("npc: {} → [{}]: {}", player, npc_name, response);

        // Sentiment scoring (keyword-based, no AI)
        let delta = sentiment(&text);
        if delta != 0 {
            db.update_score(&pid, &player, &npc_id, delta, if delta > 0 { "positive_chat" } else { "negative_chat" });
        }
    }

    fn resolve_npc<'a>(&self, text: &str, pid: &str, cfg: &'a NpcConfig) -> Option<&'a NpcDef> {
        // Trigger match first
        if let Some(npc) = cfg.find_by_trigger(text) {
            return Some(npc);
        }
        // Active conversation continuation
        if let Some(h) = self.convs.get(pid) {
            if h.last_msg.elapsed() < CONV_TTL {
                return cfg.npcs.iter().find(|n| n.id == h.npc_id);
            }
        }
        None
    }

    fn cleanup(&mut self) {
        self.convs.retain(|_, h| h.last_msg.elapsed() < CONV_TTL);
        self.rate.retain(|_, t| t.elapsed() < Duration::from_secs(300));
    }
}

fn sentiment(text: &str) -> i32 {
    let lower = text.to_lowercase();
    let pos = POS_WORDS.iter().any(|w| lower.contains(w));
    let neg = NEG_WORDS.iter().any(|w| lower.contains(w));
    match (pos, neg) {
        (true, false) => 2,
        (false, true) => -3,
        _ => 0,
    }
}
