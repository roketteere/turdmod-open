//! Append-only log at %LOCALAPPDATA%/TurdMOD/decorators.log. Distinct from
//! the kitchen-sink loader's loader.log so the two DLLs' logs don't
//! interleave when a player runs both side-by-side.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

static LOG_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn log_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let mut p = PathBuf::from(local);
    p.push("TurdMOD");
    create_dir_all(&p).ok()?;
    p.push("decorators.log");
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
