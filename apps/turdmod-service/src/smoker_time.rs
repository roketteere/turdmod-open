// SmokerTime - persistent smoking cook-time override.
// Sweeps every loaded CookingRecipe whose name marks it a smoking recipe (Cook_Dish_Smoked_*)
// and sets CookingTime to the configured seconds. Re-applied on a timer so it survives SCUM
// restarts (recipe objects reset to their packaged default on restart) and catches recipes that
// load on demand. The value persists to disk and is settable in-game.
//
//   !smoketime            - show the current cook time
//   !smoketime <seconds>  - owner: set + persist + apply now (e.g. 36000 = 10h)
//
// @dep findInstancesByClass + writeActorProperty (bridge RPCs) — no bridge rebuild needed.
// @inv CookingRecipe.CookingTime is a FloatProperty @180; unit assumed seconds (calibration
//      note in IDEAS — confirm real-vs-game-time against a live smoke before trusting "10h").

use std::time::Duration;

use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1"; // YOUR_OWNER_NAME (Joel)
const ZILLA_STEAM_ID: &str = "YOUR_STEAM_ID_2"; // co-owner
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\smoker_time.json";
const DEFAULT_SECS: f64 = 36000.0; // 10 hours
// The router drops the first immediate tick, so we wake on a SHORT cadence (apply soon after a
// restart — small window where a fresh smoke would use the default) but internally gate the actual
// sweep to first-run + every SWEEP_EVERY. @inv the sweep does a per-object FName scan
// (findInstancesByClass) which leaks on OVH if polled tightly — keep SWEEP_EVERY slow; the 60s
// wake is a cheap time-check no-op in between.
const TICK_INTERVAL: Duration = Duration::from_secs(60);
const SWEEP_EVERY: Duration = Duration::from_secs(1800); // re-pin every 30 min
const MAX_SECS: f64 = 864_000.0; // 10 days, a sane ceiling

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == ZILLA_STEAM_ID || player == OWNER_NAME
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SmokerCfg {
    seconds: f64,
}
impl Default for SmokerCfg {
    fn default() -> Self {
        Self { seconds: DEFAULT_SECS }
    }
}

fn load() -> SmokerCfg {
    std::fs::read_to_string(STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(cfg: &SmokerCfg) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

// Set CookingTime on every loaded smoking recipe. Returns (matched, applied).
async fn apply_secs(secs: f64) -> (usize, usize) {
    let res = pipe_rpc::call(
        "findInstancesByClass",
        Some(serde_json::json!({ "class": "CookingRecipe", "exact": false, "limit": "300" })),
    )
    .await;
    let Ok(v) = res else { return (0, 0) };
    let Some(insts) = v.get("instances").and_then(|i| i.as_array()) else { return (0, 0) };
    let secs_str = format!("{}", secs);
    let mut matched = 0usize;
    let mut applied = 0usize;
    for inst in insts {
        let name = inst.get("name").and_then(|n| n.as_str()).unwrap_or("");
        // Smoking recipes are Cook_Dish_Smoked_* ; skip the class-default object.
        if !name.contains("Smoked") || name.starts_with("Default__") {
            continue;
        }
        let Some(ptr) = inst.get("ptr").and_then(|p| p.as_str()) else { continue };
        matched += 1;
        let w = pipe_rpc::call(
            "writeActorProperty",
            Some(serde_json::json!({
                "ptr": ptr,
                "propertyName": "CookingTime",
                "value": secs_str,
                "valueKind": "float",
            })),
        )
        .await;
        let ok = w
            .map(|r| r.get("ok").and_then(|o| o.as_bool()).unwrap_or(false))
            .unwrap_or(false);
        if ok {
            applied += 1;
        }
    }
    (matched, applied)
}

pub struct SmokerTime {
    secs: Mutex<f64>,
    last_sweep: Mutex<Option<std::time::Instant>>,
}
impl SmokerTime {
    pub fn new() -> Self {
        Self {
            secs: Mutex::new(load().seconds),
            last_sweep: Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl Mod for SmokerTime {
    fn name(&self) -> &'static str {
        "smoker_time"
    }
    fn commands(&self) -> &'static [&'static str] {
        &["!smoketime"]
    }
    // Scheduled: re-apply the override so it survives restarts + catches on-demand recipes.
    fn interval(&self) -> Option<Duration> {
        Some(TICK_INTERVAL)
    }

    async fn tick(&self, _ctx: &ModCtx) {
        // Gate the (leaky) sweep to first-run + every SWEEP_EVERY; cheap no-op otherwise.
        {
            let last = self.last_sweep.lock().await;
            if let Some(t) = *last {
                if t.elapsed() < SWEEP_EVERY {
                    return;
                }
            }
        }
        let secs = { *self.secs.lock().await };
        let (matched, _applied) = apply_secs(secs).await;
        // Only back off to the slow cadence once we actually found recipes. If the sweep found
        // nothing (SCUM still booting after a restart, recipes not loaded yet), keep retrying on
        // the 60s tick so the override pins within ~1 min of boot instead of waiting 30 min.
        if matched > 0 {
            *self.last_sweep.lock().await = Some(std::time::Instant::now());
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" {
            return Outcome::Ignored;
        }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let mut parts = text.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        if !cmd.eq_ignore_ascii_case("!smoketime") {
            return Outcome::Ignored;
        }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

        match parts.next() {
            None => {
                let secs = { *self.secs.lock().await };
                reply(
                    &format!(
                        "[Smoke] Cook time = {:.0}s ({:.1}h). Owner: !smoketime <seconds> to change.",
                        secs,
                        secs / 3600.0
                    ),
                    &player,
                )
                .await;
            }
            Some(arg) => {
                if !is_owner(&steam, &player) {
                    reply("[Smoke] Owner only.", &player).await;
                    return Outcome::Handled;
                }
                let Ok(secs) = arg.parse::<f64>() else {
                    reply("[Smoke] Usage: !smoketime <seconds>", &player).await;
                    return Outcome::Handled;
                };
                if !(1.0..=MAX_SECS).contains(&secs) {
                    reply(&format!("[Smoke] Range 1..{:.0} seconds.", MAX_SECS), &player).await;
                    return Outcome::Handled;
                }
                {
                    *self.secs.lock().await = secs;
                }
                save(&SmokerCfg { seconds: secs });
                let (m, a) = apply_secs(secs).await;
                reply(
                    &format!(
                        "[Smoke] Cook time set to {:.0}s ({:.1}h) — applied to {}/{} loaded smoke recipes.",
                        secs,
                        secs / 3600.0,
                        a,
                        m
                    ),
                    &player,
                )
                .await;
            }
        }
        Outcome::Handled
    }
}
