//! Sentinel — SCUM update orchestrator (Phase 1: detection + ledger + escalation).
//!
//! Watches the local Steam SCUM build manifests for changes (the CLIENT app 513710 is the canary —
//! it auto-updates the moment a new SCUM build ships; the SERVER 3792580 follows via steamcmd). On a
//! build change it records the event to a ledger and writes an ESCALATION so the autonomous ops loop
//! (a Claude session) picks it up and runs the patch/dump/redeploy pipeline with judgment — never
//! waking Joel. Phase 2/3 add the steamcmd update + dump-tool + OVH-deploy steps; until those are
//! tested, detection + escalate is the SAFE behavior (never miss an update, never blindly auto-patch
//! prod). Run periodically (scheduled task or the ops session). @dep [[reference_scum_build_numbering]].

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const STEAMAPPS: &str = r"C:\Program Files (x86)\Steam\steamapps";
const SENTINEL_DIR: &str = r"C:\TurdMOD\sentinel";
const SCUM_DB: &str = r"C:\SCUMServer\SCUM\Saved\SaveFiles\SCUM.db";
const ARCHIVE_DIR: &str = r"C:\TurdMOD\archives";
const MAX_ARCHIVES: usize = 40;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Parse `"buildid"  "NNNN"` out of a Steam appmanifest .acf.
fn read_buildid(manifest: &str) -> Option<String> {
    let s = fs::read_to_string(Path::new(STEAMAPPS).join(manifest)).ok()?;
    for line in s.lines() {
        if line.contains("\"buildid\"") {
            let parts: Vec<&str> = line.split('"').filter(|x| !x.trim().is_empty()).collect();
            if let Some(b) = parts.last() {
                let b = b.trim();
                if !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()) {
                    return Some(b.to_string());
                }
            }
        }
    }
    None
}

#[derive(Serialize, Deserialize, Clone)]
struct Event {
    at: u64,
    kind: String,
    detail: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Ledger {
    #[serde(default)]
    last_server_build: String,
    #[serde(default)]
    last_client_build: String,
    #[serde(default)]
    events: Vec<Event>,
}

fn ledger_path() -> PathBuf { Path::new(SENTINEL_DIR).join("ledger.json") }

fn load_ledger() -> Ledger {
    fs::read_to_string(ledger_path()).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_ledger(l: &Ledger) {
    let _ = fs::create_dir_all(SENTINEL_DIR);
    if let Ok(j) = serde_json::to_string_pretty(l) {
        let _ = fs::write(ledger_path(), j);
    }
}

/// Drop an escalation file the ops loop (Claude) polls + acts on. Stdout/stderr too for logs.
fn escalate(kind: &str, detail: &str) {
    let dir = Path::new(SENTINEL_DIR).join("escalations");
    let _ = fs::create_dir_all(&dir);
    let e = Event { at: now(), kind: kind.into(), detail: detail.into() };
    if let Ok(j) = serde_json::to_string_pretty(&e) {
        let _ = fs::write(dir.join(format!("{}-{}.json", now(), kind)), j);
    }
    eprintln!("[sentinel] ESCALATION [{}] {}", kind, detail);
}

/// Retained snapshot of the live SCUM.db (+ WAL/SHM) the instant a build change is detected —
/// BEFORE the server update can wipe. The client app (513710) is the canary that updates first, so
/// at detection time the live SCUM.db is still the good pre-update world. Never rolled off by the
/// 30-min checkpoint cadence (pruned only at MAX_ARCHIVES). Self-contained file copy (no service
/// dependency) so it fires even if turdmod-service is wedged. @dep checkpoint.rs shares archives/.
fn preupdate_snapshot(tag: &str) {
    let dest = Path::new(ARCHIVE_DIR).join(format!("preupdate-{}-{}", tag, now()));
    if let Err(e) = fs::create_dir_all(&dest) {
        eprintln!("[sentinel] pre-update snapshot mkdir failed: {}", e);
        return;
    }
    let mut n = 0u32;
    for suffix in ["", "-wal", "-shm"] {
        let src = format!("{}{}", SCUM_DB, suffix);
        let sp = Path::new(&src);
        if sp.exists() {
            if let Some(name) = sp.file_name() {
                if fs::copy(sp, dest.join(name)).is_ok() { n += 1; }
            }
        }
    }
    if let Ok(dirs) = fs::read_dir(ARCHIVE_DIR) {
        let mut ds: Vec<PathBuf> = dirs.flatten().filter(|e| e.path().is_dir()).map(|e| e.path()).collect();
        ds.sort();
        while ds.len() > MAX_ARCHIVES { let old = ds.remove(0); let _ = fs::remove_dir_all(old); }
    }
    eprintln!("[sentinel] PRE-UPDATE snapshot: {} files -> {}", n, dest.display());
}

fn main() {
    let server = read_buildid("appmanifest_3792580.acf").unwrap_or_else(|| "?".into());
    let client = read_buildid("appmanifest_513710.acf").unwrap_or_else(|| "?".into());

    let mut l = load_ledger();
    let first = l.last_client_build.is_empty() && l.last_server_build.is_empty();
    let mut changed = false;

    if !l.last_client_build.is_empty() && l.last_client_build != client {
        // FIRST: grab a retained pre-update snapshot before the server build follows and can wipe.
        preupdate_snapshot(&format!("{}-to-{}", l.last_client_build, client));
        let d = format!("SCUM CLIENT build {} -> {} (new build shipped — server patch + dump + redeploy needed; pre-update SCUM.db snapshot archived)",
            l.last_client_build, client);
        l.events.push(Event { at: now(), kind: "client_build_change".into(), detail: d.clone() });
        escalate("scum_update", &d);
        changed = true;
    }
    if !l.last_server_build.is_empty() && l.last_server_build != server {
        let d = format!("SCUM SERVER build {} -> {} (server install patched)", l.last_server_build, server);
        l.events.push(Event { at: now(), kind: "server_build_change".into(), detail: d.clone() });
        changed = true;
    }

    l.last_client_build = client.clone();
    l.last_server_build = server.clone();
    let n = l.events.len();
    if n > 200 { l.events.drain(0..n - 200); }
    save_ledger(&l);

    let status = if first { "(baseline recorded)" } else if changed { "(CHANGE -> escalated)" } else { "(no change)" };
    println!("[sentinel] server={} client={} {}", server, client, status);
}
