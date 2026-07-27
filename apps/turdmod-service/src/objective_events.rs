// Objective events - recurring server-wide kill challenges.
// Auto-starts on an interval; "FIRST to N kills wins R coins". Tracks kills via the
// event bus, first to target wins + coin reward. Adds hype/competition.
// Commands: !obj / !objective (status; on|off admin), !startobj (admin), !endobj (admin).
// @dep: events.rs (GameEvent kill: killerSteam/killer), economy.json (players[steam].balance)

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const CFG_PATH: &str = r"C:\TurdMOD\data\objectives.json";
const DURATION: Duration = Duration::from_secs(15 * 60); // 15 min to complete
const RATE_LIMIT: Duration = Duration::from_secs(3);
const FLAVORS: &[&str] = &["Bloodbath", "Culling", "Reaping", "Rampage", "Massacre", "Purge"];
const TICK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(serde::Serialize, serde::Deserialize)]
struct ObjCfg {
    #[serde(default = "d_true")]
    enabled: bool,
    #[serde(default = "d_interval")]
    interval_minutes: u64,
}
fn d_true() -> bool { true }
fn d_interval() -> u64 { 30 }
impl Default for ObjCfg {
    fn default() -> Self { Self { enabled: true, interval_minutes: 30 } }
}

fn load_cfg() -> ObjCfg {
    std::fs::read_to_string(CFG_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
fn save_cfg(cfg: &ObjCfg) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let tmp = format!("{}.tmp", CFG_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, CFG_PATH); }
    }
}

struct Challenge {
    name: String,
    target: u32,
    reward: i64,
    progress: HashMap<String, u32>,       // steam -> kills since start
    leader_name: HashMap<String, String>, // steam -> display name
    started: Instant,
}

fn seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}
fn rand_in(s: u64, lo: u64, hi: u64) -> u64 {
    let mut x = s ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13; x ^= x >> 7; x ^= x << 17;
    lo + (x % (hi - lo + 1))
}

fn new_challenge() -> Challenge {
    let s = seed();
    let name = FLAVORS[rand_in(s, 0, (FLAVORS.len() - 1) as u64) as usize].to_string();
    let target = rand_in(s.wrapping_add(1), 5, 12) as u32;
    let reward = (rand_in(s.wrapping_add(2), 6, 12) * 50) as i64; // 300..=600
    Challenge { name, target, reward, progress: HashMap::new(), leader_name: HashMap::new(), started: Instant::now() }
}

fn credit(steam: &str, amount: i64) {
    let path = r"C:\TurdMOD\data\economy.json";
    let Ok(data) = std::fs::read_to_string(path) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    if let Some(bal) = state.get_mut("players")
        .and_then(|p| p.get_mut(steam))
        .and_then(|p| p.get_mut("balance"))
        .and_then(|b| b.as_i64())
    {
        state["players"][steam]["balance"] = serde_json::json!(bal + amount);
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let tmp = format!("{}.tmp", path);
            if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, path); }
        }
    }
}

async fn bcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}
async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}
fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

struct ObjState {
    cfg: ObjCfg,
    active: Option<Challenge>,
    rate: HashMap<String, Instant>,
    next_start: Instant,
}

pub struct ObjectiveEvents {
    state: Mutex<ObjState>,
}

impl ObjectiveEvents {
    pub fn new() -> Self {
        let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
        let cfg = load_cfg();
        let next_start = Instant::now() + Duration::from_secs(cfg.interval_minutes * 60);
        Self {
            state: Mutex::new(ObjState { cfg, active: None, rate: HashMap::new(), next_start }),
        }
    }
}

#[async_trait::async_trait]
impl Mod for ObjectiveEvents {
    fn name(&self) -> &'static str { "objective_events" }
    // event-driven: kill + all chat (admin commands + any player !obj status query)
    fn interval(&self) -> Option<Duration> { Some(TICK_INTERVAL) }

