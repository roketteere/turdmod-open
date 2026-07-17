// World checkpoint + wipe guard.
//
// Two tiers, both fed from the live SCUM.db (the single file where ALL world state lives —
// players, stats, skills, levels, attributes, vehicles, structures, ownership, fame):
//   1. ROLLING checkpoints every 30 min -> C:\TurdMOD\checkpoints\<ts>\ (last MAX_CHECKPOINTS).
//      Cheap "roll back the last few hours". Also fired before save-edits (skills/attributes).
//   2. RETAINED archives -> C:\TurdMOD\archives\<reason>-<ts>\ that DON'T roll off at the 30-min
//      cadence (pruned only at the generous MAX_ARCHIVES). These are the wipe-proof safety net:
//        - Sentinel writes a `preupdate-*` archive the instant a SCUM build change is detected,
//          BEFORE the update can wipe (turdmod-updater does this directly).
//        - The wipe guard below writes a `prewipe-auto` archive of the last-good checkpoint the
//          instant the live DB collapses (force-wipe / corruption / surprise reset), so the good
//          data is preserved before the rolling window eats it — then escalates to the ops loop.
//
// @inv: SCUM.db is WAL-mode; we copy SCUM.db + -wal + -shm together so a hot snapshot is coherent.
// @dep: engine.rs save-edit paths call do_checkpoint(); turdmod-updater writes preupdate archives.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::events::GameEvent;
use crate::registry::{Mod, ModCtx, Outcome};

const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1800);
const SCUM_DB: &str = r"C:\SCUMServer\SCUM\Saved\SaveFiles\SCUM.db";
const TURDMOD_DATA: &str = r"C:\TurdMOD\data";
const CHECKPOINT_DIR: &str = r"C:\TurdMOD\checkpoints";
const KEEPERS_DIR: &str = r"C:\TurdMOD\keepers";
const ARCHIVE_DIR: &str = r"C:\TurdMOD\archives";
const SENTINEL_ESCALATIONS: &str = r"C:\TurdMOD\sentinel\escalations";
const MAX_CHECKPOINTS: usize = 10;
const MAX_KEEPERS: usize = 14; // ~2 weeks of clean daily keepers
const KEEPER_MIN_INTERVAL_SECS: u64 = 72_000; // 20h — fires on the first restart of each day
const MAX_ARCHIVES: usize = 40;
// Archive dirs whose names start with these prefixes are PERMANENT — never pruned (the
// pre-update snapshots are the crown jewels of wipe recovery).
const ARCHIVE_PROTECT: &[&str] = &["preupdate-"];
// Wipe heuristic: a live DB smaller than WIPE_RATIO of the previous checkpoint's DB (which must
// itself be over WIPE_FLOOR to be a meaningful baseline) = suspected wipe. The action is purely
// PRESERVE + alert (non-destructive), so a rare false positive just makes one extra archive.
const WIPE_FLOOR_BYTES: u64 = 5_000_000;
const WIPE_RATIO: f64 = 0.4;

fn timestamp() -> String {
    format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
}

fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Copy SCUM.db + its -wal/-shm companions into `dest`. Returns files copied.
fn copy_db_set(dest: &Path) -> u32 {
    let mut n = 0u32;
    for suffix in ["", "-wal", "-shm"] {
        let src = PathBuf::from(format!("{}{}", SCUM_DB, suffix));
        if src.exists() {
            let name = src.file_name().unwrap_or_default();
            if std::fs::copy(&src, dest.join(name)).is_ok() { n += 1; }
        }
    }
    n
}

/// Copy every file in TURDMOD_DATA (shallow) into `dest`. Returns files copied.
fn copy_turdmod_data(dest: &Path) -> u32 {
    let mut n = 0u32;
    if let Ok(entries) = std::fs::read_dir(TURDMOD_DATA) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default();
                if std::fs::copy(&path, dest.join(name)).is_ok() { n += 1; }
            }
        }
    }
    n
}

fn prune(dir: &str, keep: usize, protect: &[&str]) {
    if let Ok(dirs) = std::fs::read_dir(dir) {
        let mut ds: Vec<PathBuf> = dirs.flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !protect.iter().any(|pre| name.starts_with(pre)) // permanent dirs are never pruned
            })
            .collect();
        ds.sort(); // timestamp-named -> chronological
        while ds.len() > keep { let old = ds.remove(0); let _ = std::fs::remove_dir_all(old); }
    }
}

/// Age (secs) of the newest timestamp-named subdir in `dir`, or None if empty/missing.
fn newest_dir_age_secs(dir: &str) -> Option<u64> {
    let mut ds: Vec<u64> = std::fs::read_dir(dir).ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.path().file_name()?.to_str()?.parse::<u64>().ok())
        .collect();
    ds.sort();
    let newest = ds.pop()?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(now.saturating_sub(newest))
}

