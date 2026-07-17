// Install snapshot — versioned backups of the SCUM Server / Client
// installs so we can roll forward/back between builds without losing
// known-working state.
//
// Storage: C:/TurdMOD-snapshots/<target>/v<build>/  (versioned full
// copies). Uses Windows' robocopy with /MIR for fast directory mirror
// + delta semantics — unchanged files are re-linked (file-attribute
// preserved) rather than re-read, so consecutive snapshots of the
// same install are nearly free.
//
// MVP scope: snapshot server install. Client + restore are
// follow-ups. Restore needs elevation (writes back to Program Files);
// designing that path carefully is its own session.

use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;

const SNAPSHOT_ROOT: &str = r"C:\TurdMOD-snapshots";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotResult {
    pub target: String,            // "server" | "client"
    pub scum_build: String,
    pub source: String,
    pub destination: String,
    pub duration_ms: u128,
    pub ok: bool,
    pub robocopy_exit_code: i32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    pub target: String,
    pub scum_build: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at_iso: String,
}

/// Build the snapshot destination path: `C:/TurdMOD-snapshots/<target>/v<build>/`.
fn dest_for(target: &str, scum_build: &str) -> PathBuf {
    PathBuf::from(SNAPSHOT_ROOT)
        .join(target)
        .join(format!("v{}", scum_build))
}

/// Snapshot the given install directory to a versioned destination.
/// Uses robocopy /MIR for fast mirror with delta semantics.
///
/// `target`: "server" or "client" — namespaces the snapshot store.
/// `source_install`: absolute path to the install root.
/// `scum_build`: build ID from Steam appmanifest (used as the version
///   tag in the destination path).
pub fn snapshot_install(
    target: &str,
    source_install: &PathBuf,
    scum_build: &str,
) -> Result<SnapshotResult, String> {
    if !source_install.exists() {
        return Err(format!("source install missing: {}", source_install.display()));
    }
    if !matches!(target, "server" | "client") {
        return Err(format!("invalid target '{}' (expected server|client)", target));
    }

    let dst = dest_for(target, scum_build);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create snapshot root: {}", e))?;
    }
    std::fs::create_dir_all(&dst)
        .map_err(|e| format!("create snapshot dir: {}", e))?;

    let started = Instant::now();
    // robocopy semantics:
    //   /MIR    — mirror; delete extra files at dest
    //   /XJ     — exclude junction points (don't follow Windows reparse points)
    //   /NFL    — no per-file list
    //   /NDL    — no per-directory list
    //   /NJH    — no job header
    //   /NJS    — no job summary
    //   /NP     — no progress bar
    //   /R:1    — retry once on transient failures
    //   /W:1    — wait 1s between retries
    //   /MT:8   — 8 threads
    let output = std::process::Command::new("robocopy")
        .arg(source_install.as_os_str())
        .arg(&dst)
        .args([
            "/MIR", "/XJ", "/NFL", "/NDL", "/NJH", "/NJS", "/NP",
            "/R:1", "/W:1", "/MT:8",
        ])
        .output()
        .map_err(|e| format!("spawn robocopy: {}", e))?;

    let duration_ms = started.elapsed().as_millis();
    // robocopy exit codes are bitmasks: 0-7 = success, 8+ = error.
    // 1 = one or more files copied successfully (most common path)
    // 0 = no copy required, source/dst already in sync
    // 2 = extra files/dirs detected (when /MIR removed them at dst)
    // 3 = some files copied + some extras detected
    let exit_code = output.status.code().unwrap_or(-1);
    let ok = exit_code < 8;
    let summary = if ok {
        match exit_code {
            0 => "no changes — destination already in sync",
            1 => "files copied successfully",
            2 => "extra files at destination cleaned up",
            3 => "files copied + extras at destination cleaned up",
            _ => "completed with non-fatal flags",
        }
    } else {
        "robocopy reported errors — check the destination manually"
    };

    Ok(SnapshotResult {
        target: target.to_string(),
        scum_build: scum_build.to_string(),
        source: source_install.to_string_lossy().into_owned(),
        destination: dst.to_string_lossy().into_owned(),
        duration_ms,
        ok,
        robocopy_exit_code: exit_code,
        summary: summary.to_string(),
    })
}

/// List all available snapshots for one or both targets.
pub fn list_snapshots(target_filter: Option<&str>) -> Vec<SnapshotEntry> {
    let mut out = Vec::new();
    let root = PathBuf::from(SNAPSHOT_ROOT);
    if !root.exists() {
        return out;
    }

    let targets: Vec<&str> = match target_filter {
        Some("server") => vec!["server"],
        Some("client") => vec!["client"],
        _ => vec!["server", "client"],
    };

    for target in targets {
        let target_dir = root.join(target);
        if !target_dir.exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&target_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('v') {
                continue;
            }
            let build = name.trim_start_matches('v').to_string();

            // Size + created time
            let size_bytes = dir_size(&path);
            let created_at_iso = entry.metadata()
                .and_then(|m| m.created())
                .ok()
                .and_then(|t| {
                    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
                    Some(iso_from_unix_secs(secs))
                })
                .unwrap_or_else(|| "unknown".to_string());

            out.push(SnapshotEntry {
                target: target.to_string(),
                scum_build: build,
                path: path.to_string_lossy().into_owned(),
                size_bytes,
                created_at_iso,
            });
        }
    }

    // Newest first (by build id, lexicographic — works for SCUM's
    // monotonic-increasing numeric build IDs)
    out.sort_by(|a, b| b.scum_build.cmp(&a.scum_build));
    out
}

fn dir_size(path: &PathBuf) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total = total.saturating_add(dir_size(&p));
            } else if let Ok(m) = entry.metadata() {
                total = total.saturating_add(m.len());
            }
        }
    }
    total
}

fn iso_from_unix_secs(secs: u64) -> String {
    // Cheap ISO-8601 UTC formatter without pulling chrono just for this.
    // Reasonable for snapshot UI; not for protocol-level timestamps.
    let days_from_epoch = (secs / 86400) as i64;
    let sec_of_day = (secs % 86400) as u32;
    let h = sec_of_day / 3600;
    let m = (sec_of_day % 3600) / 60;
    let s = sec_of_day % 60;
    // Convert days to Y/M/D via the civil-from-days algorithm
    // (Howard Hinnant). Public domain.
    let z = days_from_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d = doy - (153*mp + 2)/5 + 1;
    let mon = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + (if mon <= 2 { 1 } else { 0 });
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, mon, d, h, m, s)
}