    // Replaces the old select! interval branch: expire active challenge, auto-start new one.
    // @inv: collect broadcast strings under lock, send after dropping lock
    async fn tick(&self, _ctx: &ModCtx) {
        let bcast_msgs: Vec<String> = {
            let mut st = self.state.lock().await;
            let mut msgs = Vec::new();
            if let Some(ref ch) = st.active {
                if ch.started.elapsed() > DURATION {
                    msgs.push(format!("-- The {} ends with no champion. Sharpen up for next time.", ch.name));
                    st.active = None;
                    st.next_start = Instant::now() + Duration::from_secs(st.cfg.interval_minutes * 60);
                }
            }
            if st.active.is_none() && st.cfg.enabled && Instant::now() >= st.next_start {
                let ch = new_challenge();
                msgs.push(format!(
                    ">> EVENT: The {} begins! FIRST to {} kills wins {} coins. Hunt now!",
                    ch.name, ch.target, ch.reward
                ));
                st.active = Some(ch);
            }
            msgs
        };
        for msg in &bcast_msgs { bcast(msg).await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let ks = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let kn = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if ks.is_empty() { return Outcome::Ignored; }

                // Enum: Won(steam, killer_name, ch_name, count, reward) | NearWin(killer_name, ch_name) | None
                enum KillAction {
                    Won(String, String, String, u32, i64),
                    NearWin(String, String),
                    Nothing,
                }

                // @inv: all Challenge mutation + decision under lock; no awaits inside
                let action: KillAction = {
                    let mut st = self.state.lock().await;
                    if let Some(ref mut ch) = st.active {
                        let c = ch.progress.entry(ks.clone()).or_insert(0);
                        *c += 1;
                        ch.leader_name.insert(ks.clone(), kn.clone());
                        let count = *c;
                        if count >= ch.target {
                            let ch_name = ch.name.clone();
                            let reward = ch.reward;
                            st.active = None;
                            st.next_start = Instant::now() + Duration::from_secs(st.cfg.interval_minutes * 60);
                            KillAction::Won(ks, kn, ch_name, count, reward)
                        } else if count + 1 == ch.target {
                            KillAction::NearWin(kn, ch.name.clone())
                        } else {
                            KillAction::Nothing
                        }
                    } else {
                        KillAction::Nothing
                    }
                };

                match action {
                    KillAction::Won(ks2, kn2, ch_name, count, reward) => {
                        credit(&ks2, reward);
                        bcast(&format!("*** {} WINS the {} with {} kills! +{} coins! ***", kn2, ch_name, count, reward)).await;
                        Outcome::Handled
                    }
                    KillAction::NearWin(kn2, ch_name) => {
                        bcast(&format!(">> {} is ONE kill from taking the {}!", kn2, ch_name)).await;
                        Outcome::Handled
                    }
                    KillAction::Nothing => Outcome::Ignored,
                }
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

                {
                    let mut st = self.state.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = st.rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    st.rate.insert(rate_key.clone(), now);
                }

                let parts: Vec<&str> = text.split_whitespace().collect();
                match parts[0].to_lowercase().as_str() {
                    "!obj" | "!objective" => {
                        // Handle admin on/off toggle first (inline, no await under lock needed)
                        if parts.len() >= 2 && is_owner(&steam, &player) {
                            match parts[1].to_lowercase().as_str() {
                                "on" => {
                                    { let mut st = self.state.lock().await; st.cfg.enabled = true; save_cfg(&st.cfg); }
                                    reply("[Objective] Auto-events ON.", &player).await;
                                    return Outcome::Handled;
                                }
                                "off" => {
                                    { let mut st = self.state.lock().await; st.cfg.enabled = false; save_cfg(&st.cfg); }
                                    reply("[Objective] Auto-events OFF.", &player).await;
                                    return Outcome::Handled;
                                }
                                _ => {}
                            }
                        }
                        // Status query - collect message under lock, send after
                        let reply_msg: String = {
                            let st = self.state.lock().await;
                            if let Some(ref ch) = st.active {
                                let remaining = DURATION.checked_sub(ch.started.elapsed())
                                    .map(|d| d.as_secs() / 60).unwrap_or(0);
                                let leader = ch.progress.iter().max_by_key(|(_, v)| **v);
                                let lead_txt = match leader {
                                    Some((s, v)) => format!("{} ({})", ch.leader_name.get(s).cloned().unwrap_or_else(|| "?".into()), v),
                                    None => "nobody yet".into(),
                                };
                                format!("[Objective] {} - first to {} kills = {} coins. {}min left. Leader: {}",
                                    ch.name, ch.target, ch.reward, remaining, lead_txt)
                            } else {
                                "[Objective] No active event. The next hunt is coming - stay ready.".to_string()
                            }
                        };
                        reply(&reply_msg, &player).await;
                        Outcome::Handled
                    }

                    "!startobj" => {
                        if !is_owner(&steam, &player) {
                            reply("[Objective] Admin only.", &player).await;
                            return Outcome::Handled;
                        }
                        // @inv: lock, mutate, capture info, drop lock, then broadcast
                        let start_info: Option<(String, u32, i64)> = {
                            let mut st = self.state.lock().await;
                            if st.active.is_some() { None } else {
                                let ch = new_challenge();
                                let info = (ch.name.clone(), ch.target, ch.reward);
                                st.active = Some(ch);
                                Some(info)
                            }
                        };
                        if let Some((name, target, reward)) = start_info {
                            bcast(&format!(
                                ">> EVENT: The {} begins! FIRST to {} kills wins {} coins. Hunt now!",
                                name, target, reward
                            )).await;
                        } else {
                            reply("[Objective] One already active.", &player).await;
                        }
                        Outcome::Handled
                    }

                    "!endobj" => {
                        if !is_owner(&steam, &player) {
                            reply("[Objective] Admin only.", &player).await;
                            return Outcome::Handled;
                        }
                        let had_active: bool = {
                            let mut st = self.state.lock().await;
                            let had = st.active.take().is_some();
                            if had { st.next_start = Instant::now() + Duration::from_secs(st.cfg.interval_minutes * 60); }
                            had
                        };
                        if had_active {
                            bcast("-- The event was called off by the admin.").await;
                        } else {
                            reply("[Objective] None active.", &player).await;
                        }
                        Outcome::Handled
                    }

                    _ => Outcome::Ignored,
                }
            }

            _ => Outcome::Ignored,
        }
    }
}
