//! Persistent loader log — append-only NDJSON-ish lines at
//! `%LOCALAPPDATA%/TurdMOD/loader.log`. Same file as the CLI's detection
//! layer writes to (#170), so a single audit trail captures both
//! "user launched the loader" and "DLL was loaded into SCUM.exe".
//!
//! We deliberately never use `println!` or stdout — SCUM doesn't have a
//! console and Windows discards anything written to a closed handle.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use parking_lot::Mutex;
use once_cell::sync::Lazy;

static LOG_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn log_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let mut p = PathBuf::from(local);
    p.push("TurdMOD");
    create_dir_all(&p).ok()?;
    p.push("loader.log");
    Some(p)
}

pub fn log(msg: &str) {
    let _g = LOG_LOCK.lock();
    let line = format!("{} {}\n", Utc::now().to_rfc3339(), msg);
    if let Some(path) = log_path() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
