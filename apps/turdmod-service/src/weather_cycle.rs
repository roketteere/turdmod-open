// WeatherCycle - periodic weather phase rotation (extracted from scheduler.rs into a trait mod).
// Phases: clear -> building -> storm -> clearing (loop). scheduler.rs still owns restart_scheduler.

use std::time::Duration;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

// (name, severity, duration_secs)
const PHASES: &[(&str, f64, u64)] = &[
    ("clear",    0.0,  2400), // 40 min clear
    ("building", 0.3,  1200), // 20 min light clouds
    ("storm",    0.8,  1800), // 30 min heavy rain
    ("clearing", 0.2,  900),  // 15 min dying down
];

const CYCLE_SECS: u64 = 2400 + 1200 + 1800 + 900; // 6300s = 105 min

pub struct WeatherCycle;
impl WeatherCycle {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Mod for WeatherCycle {
    fn name(&self) -> &'static str { "weather_cycle" }
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(CYCLE_SECS)) }

    // One full weather cycle per tick (router fires tick every CYCLE_SECS; first fire is dropped
    // by the interval ticker, so the first real cycle starts after the server is well up).
    async fn tick(&self, _ctx: &ModCtx) {
        for (name, severity, duration_secs) in PHASES {
            let params = serde_json::json!({ "severity": severity });
            if pipe_rpc::call("setWeather", Some(params)).await.is_ok() {
                pipe_rpc::call("forceWeatherSnapshot", None).await.ok();
                tracing::info!("weather_cycle: phase={} severity={}", name, severity);
            }
            tokio::time::sleep(Duration::from_secs(*duration_secs)).await;
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}
