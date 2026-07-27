// Server performance + capacity monitor. Polls getServerStats on a 60s tick, logs JSONL,
// alerts the owner on high memory. @inv getServerStats: memory{privateUsageMB,...}, uobjects{...}.

use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const MEM_WARN_MB: f64 = 24_000.0;
const ANALYTICS_DIR: &str = r"C:\TurdMOD\data\analytics";

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn f(v: &serde_json::Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for k in path { cur = match cur.get(k) { Some(x) => x, None => return 0.0 }; }
    cur.as_f64().unwrap_or(0.0)
}

pub struct PerfMonitor { warned_mem: Mutex<bool> }
impl PerfMonitor { pub fn new() -> Self { Self { warned_mem: Mutex::new(false) } } }

#[async_trait::async_trait]
impl Mod for PerfMonitor {
    fn name(&self) -> &'static str { "perf_monitor" }
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    async fn tick(&self, _ctx: &ModCtx) {
        let Ok(s) = pipe_rpc::call("getServerStats", None).await else { return };
        let priv_mb = f(&s, &["memory", "privateUsageMB"]);
        let ws_mb = f(&s, &["memory", "workingSetMB"]);
        let peak_ws_mb = f(&s, &["memory", "peakWorkingSetMB"]);
        let pagefile_mb = f(&s, &["memory", "pageFileMB"]);
        let uobjects = f(&s, &["uobjects", "total"]);
        let classes = f(&s, &["uobjects", "classes"]);
        let players = f(&s, &["uobjects", "playerControllers"]);
        let prisoners = f(&s, &["uobjects", "prisoners"]);
        let cpu_total_ms = f(&s, &["cpu", "totalMs"]);
        let cpu_user_ms = f(&s, &["cpu", "userMs"]);
        let io_read_mb = f(&s, &["io", "readBytes"]) / 1_048_576.0;
        let io_write_mb = f(&s, &["io", "writeBytes"]) / 1_048_576.0;
        let uptime_s = f(&s, &["uptimeMs"]) / 1000.0;

        let alert_msg: Option<String> = {
            let mut warned = self.warned_mem.lock().await;
            if priv_mb >= MEM_WARN_MB {
                if !*warned { *warned = true; Some(format!("[MEM WARNING] Private memory {:.0} MB (>{:.0}) | uobjects {:.0} | players {:.0} | uptime {:.1}h - approaching OOM", priv_mb, MEM_WARN_MB, uobjects, players, uptime_s / 3600.0)) } else { None }
            } else if *warned && priv_mb < MEM_WARN_MB * 0.9 { *warned = false; Some(format!("[MEM] recovered to {:.0} MB", priv_mb)) } else { None }
        };
        if priv_mb >= MEM_WARN_MB { tracing::warn!("perf: HIGH MEM priv={:.0}MB uobjects={:.0} uptime={:.1}h", priv_mb, uobjects, uptime_s / 3600.0); }
        if let Some(msg) = alert_msg { reply(&msg, crate::owner::name()).await; }

        let line = serde_json::json!({
            "t": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            "type": "perf", "uptime_s": uptime_s as u64, "players": players as u64,
            "priv_mb": priv_mb, "ws_mb": ws_mb, "peak_ws_mb": peak_ws_mb, "pagefile_mb": pagefile_mb,
            "uobjects": uobjects as u64, "classes": classes as u64, "prisoners": prisoners as u64,
            "cpu_total_ms": cpu_total_ms as u64, "cpu_user_ms": cpu_user_ms as u64,
            "io_read_mb": io_read_mb, "io_write_mb": io_write_mb
        });
        let dir = std::path::Path::new(ANALYTICS_DIR);
        let _ = std::fs::create_dir_all(dir);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("perf.jsonl")) {
            use std::io::Write;
            if let Ok(s) = serde_json::to_string(&line) { writeln!(file, "{}", s).ok(); }
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}
