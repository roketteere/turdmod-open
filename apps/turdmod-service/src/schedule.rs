//! Service-owned recurring restart scheduler. Interval-based (epoch math, no
//! timezone) — the manager renders local clock times from the epochs it returns.
//! Fires by self-calling POST /server/restart?countdown=N so it reuses the full
//! warn-banner + stop/start path (zero duplication). Persists to
//! C:\TurdMOD\restart-schedule[-<instance>].json so last_restart_at survives a
//! service restart. @dep: server.rs handle_restart, config.rs.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One pre-restart warning banner: shown when `at_secs` remain before the restart.
/// `color` is "R-G-B" (Notifications.json center-banner format). Admin-editable from the
/// manager's Restart Schedule panel. @dep server.rs restart_countdown (renders these).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarnBanner {
    pub at_secs: u64,
    pub color: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartSchedule {
    #[serde(default)]
    pub enabled: bool,
    /// Hours between restarts. f64 so the UI can offer 0.5h etc.
    #[serde(default = "default_interval")]
    pub interval_hours: f64,
    /// Lead time before a scheduled restart that the warning countdown begins (seconds).
    /// Passed as /server/restart?countdown=N; banners fire at their `at_secs` marks within it.
    #[serde(default = "default_warn")]
    pub warn_secs: u64,
    /// Epoch of the last restart this scheduler fired (or the anchor it set on
    /// first observing an enabled schedule). Drives next_restart_at.
    #[serde(default)]
    pub last_restart_at: Option<u64>,
    /// Editable pre-restart warning banners. Empty = no banners (just the chat countdown).
    #[serde(default = "default_messages")]
    pub messages: Vec<WarnBanner>,
}

// 3h = the OOM-safe cadence for SCUM's ~4.7GB/h native leak on the 32GB box. @ctx: this
// in-service scheduler is now the SOLE restart driver — replaces the old external
// TurdMODRestart Windows task (retired 2026-06-18 so the manager can fully control it).
fn default_interval() -> f64 {
    3.0
}
fn default_warn() -> u64 {
    600
}
fn default_messages() -> Vec<WarnBanner> {
    vec![
        WarnBanner { at_secs: 300, color: "255-220-0".into(), text: "~5 Minutes until server restart \u{2014} wrap up".into() },
        WarnBanner { at_secs: 240, color: "255-180-0".into(), text: "~4 Minutes until server restart".into() },
        WarnBanner { at_secs: 180, color: "255-140-0".into(), text: "~3 Minutes until server restart".into() },
        WarnBanner { at_secs: 120, color: "255-70-40".into(), text: "~2 Minutes until server restart \u{2014} get to safety".into() },
        WarnBanner { at_secs: 60,  color: "255-0-0".into(),   text: "~60 Seconds until server restart!".into() },
    ]
}

impl Default for RestartSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: default_interval(),
            warn_secs: default_warn(),
            last_restart_at: None,
            messages: default_messages(),
        }
    }
}

impl RestartSchedule {
    /// Next scheduled restart epoch, or None if disabled / not yet anchored.
    pub fn next_restart_at(&self) -> Option<u64> {
        if !self.enabled || self.interval_hours <= 0.0 {
            return None;
        }
        self.last_restart_at.map(|base| base + (self.interval_hours * 3600.0) as u64)
    }
}

fn path(instance: &str) -> PathBuf {
    if instance == "default" {
        PathBuf::from(r"C:\TurdMOD\restart-schedule.json")
    } else {
        PathBuf::from(format!(r"C:\TurdMOD\restart-schedule-{}.json", instance))
    }
}

pub fn load(instance: &str) -> RestartSchedule {
    std::fs::read_to_string(path(instance))
        .ok()
        // @brk: strip a leading UTF-8 BOM before parsing. PowerShell 5.1 `Set-Content -Encoding
        //   utf8` writes a BOM, and serde_json::from_str rejects it -> silent fall-through to
        //   default (enabled=false) -> scheduler never fires (OVH ran 6+ days no-restart, Jul-14).
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

pub fn save(instance: &str, s: &RestartSchedule) -> std::io::Result<()> {
    std::fs::write(path(instance), serde_json::to_string_pretty(s).unwrap_or_default())
}

/// Background loop: every 30s, fire a restart if the schedule is due. Anchors
/// last_restart_at to "now" the first time it sees an enabled schedule, so
/// enabling never triggers an immediate restart.
pub async fn run_scheduler(cfg: Config) {
    let instance = cfg.instance_id.clone();
    tracing::info!("restart scheduler started (instance={})", instance);
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut sched = load(&instance);
        // The post-wipe restore campaign overrides the cadence with its taper (1h→3h→6h) while
        // active, and forces restarts even if the normal schedule is disabled. We compute EFFECTIVE
        // values locally and never write them back to the schedule file, so when the campaign ends
        // the normal schedule (its own enabled flag + interval) resumes unchanged.
        let camp_override = crate::restore_campaign::interval_override(&instance);
        let effective_enabled = sched.enabled || camp_override.is_some();
        let effective_interval = camp_override.unwrap_or(sched.interval_hours);
        if !effective_enabled || effective_interval <= 0.0 {
            continue;
        }
        let now = now_epoch();
        // Skip-if-recently-restarted: if SCUM was (re)started more recently than our last recorded
        // fire (a manual restart or a deploy), re-anchor to its real start so the schedule measures
        // from the live process. Prevents the double-bounce the old Windows task guarded against.
        // Best-effort — on a /status read failure we fall through to the normal schedule.
        if let Some(up) = scum_uptime_secs(&cfg).await {
            let scum_start = now.saturating_sub(up);
            if sched.last_restart_at.map_or(true, |b| scum_start > b + 60) {
                sched.last_restart_at = Some(scum_start);
                let _ = save(&instance, &sched);
            }
        }
        let base = match sched.last_restart_at {
            Some(b) => b,
            None => {
                // First observation — anchor to now so the first restart is one
                // full interval away (no surprise restart on enable).
                sched.last_restart_at = Some(now);
                let _ = save(&instance, &sched);
                continue;
            }
        };
        let next = base + (effective_interval * 3600.0) as u64;
        if now >= next {
            tracing::info!("scheduled restart firing (every {}h, warn {}s){}", effective_interval, sched.warn_secs,
                if camp_override.is_some() { " [restore-campaign]" } else { "" });
            let url = format!("http://127.0.0.1:{}/server/restart?countdown={}", cfg.port, sched.warn_secs);
            if let Err(e) = reqwest::Client::new()
                .post(&url)
                .bearer_auth(&cfg.token)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                tracing::warn!("scheduled restart self-call failed: {}", e);
            }
            sched.last_restart_at = Some(now_epoch());
            let _ = save(&instance, &sched);
        }
    }
}

/// SCUM process uptime in seconds via the service's own /status (None if down/unreachable).
/// @dep server.rs handle_status (uptime_secs + pid).
async fn scum_uptime_secs(cfg: &Config) -> Option<u64> {
    let url = format!("http://127.0.0.1:{}/status", cfg.port);
    let v: serde_json::Value = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&cfg.token)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    if v.get("pid").map_or(true, |p| p.is_null()) {
        return None; // SCUM not running — don't anchor to a dead process
    }
    v.get("uptime_secs").and_then(|u| u.as_u64())
}
