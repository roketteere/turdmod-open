//! Speedhack detector.
//!
//! Tracks the last known position of every player from any event that
//! carries a position (login, logout, kill killer/victim positions). If
//! the time-delta + distance-delta between two consecutive observations
//! implies a velocity higher than what's physically possible — even
//! generously assuming a vehicle and chained admin teleports — flag.
//!
//! This is intentionally conservative. Real false-positive sources:
//!   * Server restarts (player reappears far from where they were)
//!   * Admin teleports (handled by the teleport detector instead)
//!   * Death + respawn far from the corpse
//!   * Network reconnection landing on a fresh save state
//!
//! We bucket observations by steam id and require N>=2 events within a
//! reasonable window before flagging — and we *cross-reference* with the
//! teleport detector's admin-command log so an admin's `Teleport`
//! command immediately invalidates speed flags for that player for ~10s.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

use super::{Detector, Event, Severity, Vec3, Verdict};

pub struct SpeedDetector {
    /// Last observation per steam id.
    last: HashMap<String, Observation>,
    /// Recent admin teleport timestamps per steam id (prevents flagging
    /// admin movements as speedhacks).
    recent_admin_teleports: HashMap<String, DateTime<Utc>>,
}

struct Observation {
    pos: Vec3,
    ts: DateTime<Utc>,
}

impl SpeedDetector {
    pub fn new() -> Self {
        Self {
            last: HashMap::new(),
            recent_admin_teleports: HashMap::new(),
        }
    }
}

/// Conservative max sustained ground velocity (m/s). Tuned so a fast
/// vehicle on a downhill straightaway (~30 m/s ≈ 108 km/h) stays under,
/// while teleport-style jumps blow through.
const MAX_PLAUSIBLE_VELOCITY_MPS: f32 = 50.0;

/// We don't speed-check across gaps longer than this. Long gaps include
/// disconnect/reconnect; the speed signal is unreliable.
const MAX_GAP_SECONDS: i64 = 300;

/// After an admin teleport, suppress speed flags on that player for
/// this many seconds.
const TELEPORT_GRACE_SECONDS: i64 = 10;

fn parse_scum_ts(s: &str) -> Option<DateTime<Utc>> {
    // SCUM ts: "YYYY.MM.DD-HH.MM.SS"
    let nd = NaiveDateTime::parse_from_str(s, "%Y.%m.%d-%H.%M.%S").ok()?;
    Some(Utc.from_utc_datetime(&nd))
}

impl Detector for SpeedDetector {
    fn name(&self) -> &'static str {
        "speed"
    }

    fn evaluate(&mut self, event: &Event) -> Verdict {
        // Soak up admin teleport events first — they shape the
        // suppression window.
        if let Event::Admin { ts, steam, cmd, .. } = event {
            if cmd.to_lowercase().contains("teleport") {
                if let Some(t) = parse_scum_ts(ts) {
                    self.recent_admin_teleports.insert(steam.clone(), t);
                }
            }
            return Verdict::Ok;
        }

        // Pull (steam, pos, ts) out of any event that has them.
        let (steam, pos, ts) = match event {
            Event::Login { steam, pos, ts, .. } => (steam.clone(), *pos, ts.clone()),
            Event::Logout { steam, ts, .. } => {
                // Logouts don't carry position in our parser today;
                // skip them. (When SCUM logs include logout pos, wire it up.)
                let _ = (steam, ts);
                return Verdict::Ok;
            }
            Event::Kill {
                killer_steam: Some(steam),
                killer_pos: Some(pos),
                ts,
                ..
            } => (steam.clone(), *pos, ts.clone()),
            _ => return Verdict::Ok,
        };

        let ts_dt = match parse_scum_ts(&ts) {
            Some(t) => t,
            None => return Verdict::Ok,
        };

        // Was the player teleported recently? Reset their last known
        // position to the current one (no speed signal across a teleport)
        // and skip the velocity check.
        if let Some(t) = self.recent_admin_teleports.get(&steam) {
            if (ts_dt - *t).num_seconds().abs() <= TELEPORT_GRACE_SECONDS {
                self.last.insert(steam.clone(), Observation { pos, ts: ts_dt });
                return Verdict::Ok;
            }
        }

        let prev = self.last.insert(steam.clone(), Observation { pos, ts: ts_dt });
        let prev = match prev {
            Some(p) => p,
            None => return Verdict::Ok, // first sighting
        };

        let dt = (ts_dt - prev.ts).num_seconds();
        if dt <= 0 || dt > MAX_GAP_SECONDS {
            return Verdict::Ok;
        }
        let dist_m = pos.distance_m(&prev.pos);
        let velocity = dist_m / dt as f32;
        if velocity <= MAX_PLAUSIBLE_VELOCITY_MPS {
            return Verdict::Ok;
        }

        let severity = if velocity > MAX_PLAUSIBLE_VELOCITY_MPS * 5.0 {
            Severity::Critical
        } else if velocity > MAX_PLAUSIBLE_VELOCITY_MPS * 2.0 {
            Severity::High
        } else {
            Severity::Medium
        };

        Verdict::Flag {
            reason: format!(
                "implied velocity {velocity:.1} m/s ({dist_m:.0}m / {dt}s) exceeds plausible {MAX_PLAUSIBLE_VELOCITY_MPS:.0} m/s"
            ),
            severity,
            evidence: serde_json::json!({
                "steam": steam,
                "from": prev.pos,
                "to": pos,
                "from_ts": prev.ts.to_rfc3339(),
                "to_ts": ts_dt.to_rfc3339(),
                "dt_seconds": dt,
                "distance_m": dist_m,
                "velocity_mps": velocity,
            }),
        }
    }
}
