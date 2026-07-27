// VAC ban screening — checks Steam API on player login, alerts owner, auto-kicks recent bans

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const API_KEY_PATH: &str = r"C:\TurdMOD\data\steam-api-key.txt";
const BANS_URL: &str = "https://api.steampowered.com/ISteamUser/GetPlayerBans/v1/";
const RECENT_BAN_DAYS: i64 = 365;

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn alert_owner(msg: &str) {
    let owner_name = match pipe_rpc::call("getOnlinePlayers", None).await {
        Ok(resp) => {
            resp.get("players").and_then(|v| v.as_array()).and_then(|arr| {
                arr.iter().find_map(|p| {
                    if p.get("steam").and_then(|v| v.as_str()) == Some(crate::owner::primary_id()) {
                        p.get("name").and_then(|v| v.as_str()).map(String::from)
                    } else { None }
                })
            }).unwrap_or_else(|| crate::owner::name().to_string())
        }
        Err(_) => crate::owner::name().to_string(),
    };
    reply(msg, &owner_name).await;
}

pub struct VacScreening {
    api_key: String,
    checked: Mutex<HashMap<String, Instant>>,
    client: reqwest::Client,
}

impl VacScreening {
    pub fn new() -> Self {
        let api_key = match std::fs::read_to_string(API_KEY_PATH) {
            Ok(s) if !s.trim().is_empty() => {
                tracing::info!("vac: enabled (auto-kick < {} days)", RECENT_BAN_DAYS);
                s.trim().to_string()
            }
            _ => {
                tracing::warn!("vac: no API key at {} - disabled", API_KEY_PATH);
                String::new()
            }
        };
        Self {
            api_key,
            checked: Mutex::new(HashMap::new()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl Mod for VacScreening {
    fn name(&self) -> &'static str { "vac_screening" }
    // event-driven: login + owner chat for !vacstatus

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                if self.api_key.is_empty() { return Outcome::Ignored; }

                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if steam.is_empty() || crate::owner::is_owner_steam(&steam) {
                    return Outcome::Ignored;
                }

                {
                    let mut checked = self.checked.lock().await;
                    if let Some(t) = checked.get(&steam) {
                        if t.elapsed() < Duration::from_secs(3600) { return Outcome::Ignored; }
                    }
                    checked.insert(steam.clone(), Instant::now());
                }

                let url = format!("{}?key={}&steamids={}", BANS_URL, self.api_key, steam);
                let client2 = self.client.clone();
                tokio::spawn(async move {
                    let Ok(resp) = client2.get(&url).timeout(Duration::from_secs(10)).send().await else { return };
                    let Ok(data) = resp.json::<serde_json::Value>().await else { return };
                    let Some(p) = data.get("players").and_then(|v| v.as_array()).and_then(|a| a.first()) else { return };

                    let vac = p.get("VACBanned").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !vac { return; }

                    let count = p.get("NumberOfVACBans").and_then(|v| v.as_u64()).unwrap_or(0);
                    let days = p.get("DaysSinceLastBan").and_then(|v| v.as_i64()).unwrap_or(i64::MAX);

                    alert_owner(&format!("[VAC] {} has {} VAC ban(s), last {} days ago", player, count, days)).await;

                    if days < RECENT_BAN_DAYS {
                        tracing::info!("vac: auto-kicking {} (ban {} days ago)", player, days);
                        let params = serde_json::json!({ "playerName": player, "reason": "VAC banned account" });
                        pipe_rpc::call("kickPlayer", Some(params)).await.ok();
                    }
                });
                Outcome::Handled
            }

            "chat" => {
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("");
                if !crate::owner::is_owner_steam(steam) { return Outcome::Ignored; }
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();

                if text.eq_ignore_ascii_case("!vacstatus") {
                    reply(&format!("[VAC] Screening enabled. Auto-kick: < {} days", RECENT_BAN_DAYS), &player).await;
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }

            _ => Outcome::Ignored,
        }
    }
}
