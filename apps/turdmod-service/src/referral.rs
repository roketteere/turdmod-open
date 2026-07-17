// Referral system - rewards for inviting new players.
// !refer <code> / !mycode. Referrer gets 200c per new player, referee gets 50c. Command-only.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\referrals.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const REFERRER_REWARD: i64 = 200;
const REFEREE_REWARD: i64 = 50;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct ReferralData {
    name: String,
    code: String,
    referrals: Vec<String>,
    referred_by: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ReferralState {
    players: HashMap<String, ReferralData>, // steam -> data
}

fn load() -> ReferralState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &ReferralState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
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

fn generate_code(steam: &str) -> String {
    let hash = steam.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    format!("TM{:06X}", hash % 0xFFFFFF)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Referral { state: Mutex<ReferralState>, rate: Mutex<HashMap<String, Instant>> }
impl Referral {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Referral {
    fn name(&self) -> &'static str { "referral" }
    fn commands(&self) -> &'static [&'static str] { &["!refer", "!mycode"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
        if !matches!(cmd.as_str(), "!refer" | "!mycode") { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        match cmd.as_str() {
            "!mycode" => {
                let (code, count) = {
                    let mut st = self.state.lock().await;
                    let entry = st.players.entry(steam.clone()).or_insert_with(|| ReferralData {
                        name: player.clone(), code: generate_code(&steam), referrals: vec![], referred_by: None,
                    });
                    entry.name = player.clone();
                    let r = (entry.code.clone(), entry.referrals.len());
                    save(&st);
                    r
                };
                reply(&format!("[Referral] Your code: {} - Share it! You get {}c per new player.", code, REFERRER_REWARD), &player).await;
                reply(&format!("[Referral] {} players referred so far.", count), &player).await;
                Outcome::Handled
            }
            "!refer" => {
                let code = text.split_whitespace().nth(1).unwrap_or("").to_uppercase();
                if code.is_empty() { reply("[Referral] Usage: !refer <code> (get a code from a friend)", &player).await; return Outcome::Handled; }

                // Ok((ref_steam, ref_name, total)) applies the reward; Err(reply) otherwise.
                let outcome: Result<(String, String, usize), String> = {
                    let mut st = self.state.lock().await;
                    if st.players.get(&steam).map(|e| e.referred_by.is_some()).unwrap_or(false) {
                        Err("[Referral] You've already used a referral code.".to_string())
                    } else {
                        let referrer = st.players.iter().find(|(_, d)| d.code == code).map(|(s, d)| (s.clone(), d.name.clone()));
                        match referrer {
                            None => Err("[Referral] Invalid code.".to_string()),
                            Some((ref_steam, _)) if ref_steam == steam => Err("[Referral] Can't refer yourself.".to_string()),
                            Some((ref_steam, ref_name)) => {
                                {
                                    let my = st.players.entry(steam.clone()).or_insert_with(|| ReferralData {
                                        name: player.clone(), code: generate_code(&steam), referrals: vec![], referred_by: None,
                                    });
                                    my.referred_by = Some(ref_name.clone());
                                }
                                if let Some(re) = st.players.get_mut(&ref_steam) { re.referrals.push(player.clone()); }
                                save(&st);
                                let total = st.players.get(&ref_steam).map(|e| e.referrals.len()).unwrap_or(0);
                                Ok((ref_steam, ref_name, total))
                            }
                        }
                    }
                };
                match outcome {
                    Err(msg) => reply(&msg, &player).await,
                    Ok((ref_steam, ref_name, total)) => {
                        credit(&ref_steam, REFERRER_REWARD);
                        credit(&steam, REFEREE_REWARD);
                        reply(&format!("[Referral] Referred by {}! +{}c bonus!", ref_name, REFEREE_REWARD), &player).await;
                        reply(&format!("[Referral] {} used your code! +{}c! ({} total referrals)", player, REFERRER_REWARD, total), &ref_name).await;
                    }
                }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
