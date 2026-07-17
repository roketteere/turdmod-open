//! Kill-distance detector.
//!
//! When a kill event arrives, look at the reported weapon and distance.
//! If the distance is far beyond the maximum effective range of the
//! weapon, that's classic aimbot signature. (Real edge case: a sniper
//! hitting a tower at 800m is fine; an AK-47 hitting at 800m is not.)
//!
//! Ranges are conservative — keyed off the *effective range to land hits
//! on a moving target without lucky-shot RNG*. The pak's weapon datatable
//! has authoritative numbers; for now we use community-mod-server values
//! and cross-reference against the .pak in a follow-up.

use super::{Detector, Event, Severity, Vec3, Verdict};

pub struct KillDistanceDetector;

impl KillDistanceDetector {
    pub fn new() -> Self {
        Self
    }
}

/// Maximum *plausible* hit distance (m) by weapon name fragment. Anything
/// beyond + a 20% slack triggers a flag. Iterate the table as we observe
/// real events.
fn max_range_m(weapon: &str) -> Option<f32> {
    let w = weapon.to_lowercase();
    // Pistols
    if w.contains("m1911") || w.contains("glock") || w.contains("desert") || w.contains(".45") {
        return Some(60.0);
    }
    // SMGs
    if w.contains("mp5") || w.contains("uzi") || w.contains("vector") {
        return Some(120.0);
    }
    // Shotguns
    if w.contains("shotgun") || w.contains("12 ga") || w.contains("buck") {
        return Some(40.0);
    }
    // Battle rifles / assault rifles
    if w.contains("ak-47") || w.contains("ak47") || w.contains("m4") || w.contains("ar-15") || w.contains("aug") || w.contains("g36") {
        return Some(400.0);
    }
    // Marksman / DMR
    if w.contains("vss") || w.contains("svd") || w.contains("m14") || w.contains("scar") {
        return Some(700.0);
    }
    // Sniper rifles
    if w.contains("awp") || w.contains("m24") || w.contains("m200") || w.contains("barrett") || w.contains("kar") || w.contains("mosin") {
        return Some(1500.0);
    }
    // Bows / crossbows
    if w.contains("bow") || w.contains("crossbow") {
        return Some(80.0);
    }
    None
}

impl Detector for KillDistanceDetector {
    fn name(&self) -> &'static str {
        "kill_distance"
    }

    fn evaluate(&mut self, event: &Event) -> Verdict {
        let (ts, weapon, distance_m, killer, victim, killer_steam, killer_pos, victim_pos) = match event {
            Event::Kill {
                ts,
                weapon,
                distance_m,
                killer,
                victim,
                killer_steam,
                killer_pos,
                victim_pos,
                ..
            } => (
                ts,
                weapon.clone().unwrap_or_default(),
                *distance_m,
                killer.clone().unwrap_or_default(),
                victim.clone(),
                killer_steam.clone(),
                *killer_pos,
                *victim_pos,
            ),
            _ => return Verdict::Ok,
        };

        // We need a weapon to evaluate; PvE / suicide / fall pass through.
        if weapon.is_empty() {
            return Verdict::Ok;
        }
        let max = match max_range_m(&weapon) {
            Some(m) => m,
            None => return Verdict::Ok, // unknown weapon — don't false-flag
        };

        // Prefer the explicit `distance_m` field; fall back to computing
        // it from the killer/victim positions if both are present.
        let distance = match (distance_m, killer_pos, victim_pos) {
            (Some(d), _, _) if d > 0.0 => d,
            (_, Some(k), Some(v)) => Vec3::distance_m(&k, &v),
            _ => return Verdict::Ok,
        };

        let slack = max * 1.20;
        if distance <= slack {
            return Verdict::Ok;
        }

        let severity = if distance > slack * 2.0 {
            Severity::Critical
        } else if distance > slack * 1.5 {
            Severity::High
        } else {
            Severity::Medium
        };

        Verdict::Flag {
            reason: format!(
                "kill at {distance:.0}m with {weapon} (max plausible {max:.0}m + 20% slack = {slack:.0}m)"
            ),
            severity,
            evidence: serde_json::json!({
                "ts": ts,
                "killer": killer,
                "killer_steam": killer_steam,
                "victim": victim,
                "weapon": weapon,
                "distance_m": distance,
                "max_plausible_m": max,
                "killer_pos": killer_pos,
                "victim_pos": victim_pos,
            }),
        }
    }
}
