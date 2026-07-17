//! Detector framework.
//!
//! A detector is a stateful evaluator that subscribes to one or more
//! event channels (the same channels the companion fans out:
//! `system.login`, `system.logout`, `system.kill`, `vehicle.any`,
//! `system.admin`, `system.chat`). For each event it returns a `Verdict`.
//!
//! The harness (`main.rs`) holds a `Vec<Box<dyn Detector>>`, dispatches
//! every incoming event to every interested detector, and forwards their
//! flags to the reporter.
//!
//! A detector that needs to remember state across events (most of them
//! do) holds an internal `&mut self`. The framework is single-threaded
//! per detector — order matters. Cross-detector concurrency is at the
//! harness level.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod kill_distance;
pub mod login_burst;
pub mod speed;
pub mod teleport;

/// What a detector concluded about a single event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Verdict {
    /// Nothing to see; pass.
    Ok,
    /// Suspicious but not actionable. Logged for trend analysis.
    Warn { reason: String },
    /// Strongly suggestive of cheating. Forwarded to the backend; admin
    /// is paged in real-time.
    Flag {
        reason: String,
        severity: Severity,
        /// Optional supporting data for the admin to inspect (positions,
        /// distances, weapon, prior events). Stored as JSON in the
        /// backend's flag history.
        evidence: serde_json::Value,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    /// Borderline-certain. Admin should ban with prejudice.
    Critical,
}

/// One log-derived event the companion publishes. Mirrors the shape of
/// `ServerEvent` in `apps/turdmod-companion/src/parsers.ts`. Anything we
/// add to the companion's parser surfaces here too.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Event {
    Login {
        ts: String,
        ip: String,
        steam: String,
        player: String,
        pos: Vec3,
    },
    Logout {
        ts: String,
        steam: String,
        player: String,
    },
    Kill {
        ts: String,
        victim: String,
        #[serde(default)] victim_steam: Option<String>,
        #[serde(default)] killer: Option<String>,
        #[serde(default)] killer_steam: Option<String>,
        #[serde(default)] weapon: Option<String>,
        #[serde(default)] distance_m: Option<f32>,
        #[serde(default)] headshot: Option<bool>,
        #[serde(default)] killer_pos: Option<Vec3>,
        #[serde(default)] victim_pos: Option<Vec3>,
    },
    Vehicle {
        ts: String,
        event: String,
        vclass: String,
        vid: String,
        #[serde(default)] owner: Option<String>,
        pos: Vec3,
    },
    Admin {
        ts: String,
        steam: String,
        player: String,
        cmd: String,
    },
    Chat {
        ts: String,
        channel: String,
        player: String,
        #[serde(default)] steam: Option<String>,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn distance_cm(&self, other: &Vec3) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
    pub fn distance_m(&self, other: &Vec3) -> f32 {
        self.distance_cm(other) / 100.0
    }
}

/// What a detector reports up to the harness.
#[derive(Debug, Clone, Serialize)]
pub struct DetectorReport {
    pub detector: String,
    pub ts: DateTime<Utc>,
    pub verdict: Verdict,
    pub steam: Option<String>,
    pub player: Option<String>,
}

/// The trait every detector implements.
pub trait Detector: Send {
    fn name(&self) -> &'static str;
    /// Called once per inbound event the harness has decoded. The
    /// detector decides whether the event is interesting and returns its
    /// verdict. Detectors keep their own state across calls.
    fn evaluate(&mut self, event: &Event) -> Verdict;
}

/// Build the default registry. The harness calls this at start-up; tests
/// build their own subsets.
pub fn default_registry() -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(kill_distance::KillDistanceDetector::new()),
        Box::new(speed::SpeedDetector::new()),
        Box::new(login_burst::LoginBurstDetector::new()),
        Box::new(teleport::TeleportDetector::new()),
    ]
}
