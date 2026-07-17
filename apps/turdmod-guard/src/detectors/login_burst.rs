//! Login-burst detector.
//!
//! Catches alt-account / re-creation patterns. If a single IP produces
//! many distinct steam ids in a short window, that's classic ban-evasion
//! / vote-stuffing / market-stuffing.
//!
//! The signal is per-IP, NOT per-steam, because a banned cheater making
//! a new account is the threat model.

use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

use super::{Detector, Event, Severity, Verdict};

pub struct LoginBurstDetector {
    /// Sliding window of recent logins per IP.
    by_ip: HashMap<String, VecDeque<LoginRow>>,
}

#[derive(Clone)]
struct LoginRow {
    steam: String,
    ts: DateTime<Utc>,
}

impl LoginBurstDetector {
    pub fn new() -> Self {
        Self { by_ip: HashMap::new() }
    }
}

const WINDOW_SECONDS: i64 = 600; // 10 min
const FLAG_DISTINCT_ACCOUNTS: usize = 4; // 4+ accounts from one IP in 10m

fn parse_scum_ts(s: &str) -> Option<DateTime<Utc>> {
    let nd = NaiveDateTime::parse_from_str(s, "%Y.%m.%d-%H.%M.%S").ok()?;
    Some(Utc.from_utc_datetime(&nd))
}

impl Detector for LoginBurstDetector {
    fn name(&self) -> &'static str {
        "login_burst"
    }

    fn evaluate(&mut self, event: &Event) -> Verdict {
        let (ip, steam, ts) = match event {
            Event::Login { ip, steam, ts, .. } => (ip.clone(), steam.clone(), ts.clone()),
            _ => return Verdict::Ok,
        };
        let ts_dt = match parse_scum_ts(&ts) {
            Some(t) => t,
            None => return Verdict::Ok,
        };

        // Skip the loopback/same-host login from the dedicated server
        // itself — the host's own steam id appears as 127.0.0.1.
        if ip == "127.0.0.1" {
            return Verdict::Ok;
        }

        let q = self.by_ip.entry(ip.clone()).or_default();
        q.push_back(LoginRow { steam: steam.clone(), ts: ts_dt });
        let cutoff = ts_dt - chrono::Duration::seconds(WINDOW_SECONDS);
        while let Some(front) = q.front() {
            if front.ts < cutoff {
                q.pop_front();
            } else {
                break;
            }
        }

        let distinct: std::collections::HashSet<&String> = q.iter().map(|r| &r.steam).collect();
        if distinct.len() < FLAG_DISTINCT_ACCOUNTS {
            return Verdict::Ok;
        }

        Verdict::Flag {
            reason: format!(
                "{} distinct steam ids logged in from {ip} in last {WINDOW_SECONDS}s",
                distinct.len()
            ),
            severity: if distinct.len() >= 8 { Severity::High } else { Severity::Medium },
            evidence: serde_json::json!({
                "ip": ip,
                "window_seconds": WINDOW_SECONDS,
                "distinct_accounts": distinct.iter().collect::<Vec<_>>(),
                "rows": q.iter().map(|r| serde_json::json!({"steam": r.steam, "ts": r.ts.to_rfc3339()})).collect::<Vec<_>>(),
            }),
        }
    }
}
