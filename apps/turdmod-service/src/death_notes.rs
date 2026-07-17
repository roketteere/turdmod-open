// Death notes - on every kill, persist a forensic record (who/what/where/when/
// how + inventory-at-death) to death_notes.json. Backs the TMM map death markers
// + Death Note panel. Event-driven (`kill`).
// @inv inventory MUST be snapshotted at the kill event — after death the corpse
//      reassigns item ownership and player_inventory returns empty.
// @dep scumdb::{player_inventory, sector_of, profile_id_for_name, steam_for_profile_id}.
// @ctx kill events originate from turdmod-companion (log tail), rebroadcast via
//      the bridge pipe; victimSteam in the payload is a SCUM prisoner id, NOT a
//      Steam64 — resolve the real Steam64 by victim name.

use crate::events::GameEvent;
use crate::registry::{Mod, ModCtx, Outcome};

const PATH: &str = r"C:\TurdMOD\data\death_notes.json";
const MAX_NOTES: usize = 500;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct DeathNotes;
impl DeathNotes {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Mod for DeathNotes {
    fn name(&self) -> &'static str { "death_notes" }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "kill" { return Outcome::Ignored; }
        let s = |k: &str| ev.data.get(k).and_then(|v| v.as_str()).map(|x| x.to_string());

        let victim = match s("victim") { Some(v) if !v.is_empty() => v, _ => return Outcome::Ignored };
        let killer = s("killer");
        let weapon = s("weapon");
        let headshot = ev.data.get("headshot").and_then(|v| v.as_bool()).unwrap_or(false);
        // service-side event carries `distance` (cm, per death_recap); companion's
        // raw `distanceM` is the fallback.
        let distance_m = ev
            .data.get("distance").and_then(|v| v.as_f64()).map(|cm| cm / 100.0)
            .or_else(|| ev.data.get("distanceM").and_then(|v| v.as_f64()));

        // Death location: victim's position, falling back to the killer's.
        let pos = ev.data.get("victimPos").or_else(|| ev.data.get("killerPos"));
        let coord = |axis: &str| pos.and_then(|p| p.get(axis)).and_then(|v| v.as_f64());
        let (x, y, z) = (coord("x"), coord("y"), coord("z"));
        let sector = match (x, y) {
            (Some(x), Some(y)) => crate::scumdb::sector_of(x, y),
            _ => String::new(),
        };

        let db = crate::scumdb::db_path(None).to_string();
        // Resolve the victim's real Steam64 by name (payload carries a prisoner id).
        let steam = crate::scumdb::profile_id_for_name(&db, &victim).ok().flatten()
            .and_then(|pid| crate::scumdb::steam_for_profile_id(&db, pid).ok().flatten());

        // SNAPSHOT inventory NOW, before the corpse/drop reassigns ownership.
        let inventory = steam.as_deref()
            .and_then(|st| crate::scumdb::player_inventory(&db, st).ok())
            .and_then(|inv| inv.get("items").cloned())
            .unwrap_or_else(|| serde_json::json!([]));

        // Cause: PvP when a real, non-self killer is named; else environment/PvE.
        let cause = match killer.as_deref() {
            Some(k) if !k.is_empty() && !k.eq_ignore_ascii_case(&victim) && !k.eq_ignore_ascii_case("none") => "pvp",
            _ => "environment",
        };

        let note = serde_json::json!({
            "at": now(),
            "victim": victim,
            "victim_steam": steam,
            "killer": killer,
            "killer_steam": s("killerSteam"),
            "cause": cause,
            "weapon": weapon,
            "headshot": headshot,
            "distance_m": distance_m.map(|d| d.round()),
            "x": x, "y": y, "z": z,
            "sector": sector,
            "inventory": inventory,
        });

        // Append, capping to MAX_NOTES (drop oldest). Persisted so markers survive
        // restarts until the admin clears them in TMM.
        let mut arr: Vec<serde_json::Value> = std::fs::read_to_string(PATH).ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        arr.push(note);
        let len = arr.len();
        if len > MAX_NOTES { arr.drain(0..len - MAX_NOTES); }
        let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
        if let Ok(j) = serde_json::to_string_pretty(&arr) {
            let tmp = format!("{}.tmp", PATH);
            if std::fs::write(&tmp, &j).is_ok() { let _ = std::fs::rename(&tmp, PATH); }
        }
        Outcome::Handled
    }
}
