// Local install — the common case: SCUM server on this machine.
//
// Places the three artifacts where the loader/UE4SS expect them, writes a
// service.json with real paths and a generated token, then installs and starts
// the Windows Service.
//
// @inv: layout must match what turdmod_server_loader + UE4SS look for:
//   <root>/SCUM/Binaries/Win64/turdmod_server_loader.dll
//   <root>/SCUM/Binaries/Win64/UE4SS/UE4SS.dll
//   <root>/SCUM/Binaries/Win64/UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll
//   <root>/SCUM/Binaries/Win64/UE4SS/Mods/TurdMODEngineBridge/enabled.txt

use serde::Serialize;
use std::path::{Path, PathBuf};

const TURDMOD_DIR: &str = r"C:\TurdMOD";

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub step: String,
    pub ok: bool,
    pub detail: String,
}

impl StepResult {
    fn ok(step: &str, detail: impl Into<String>) -> Self {
        Self { step: step.into(), ok: true, detail: detail.into() }
    }
    fn fail(step: &str, detail: impl Into<String>) -> Self {
        Self { step: step.into(), ok: false, detail: detail.into() }
    }
}

/// Generate a URL-safe random token for the service API.
/// Not cryptographic-grade RNG, but the token only gates a localhost/LAN API
/// and is regenerated per install — sufficient, and avoids a crypto dep.
pub fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id() as u128;
    let mut seed = nanos ^ (pid << 64) ^ (nanos.rotate_left(37));
    const CHARS: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    (0..40)
        .map(|_| {
            // xorshift
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            CHARS[(seed % CHARS.len() as u128) as usize] as char
        })
        .collect()
}

fn win64(root: &str) -> PathBuf {
    Path::new(root).join("SCUM").join("Binaries").join("Win64")
}

/// Where the Server Pack was extracted. We look next to the running exe first
/// (the pack ships Setup alongside the artifacts), then C:\TurdMOD.
pub fn find_artifacts_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
            if let Some(up) = dir.parent() {
                candidates.push(up.to_path_buf());
            }
        }
    }
    candidates.push(PathBuf::from(TURDMOD_DIR));

    candidates.into_iter().find(|d| d.join("turdmod-service.exe").exists())
}

/// Build the service.json contents for a detected install.
pub fn build_service_config(server_root: &str, token: &str, port: u16) -> serde_json::Value {
    let w = win64(server_root);
    serde_json::json!({
        "port": port,
        "token": token,
        "scum_exe": w.join("GameServer.exe").display().to_string(),
        "inject_dlls": [
            w.join("UE4SS").join("UE4SS.dll").display().to_string(),
            w.join("turdmod_server_loader.dll").display().to_string(),
        ],
        "auto_restart": true,
        "restart_interval_hours": 6,
        "restart_countdown_seconds": 60
    })
}

fn copy_into(src: &Path, dst: &Path, results: &mut Vec<StepResult>, label: &str) -> bool {
    if !src.exists() {
        results.push(StepResult::fail(label, format!("missing source: {}", src.display())));
        return false;
    }
    if let Some(parent) = dst.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            results.push(StepResult::fail(label, format!("mkdir failed: {e}")));
            return false;
        }
    }
    match std::fs::copy(src, dst) {
        Ok(_) => {
            results.push(StepResult::ok(label, dst.display().to_string()));
            true
        }
        Err(e) => {
            // The classic: DLL is locked because the server is running.
            let hint = if e.raw_os_error() == Some(32) {
                " — the file is in use. Stop the SCUM server and try again."
            } else {
                ""
            };
            results.push(StepResult::fail(label, format!("copy failed: {e}{hint}")));
            false
        }
    }
}

/// Copy artifacts into the server install. Does not touch the service.
pub fn place_artifacts(server_root: &str, artifacts: &Path) -> Vec<StepResult> {
    let mut r = Vec::new();
    let w = win64(server_root);

    copy_into(
        &artifacts.join("turdmod_server_loader.dll"),
        &w.join("turdmod_server_loader.dll"),
        &mut r,
        "Loader DLL",
    );

    let ue4ss_src = artifacts.join("UE4SS").join("UE4SS.dll");
    if ue4ss_src.exists() {
        copy_into(&ue4ss_src, &w.join("UE4SS").join("UE4SS.dll"), &mut r, "UE4SS");
    } else {
        r.push(StepResult::ok("UE4SS", "not in pack — assuming already installed"));
    }

    let bridge_dst = w
        .join("UE4SS")
        .join("Mods")
        .join("TurdMODEngineBridge")
        .join("dlls")
        .join("main.dll");
    copy_into(
        &artifacts.join("UE4SS").join("Mods").join("TurdMODEngineBridge").join("dlls").join("main.dll"),
        &bridge_dst,
        &mut r,
        "Engine bridge",
    );

    // UE4SS only checks that enabled.txt exists; contents are ignored.
    let enabled = w.join("UE4SS").join("Mods").join("TurdMODEngineBridge").join("enabled.txt");
    if let Some(p) = enabled.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    match std::fs::write(&enabled, "") {
        Ok(_) => r.push(StepResult::ok("Enable bridge", enabled.display().to_string())),
        Err(e) => r.push(StepResult::fail("Enable bridge", format!("{e}"))),
    }

    r
}