/// Clean daily keeper — call from engine::start_server PRE-START (SCUM stopped = coherent snapshot,
/// unlike the hot 30-min rolling checkpoints). Takes at most one per ~day; retains MAX_KEEPERS.
/// These are the consistent long-term restore points and the primary off-box (Google Drive) target.
pub fn do_keeper_if_due() {
    if let Some(age) = newest_dir_age_secs(KEEPERS_DIR) {
        if age < KEEPER_MIN_INTERVAL_SECS { return; }
    }
    let dest = PathBuf::from(KEEPERS_DIR).join(timestamp());
    if std::fs::create_dir_all(&dest).is_err() { return; }
    let count = copy_db_set(&dest) + copy_turdmod_data(&dest);
    prune(KEEPERS_DIR, MAX_KEEPERS, &[]);
    tracing::info!("[checkpoint] clean daily keeper saved ({} files) -> {}", count, dest.display());
}

/// The most recent rolling checkpoint dir, if any.
fn latest_checkpoint() -> Option<PathBuf> {
    let mut cp: Vec<PathBuf> = std::fs::read_dir(CHECKPOINT_DIR).ok()?
        .flatten().filter(|e| e.path().is_dir()).map(|e| e.path()).collect();
    cp.sort();
    cp.pop()
}

// Public so save-edit paths (e.g. skill editing) can force a fresh scum.db backup to our
// external location BEFORE mutating the save — always reversible, never a forced wipe.
pub fn do_checkpoint() -> Result<(String, u32), String> {
    let ts = timestamp();
    let dest = PathBuf::from(CHECKPOINT_DIR).join(&ts);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let count = copy_db_set(&dest) + copy_turdmod_data(&dest);
    prune(CHECKPOINT_DIR, MAX_CHECKPOINTS, &[]);
    Ok((ts, count))
}

/// Retained snapshot of the LIVE SCUM.db (+ companions + TurdMOD data) into archives/<reason>-<ts>.
/// Pruned only at MAX_ARCHIVES. Used for manual / pre-update / "snapshot now" captures.
pub fn do_archive_live(reason: &str) -> Result<String, String> {
    let dest = PathBuf::from(ARCHIVE_DIR).join(format!("{}-{}", reason, timestamp()));
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let count = copy_db_set(&dest) + copy_turdmod_data(&dest);
    prune(ARCHIVE_DIR, MAX_ARCHIVES, ARCHIVE_PROTECT);
    Ok(format!("{} ({} files)", dest.display(), count))
}

/// Retain an existing checkpoint dir as an archive (used by the wipe guard to preserve the
/// last-good, pre-wipe checkpoint before the rolling window prunes it).
fn archive_existing(src_dir: &Path, reason: &str) -> Result<String, String> {
    let dest = PathBuf::from(ARCHIVE_DIR).join(format!("{}-{}", reason, timestamp()));
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    if let Ok(entries) = std::fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                let name = p.file_name().unwrap_or_default();
                let _ = std::fs::copy(&p, dest.join(name));
            }
        }
    }
    prune(ARCHIVE_DIR, MAX_ARCHIVES, ARCHIVE_PROTECT);
    Ok(dest.display().to_string())
}

/// Drop an escalation file the ops loop (Claude) polls — same contract as turdmod-updater.
fn escalate_wipe(detail: &str) {
    let _ = std::fs::create_dir_all(SENTINEL_ESCALATIONS);
    let ts = timestamp();
    let body = serde_json::json!({ "at": ts.parse::<u64>().unwrap_or(0), "kind": "db_wipe_detected", "detail": detail });
    if let Ok(j) = serde_json::to_string_pretty(&body) {
        let _ = std::fs::write(PathBuf::from(SENTINEL_ESCALATIONS).join(format!("{}-db_wipe_detected.json", ts)), j);
    }
    tracing::warn!("[checkpoint] ESCALATION db_wipe_detected: {}", detail);
}

pub struct Checkpoint;
impl Checkpoint { pub fn new() -> Self { Self } }

#[async_trait::async_trait]
impl Mod for Checkpoint {
    fn name(&self) -> &'static str { "checkpoint" }
    fn interval(&self) -> Option<Duration> { Some(CHECKPOINT_INTERVAL) }

    // Silent backup every 30 min — no player-facing broadcast (Joel: don't flood). Before the
    // rolling snapshot, the wipe guard checks whether the live DB just collapsed vs the last
    // checkpoint; if so it preserves that pre-wipe checkpoint as a retained archive + escalates.
    async fn tick(&self, _ctx: &ModCtx) {
        let live = file_size(Path::new(SCUM_DB));
        if let Some(prev) = latest_checkpoint() {
            let prev_db = file_size(&prev.join("SCUM.db"));
            if prev_db > WIPE_FLOOR_BYTES && (live as f64) < (prev_db as f64) * WIPE_RATIO {
                match archive_existing(&prev, "prewipe-auto") {
                    Ok(p) => escalate_wipe(&format!(
                        "SCUM.db collapsed {} -> {} bytes; preserved pre-wipe archive {}", prev_db, live, p
                    )),
                    Err(e) => tracing::error!("[checkpoint] wipe-preserve failed: {}", e),
                }
            }
        }
        match do_checkpoint() {
            Ok((ts, count)) => tracing::info!("checkpoint: {} - {} files backed up", ts, count),
            Err(e) => tracing::warn!("checkpoint failed: {}", e),
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}
