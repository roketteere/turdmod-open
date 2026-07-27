// phantom.rs — synthetic population padding for the map snapshot ONLY.
//
// Pads the REPORTED player list (/map/players count + map markers) up to a configured
// floor with stable, unique dummy players that drift slowly so they read as roaming.
//
// @inv: phantoms exist ONLY in the map snapshot. The operational player list
//   (getOnlinePlayers — used by chat_cmds, welcome, spa, economy, teleport, etc.) is
//   never padded, so no gameplay logic ever tries to act on a fake player.
// @ctx: server-population display tactic (appear ~N populated to attract real joins).
//   Deception risk is the operator's call — flagged to Joel.

use crate::map_tracker::PlayerPosition;

// Unique dummy SteamID64 range. 7656119900000xxxx is valid Steam space, distinct from
// real players (which sit in the 76561198xxxxxxxxx era), so they never collide.
const STEAMID_BASE: u64 = 76561199000000000;

// SCUM island land box (cm) — kept inside the ocean border so markers land on terrain,
// not floating in the sea (a dead giveaway). Sample taken from a real spawn @ (-438950,-378716).
const MIN: f64 = -550000.0;
const MAX: f64 = 550000.0;
const GROUND_Z: f64 = 10000.0;

// Plausible survivor handles (≥ any reasonable floor, so each slot is unique without a
// numeric suffix until the pool is exhausted).
const NAMES: &[&str] = &[
    "Ghost", "Reaper", "Viper", "Nomad", "Husk", "Crow", "Ranger", "Drifter", "Wolf",
    "Ash", "Saint", "Rook", "Echo", "Slate", "Maverick", "Cobra", "Talon", "Bandit",
    "Hawk", "Frost", "Vandal", "Specter", "Rogue", "Jackal", "Dozer", "Knox", "Riggs",
    "Vega", "Brick", "Dagger", "Wraith", "Hollow", "Marlow", "Creed", "Tex", "Wilder",
    "Sable", "Onyx", "Cain", "Reyes", "Shade", "Boon", "Quill", "Harlan", "Dane",
    "Striker", "Lurch", "Grit", "Mako", "Vox", "Kane", "Brisk", "Holt", "Ferro",
    "Crank", "Dusty", "Roan", "Pike", "Smoke", "Zane", "Burr", "Cleat", "Hatch", "Lash",
];

// Deterministic pseudo-random in [0,1) from a seed — no RNG state, stable per slot.
fn h(seed: u64) -> f64 {
    let x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((x >> 33) as u32 as f64) / (u32::MAX as f64)
}

/// Build the phantom fill for the snapshot: enough dummies to bring `real.len()` up to
/// `floor`. Returns empty when real population already meets/exceeds the floor (real
/// players are never hidden). `tick` advances the slow drift between polls.
pub fn fill(real: &[PlayerPosition], floor: usize, tick: u64) -> Vec<PlayerPosition> {
    if floor == 0 || real.len() >= floor {
        return Vec::new();
    }
    let need = floor - real.len();
    (0..need)
        .map(|i| {
            let idx = i as u64;
            let bx = MIN + h(idx * 2 + 1) * (MAX - MIN);
            let by = MIN + h(idx * 2 + 7) * (MAX - MIN);
            // slow per-slot drift (~±3km) so markers move a little each 10s poll
            let t = tick as f64;
            let jx = (t * 0.03 + idx as f64).sin() * 3000.0;
            let jy = (t * 0.027 + idx as f64 * 1.3).cos() * 3000.0;
            let name = if (idx as usize) < NAMES.len() {
                NAMES[idx as usize].to_string()
            } else {
                format!("{}{}", NAMES[idx as usize % NAMES.len()], idx / NAMES.len() as u64 + 1)
            };
            PlayerPosition {
                name,
                steam_id: (STEAMID_BASE + idx + 1).to_string(),
                x: bx + jx,
                y: by + jy,
                z: GROUND_Z,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_to_floor_unique_on_map() {
        let real = vec![PlayerPosition {
            name: crate::owner::name().to_string(),
            steam_id: format!("{}", STEAMID_BASE),
            x: -438950.0, y: -378716.0, z: 11516.0,
        }];
        let ph = fill(&real, 50, 7);
        assert_eq!(ph.len(), 49, "50 floor minus 1 real");
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for p in &ph {
            assert!(ids.insert(p.steam_id.clone()), "dup steamid {}", p.steam_id);
            assert!(names.insert(p.name.clone()), "dup name {}", p.name);
            assert!(p.steam_id.starts_with("76561199"), "id not in dummy range");
            assert!(p.x >= MIN - 4000.0 && p.x <= MAX + 4000.0, "x off-map {}", p.x);
            assert!(p.y >= MIN - 4000.0 && p.y <= MAX + 4000.0, "y off-map {}", p.y);
        }
    }

    #[test]
    fn never_hides_real_players() {
        let many: Vec<PlayerPosition> = (0..60)
            .map(|i| PlayerPosition { name: format!("P{i}"), steam_id: format!("{i}"), x: 0.0, y: 0.0, z: 0.0 })
            .collect();
        assert_eq!(fill(&many, 50, 1).len(), 0, "real >= floor -> no phantoms");
        assert_eq!(fill(&[], 0, 1).len(), 0, "floor 0 = off");
    }

    #[test]
    fn drifts_between_ticks() {
        let a = fill(&[], 50, 1);
        let b = fill(&[], 50, 2);
        assert_ne!(a[0].x, b[0].x, "positions should drift across ticks");
        assert_eq!(a[0].steam_id, b[0].steam_id, "but identity stays stable");
    }
}
