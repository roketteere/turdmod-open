//! Teleport detector.
//!
//! Specifically catches *unauthorized* position jumps — i.e. the player
//! moved a big distance with no admin teleport command preceding the
//! jump. The speed detector will flag the same motion as a velocity
//! anomaly, but teleport is the more specific pattern: jump implausibly
//! far in one tick instead of sustained high velocity.
//!
//! Heuristic:
//!   * Track last position per steam id (similar to speed).
//!   * If the next observation is >`TELEPORT_THRESHOLD_M` away from the
//!     last AND the gap is < `TELEPORT_MIN_DT_SECONDS`, flag.
//!   * Suppress for ~10s after an admin Teleport command for that
//!     player.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

use super::{Detector, Event, Severity, Vec3, Verdict};

pub struct TeleportDetector {
    last: HashMap<String, Observation>,
    recent_admin_teleports: HashMap<String, DateTime<Utc>>,
}

struct Observation {
    pos: Vec3,
    ts: DateTime<Utc>,
}

impl TeleportDetector {
    pub fn new() -> Self {
        Self {
            last: HashMap::new(),
            recent_admin_teleports: HashMap::new(),
        }
    }
}

/// Anything above this distance jumped within a short window is teleport.
const TELEPORT_THRESHOLD_M: f32 = 500.0;
/// Window we consider too-fast.
const TELEPORT_MIN_DT_SECONDS: i64 = 30;
const TELEPORT_GRACE_SECONDS: i64 = 10;

fn parse_scum_ts(s: &str) -> Option<DateTime<Utc>> {
    let nd = NaiveDateTime::parse_from_str(s, "%Y.%m.%d-%H.%M.%S").ok()?;
    Some(Utc.from_utc_datetime(&nd))
}

impl Detector for TeleportDetector {
    fn name(&self) -> &'static str {
        "teleport"
    }

    fn evaluate(&mut self, event: &Event) -> Verdict {
        if let Event::Admin { ts, steam, cmd, .. } = event {
            if cmd.to_lowercase().contains("teleport") {
                if let Some(t) = parse_scum_ts(ts) {
                    self.recent_admin_teleports.insert(steam.clone(), t);
                }
            }
            return Verdict::Ok;
        }

        let (steam, pos, ts) = match event {
            Event::Login { steam, pos, ts, .. } => (steam.clone(), *pos, ts.clone()),
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

        if let Some(t) = self.recent_admin_teleports.get(&steam) {
            if (ts_dt - *t).num_seconds().abs() <= TELEPORT_GRACE_SECONDS {
                self.last.insert(steam.clone(), Observation { pos, ts: ts_dt });
                return Verdict::Ok;
            }
        }

        let prev = self.last.insert(steam.clone(), Observation { pos, ts: ts_dt });
        let prev = match prev {
            Some(p) => p,
            None => return Verdict::Ok,
        };

        let dt = (ts_dt - prev.ts).num_seconds();
        if dt < 0 || dt > TELEPORT_MIN_DT_SECONDS {
            return Verdict::Ok;
        }
        let dist_m = pos.distance_m(&prev.pos);
        if dist_m < TELEPORT_THRESHOLD_M {
            return Verdict::Ok;
        }

        Verdict::Flag {
            reason: format!(
                "jumped {dist_m:.0}m in {dt}s with no admin teleport — likely teleport hack"
            ),
            severity: if dist_m > 5000.0 { Severity::Critical } else { Severity::High },
            evidence: serde_json::json!({
                "steam": steam,
                "from": prev.pos,
                "to": pos,
                "from_ts": prev.ts.to_rfc3339(),
                "to_ts": ts_dt.to_rfc3339(),
                "dt_seconds": dt,
                "distance_m": dist_m,
            }),
        }
    }
}