/// Write C:\TurdMOD\service.json and copy the service exe next to it.
pub fn place_service(artifacts: &Path, config: &serde_json::Value) -> Vec<StepResult> {
    let mut r = Vec::new();
    let dir = PathBuf::from(TURDMOD_DIR);

    if let Err(e) = std::fs::create_dir_all(&dir) {
        r.push(StepResult::fail("Create C:\\TurdMOD", format!("{e}")));
        return r;
    }

    copy_into(
        &artifacts.join("turdmod-service.exe"),
        &dir.join("turdmod-service.exe"),
        &mut r,
        "Service executable",
    );

    let cfg_path = dir.join("service.json");
    match serde_json::to_string_pretty(config) {
        Ok(s) => match std::fs::write(&cfg_path, s) {
            Ok(_) => r.push(StepResult::ok("Configuration", cfg_path.display().to_string())),
            Err(e) => r.push(StepResult::fail("Configuration", format!("{e}"))),
        },
        Err(e) => r.push(StepResult::fail("Configuration", format!("serialize failed: {e}"))),
    }

    r
}

/// Install + start the Windows Service. Requires elevation.
#[cfg(windows)]
pub fn install_service() -> Vec<StepResult> {
    use std::process::Command;
    let mut r = Vec::new();
    let exe = PathBuf::from(TURDMOD_DIR).join("turdmod-service.exe");

    if !exe.exists() {
        r.push(StepResult::fail("Install service", "turdmod-service.exe not found"));
        return r;
    }

    match Command::new(&exe).arg("--install").output() {
        Ok(out) if out.status.success() => {
            r.push(StepResult::ok("Install service", "registered as TurdMODService"))
        }
        Ok(out) => {
            let msg = String::from_utf8_lossy(&out.stderr);
            let hint = if msg.contains("Access") || msg.contains("denied") {
                " — run TurdMOD Setup as Administrator"
            } else {
                ""
            };
            r.push(StepResult::fail("Install service", format!("{}{hint}", msg.trim())));
            return r;
        }
        Err(e) => {
            r.push(StepResult::fail("Install service", format!("{e}")));
            return r;
        }
    }

    match Command::new("net").args(["start", "TurdMODService"]).output() {
        Ok(out) if out.status.success() => {
            r.push(StepResult::ok("Start service", "running"))
        }
        Ok(out) => {
            let so = String::from_utf8_lossy(&out.stdout);
            if so.contains("already been started") {
                r.push(StepResult::ok("Start service", "already running"));
            } else {
                r.push(StepResult::fail("Start service", so.trim().to_string()));
            }
        }
        Err(e) => r.push(StepResult::fail("Start service", format!("{e}"))),
    }

    r
}

#[cfg(not(windows))]
pub fn install_service() -> Vec<StepResult> {
    vec![StepResult::fail("Install service", "Windows only")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_long_and_varies() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 40);
        assert_ne!(a, b, "tokens must not repeat");
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn config_points_at_real_paths() {
        let cfg = build_service_config(r"C:\SCUMServer", "tok", 9090);
        assert_eq!(cfg["port"], 9090);
        assert_eq!(cfg["token"], "tok");
        let exe = cfg["scum_exe"].as_str().unwrap();
        assert!(exe.ends_with("GameServer.exe"), "got {exe}");
        assert!(exe.contains("Binaries"));
        let dlls = cfg["inject_dlls"].as_array().unwrap();
        assert_eq!(dlls.len(), 2, "UE4SS + loader");
        assert!(dlls.iter().any(|d| d.as_str().unwrap().ends_with("UE4SS.dll")));
        assert!(dlls
            .iter()
            .any(|d| d.as_str().unwrap().ends_with("turdmod_server_loader.dll")));
    }
}
