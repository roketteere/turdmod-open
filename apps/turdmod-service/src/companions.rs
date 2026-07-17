// Companion system - !tame !companion !dismiss. Tames nearest wild animal (AI -> Timid).
// Follow ticker (15s) keeps tamed animals passive near owner.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const FOLLOW_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct Companion {
    animal_class: String,
    animal_ptr: String,
    ai_ctrl_ptr: String,
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn follow_tick(companions: &HashMap<String, Companion>) {
    for (_owner, comp) in companions {
        if !comp.ai_ctrl_ptr.is_empty() && comp.ai_ctrl_ptr != "0x0" {
            let params = serde_json::json!({ "ptr": comp.ai_ctrl_ptr, "propertyName": "Agressivness", "value": "0", "valueKind": "byte" });
            pipe_rpc::call("writeActorProperty", Some(params)).await.ok();
        }
    }
}

pub struct Companions {
    companions: Mutex<HashMap<String, Companion>>,
    follow_companions: std::sync::Arc<Mutex<HashMap<String, Companion>>>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl Companions {
    pub fn new() -> Self {
        let follow_companions = std::sync::Arc::new(Mutex::new(HashMap::<String, Companion>::new()));
        let fc = follow_companions.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(FOLLOW_INTERVAL).await;
                let comps = fc.lock().await.clone();
                if !comps.is_empty() { follow_tick(&comps).await; }
            }
        });
        Self { companions: Mutex::new(HashMap::new()), follow_companions, rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Companions {
    fn name(&self) -> &'static str { "companions" }
    fn commands(&self) -> &'static [&'static str] { &["!tame", "!companion", "!dismiss"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let rk = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rk) { if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; } }
            rate.insert(rk.clone(), now);
        }
        let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
        match cmd.as_str() {
            "!tame" => {
                if self.companions.lock().await.contains_key(&rk) { reply("[Companion] You already have one. !dismiss first.", &player).await; return Outcome::Handled; }
                match pipe_rpc::call("tameNearbyAnimal", Some(serde_json::json!({ "playerName": player, "radius": 3000 }))).await {
                    Ok(resp) => {
                        let cls = resp.get("animalClass").and_then(|v| v.as_str()).unwrap_or("Animal").to_string();
                        let ptr = resp.get("animalPtr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let ai = resp.get("aiControllerPtr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let dist = resp.get("distance").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let comp = Companion { animal_class: cls.clone(), animal_ptr: ptr.clone(), ai_ctrl_ptr: ai.clone() };
                        self.companions.lock().await.insert(rk.clone(), comp.clone());
                        self.follow_companions.lock().await.insert(player.clone(), comp);
                        reply(&format!("[Companion] Tamed a {} ({:.0}m away)! It's now passive.", cls, dist / 100.0), &player).await;
                    }
                    Err(e) => reply(&format!("[Companion] No animals nearby: {}", e), &player).await,
                }
                Outcome::Handled
            }
            "!companion" => {
                let msg = { self.companions.lock().await.get(&rk).map(|c| format!("[Companion] Your {} ({})", c.animal_class, c.animal_ptr)) };
                match msg { Some(m) => reply(&m, &player).await, None => reply("[Companion] No companion. Walk near a wild animal and type !tame", &player).await }
                Outcome::Handled
            }
            "!dismiss" => {
                let removed = self.companions.lock().await.remove(&rk);
                if let Some(c) = removed {
                    self.follow_companions.lock().await.remove(&player);
                    if !c.ai_ctrl_ptr.is_empty() && c.ai_ctrl_ptr != "0x0" {
                        pipe_rpc::call("writeActorProperty", Some(serde_json::json!({ "ptr": c.ai_ctrl_ptr, "propertyName": "Agressivness", "value": "1", "valueKind": "byte" }))).await.ok();
                    }
                    reply(&format!("[Companion] {} released back to the wild.", c.animal_class), &player).await;
                } else { reply("[Companion] Nothing to dismiss.", &player).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
