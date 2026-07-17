//! Persistent server-loader log — append-only lines at
//! `%PROGRAMDATA%\TurdMOD\server-loader.log`.
//!
//! Uses %PROGRAMDATA% (typically `C:\ProgramData`) rather than
//! %LOCALAPPDATA% because SCUMServer.exe commonly runs under a service
//! account or a dedicated user whose LOCALAPPDATA differs from the
//! operator's desktop session. ProgramData is machine-wide and always
//! writable by processes running under a standard server user.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use once_cell::sync::Lazy;
use parking_lot::Mutex;

static LOG_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn log_path() -> Option<PathBuf> {
    let programdata = std::env::var_os("PROGRAMDATA")?;
    let mut p = PathBuf::from(programdata);
    p.push("TurdMOD");
    create_dir_all(&p).ok()?;
    p.push("server-loader.log");
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
