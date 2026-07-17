// Weather alerts - monitors weather severity and warns players of incoming storms.
// Event-driven (weatherChanged events); pairs with the weather cycle scheduler.

use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

struct WState { last_severity: f64, storm_active: bool }

pub struct WeatherAlerts { state: Mutex<WState> }
impl WeatherAlerts {
    pub fn new() -> Self { Self { state: Mutex::new(WState { last_severity: 0.0, storm_active: false }) } }
}

#[async_trait::async_trait]
impl Mod for WeatherAlerts {
    fn name(&self) -> &'static str { "weather_alerts" }
    // event-driven (no commands()): reacts to `weatherChanged`.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "weatherChanged" { return Outcome::Ignored; }
        let severity = ev.data.get("severity").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let mut msgs: Vec<&'static str> = Vec::new();
        {
            let mut st = self.state.lock().await;
            if severity >= 0.6 && st.last_severity < 0.6 && !st.storm_active {
                msgs.push("[Weather] Storm approaching! Seek shelter!");
                st.storm_active = true;
            }
            if severity >= 0.9 && st.last_severity < 0.9 {
                msgs.push("[Weather] SEVERE STORM! Visibility near zero!");
            }
            if severity < 0.3 && st.storm_active {
                msgs.push("[Weather] Storm passing. Skies clearing.");
                st.storm_active = false;
            }
            if severity <= 0.05 && st.last_severity > 0.1 {
                msgs.push("[Weather] Clear skies ahead.");
            }
            st.last_severity = severity;
        }
        if msgs.is_empty() { return Outcome::Ignored; }
        for m in &msgs { broadcast(m).await; }
        Outcome::Handled
    }
}
