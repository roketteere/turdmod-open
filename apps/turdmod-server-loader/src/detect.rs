//! Process detection for the server-side loader.
//!
//! Simpler than the client loader's detect.rs: we only care whether we
//! are running inside `GameServer.exe`. There is no BattlEye on the
//! server side — BE is a client-side anti-cheat component and is never
//! present in a dedicated server install.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ServerProcess,
    Wrong,
}

pub fn is_scum_server_process() -> bool {
    let exe = std::env::current_exe().unwrap_or(PathBuf::new());
    exe.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("GameServer.exe"))
        .unwrap_or(false)
}

pub fn infer_mode() -> Mode {
    if is_scum_server_process() {
        Mode::ServerProcess
    } else {
        Mode::Wrong
    }
}
