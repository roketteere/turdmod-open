// NPC contracts - Ziggy/Doc/Rust issue random missions for economy rewards.
// !contract (new) / status / abandon. Event-driven (kill progress); 10s tick handles survival + expiry.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const CONTRACT_EXPIRE: Duration = Duration::from_secs(3600); // 1 hour

#[derive(Clone)]
struct Contract {
    npc: String,
    desc: String,
    goal: ContractGoal,
    reward: i64,
    progress: f64,
    started: Instant,
}

#[derive(Clone)]
enum ContractGoal {
    Kills(u32),
    Survive(u64), // seconds
    Travel,
}

const CONTRACT_TEMPLATES: &[(&str, &str, u32, i64)] = &[
    ("Ziggy", "Eliminate 3 hostiles in the area", 3, 150),
    ("Ziggy", "Take down 5 targets for a client", 5, 250),
    ("Ziggy", "Clear 10 threats - big payout", 10, 500),
    ("Doc Vera", "Survive 15 minutes in the field without dying", 0, 100),
    ("Doc Vera", "Stay alive for 30 minutes - I need test subjects", 0, 200),
    ("Rust", "Scout the area - go to 3 locations", 0, 175),
    ("Rust", "Deliver the package - drive to the marked zone", 0, 225),
];

fn pick_contract() -> (String, String, ContractGoal, i64) {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as usize;
    let &(npc, desc, kills, reward) = &CONTRACT_TEMPLATES[seed % CONTRACT_TEMPLATES.len()];
    let goal = if kills > 0 {
        ContractGoal::Kills(kills)
    } else if desc.contains("Survive") || desc.contains("Stay alive") {
        let secs = if desc.contains("30") { 1800 } else { 900 };
        ContractGoal::Survive(secs)
    } else {
        ContractGoal::Travel
    };
    (npc.into(), desc.into(), goal, reward)
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

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct NpcContracts { contracts: Mutex<HashMap<String, Contract>>, rate: Mutex<HashMap<String, Instant>> }
impl NpcContracts {
    pub fn new() -> Self { Self { contracts: Mutex::new(HashMap::new()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for NpcContracts {
    fn name(&self) -> &'static str { "npc_contracts" }
    // event-driven (no commands()): needs `kill` for progress plus !contract chat.
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(10)) }

    // Survival completion + expiry (the old top-of-loop checks).
    async fn tick(&self, _ctx: &ModCtx) {
        let completed: Vec<(String, i64)> = { // (npc, reward) to announce
            let mut c = self.contracts.lock().await;
            let done: Vec<String> = c.iter()
                .filter(|(_, ct)| matches!(ct.goal, ContractGoal::Survive(secs) if ct.started.elapsed().as_secs() >= secs))
                .map(|(s, _)| s.clone())
                .collect();
            let mut out = Vec::new();
            for steam in &done {
                if let Some(ct) = c.remove(steam) {
                    credit(steam, ct.reward);
                    out.push((ct.npc.clone(), ct.reward));
                }
            }
            c.retain(|_, ct| ct.started.elapsed() < CONTRACT_EXPIRE);
            out
        };
        // Original announces survival completion with no player handle; preserve (broadcast-less reply).
        for (npc, reward) in &completed {
            reply(&format!("[{}] Contract complete! +{}c!", npc, reward), "").await;
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_name = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let msg = {
                    let mut c = self.contracts.lock().await;
                    if let Some(ct) = c.get_mut(&killer_steam) {
                        if let ContractGoal::Kills(target) = ct.goal {
                            ct.progress += 1.0;
                            let prog = ct.progress as u32;
                            if prog >= target {
                                let reward = ct.reward;
                                let npc = ct.npc.clone();
                                credit(&killer_steam, reward);
                                c.remove(&killer_steam);
                                Some(format!("[{}] Contract complete! {}/{} kills - +{}c!", npc, prog, target, reward))
                            } else {
                                Some(format!("[{}] Kill tracked! {}/{}", ct.npc, prog, target))
                            }
                        } else { None }
                    } else { None }
                };
                match msg {
                    Some(m) => { reply(&m, &killer_name).await; Outcome::Handled }
                    None => Outcome::Ignored,
                }
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with("!contract") { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now_i = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now_i.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now_i);
                }

                let sub = text.split_whitespace().nth(1).map(|s| s.to_lowercase()).unwrap_or_default();
                let msg = {
                    let mut c = self.contracts.lock().await;
                    match sub.as_str() {
                        "status" | "" => {
                            if sub.is_empty() && !c.contains_key(&steam) {
                                let (npc, desc, goal, reward) = pick_contract();
                                let goal_str = match &goal {
                                    ContractGoal::Kills(n) => format!("Kill {} targets", n),
                                    ContractGoal::Survive(s) => format!("Survive {}min", s / 60),
                                    ContractGoal::Travel => "Reach the destination".to_string(),
                                };
                                let line = format!("[{}] \"{}\" - {} for {}c (1hr limit)", npc, desc, goal_str, reward);
                                c.insert(steam.clone(), Contract { npc, desc, goal, reward, progress: 0.0, started: Instant::now() });
                                line
                            } else if let Some(ct) = c.get(&steam) {
                                let remaining = CONTRACT_EXPIRE.checked_sub(ct.started.elapsed()).map(|d| d.as_secs() / 60).unwrap_or(0);
                                let progress_str = match ct.goal {
                                    ContractGoal::Kills(n) => format!("{}/{} kills", ct.progress as u32, n),
                                    ContractGoal::Survive(s) => format!("{}/{}s survived", ct.started.elapsed().as_secs(), s),
                                    ContractGoal::Travel => "in progress".to_string(),
                                };
                                format!("[{}] {} - {} ({}min left, {}c)", ct.npc, ct.desc, progress_str, remaining, ct.reward)
                            } else {
                                "[Contract] No active contract. Type !contract to get one.".to_string()
                            }
                        }
                        "abandon" => {
                            if c.remove(&steam).is_some() { "[Contract] Abandoned. No reward.".to_string() }
                            else { "[Contract] No active contract.".to_string() }
                        }
                        _ => "[Contract] Commands: (no args = new contract) / status / abandon".to_string(),
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
