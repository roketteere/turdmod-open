// Radio - server-wide music/ambient announcements via chat.
// !radio on/off/skip/queue <song>/np - DJ system with queue. Songs are themed text broadcasts.
// Track rotation runs on a 10s interval tick; commands() = !radio.

use std::collections::VecDeque;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const TRACK_DURATION: Duration = Duration::from_secs(180); // 3 min per "song"
const TICK: Duration = Duration::from_secs(10);

struct RadioState {
    on: bool,
    queue: VecDeque<String>,
    current: Option<String>,
    started: Option<Instant>,
    dj: Option<String>,
}

const AMBIENT_TRACKS: &[&str] = &[
    "Wasteland Blues - The Survivors",
    "Dead City Waltz - Zombie Orchestra",
    "Running From Nothing - The Escapees",
    "Campfire Stories - Lone Wolf",
    "Last Broadcast - Radio Free SCUM",
    "Bullet Rain - The Marksmen",
    "Dawn Patrol - Coastal Drifters",
    "Bunker Lullaby - Underground Sound",
    "Chopper Inbound - Air Support",
    "The Long Road Home - Midnight Runners",
];

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn pick_random_track() -> String {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as usize;
    AMBIENT_TRACKS[seed % AMBIENT_TRACKS.len()].to_string()
}

pub struct Radio { state: Mutex<RadioState>, rate: Mutex<HashMap<String, Instant>> }
impl Radio {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RadioState { on: false, queue: VecDeque::new(), current: None, started: None, dj: None }),
            rate: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Radio {
    fn name(&self) -> &'static str { "radio" }
    fn commands(&self) -> &'static [&'static str] { &["!radio"] }
    fn interval(&self) -> Option<Duration> { Some(TICK) }

    // Advance the track when the current one runs out (the old top-of-loop rotation).
    async fn tick(&self, _ctx: &ModCtx) {
        let announce = {
            let mut st = self.state.lock().await;
            if !st.on { None }
            else {
                let should_advance = match st.started { Some(s) => s.elapsed() > TRACK_DURATION, None => true };
                if should_advance {
                    let next = st.queue.pop_front().unwrap_or_else(pick_random_track);
                    st.current = Some(next.clone());
                    st.started = Some(Instant::now());
                    Some(next)
                } else { None }
            }
        };
        if let Some(next) = announce { broadcast(&format!("[Radio] Now playing: {}", next)).await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!radio") { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

        let mut msgs: Vec<(bool, String)> = Vec::new(); // (is_broadcast, msg); replies go to `player`
        {
            let mut st = self.state.lock().await;
            match sub.as_str() {
                "on" => {
                    if st.on { msgs.push((false, "[Radio] Already playing!".to_string())); }
                    else {
                        st.on = true;
                        st.dj = Some(player.clone());
                        let t = pick_random_track();
                        st.current = Some(t.clone());
                        st.started = Some(Instant::now());
                        msgs.push((true, format!("[Radio] ON! DJ: {} | Now playing: {}", player, t)));
                    }
                }
                "off" => {
                    st.on = false;
                    st.current = None;
                    st.queue.clear();
                    msgs.push((true, "[Radio] OFF. Silence returns to the wasteland.".to_string()));
                }
                "skip" => {
                    if !st.on { msgs.push((false, "[Radio] Radio is off.".to_string())); }
                    else {
                        let next = st.queue.pop_front().unwrap_or_else(pick_random_track);
                        st.current = Some(next.clone());
                        st.started = Some(Instant::now());
                        msgs.push((true, format!("[Radio] Skipped! Now playing: {}", next)));
                    }
                }
                "queue" => {
                    let song = parts[2..].join(" ");
                    if song.is_empty() {
                        if st.queue.is_empty() {
                            msgs.push((false, "[Radio] Queue empty - auto-DJ mode.".to_string()));
                        } else {
                            let songs: Vec<String> = st.queue.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, s)).collect();
                            msgs.push((false, format!("[Radio] Queue: {}", songs.join(" | "))));
                        }
                    } else {
                        st.queue.push_back(song.clone());
                        msgs.push((false, format!("[Radio] Queued: {} (position {})", song, st.queue.len())));
                    }
                }
                "np" | "nowplaying" | "" => {
                    if let Some(ref track) = st.current {
                        let elapsed = st.started.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                        msgs.push((false, format!("[Radio] Now playing: {} ({}:{:02})", track, elapsed / 60, elapsed % 60)));
                    } else {
                        msgs.push((false, "[Radio] Off. !radio on to start.".to_string()));
                    }
                }
                _ => msgs.push((false, "[Radio] Commands: on/off/skip/queue/np".to_string())),
            }
        }
        for (is_bc, m) in &msgs {
            if *is_bc { broadcast(m).await } else { reply(m, &player).await }
        }
        Outcome::Handled
    }
}
