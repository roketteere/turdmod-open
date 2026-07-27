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

use crate::manifest::Manifest;
use serde::Serialize;
use std::path::{Path, PathBuf};

const TURDMOD_DIR: &str = r"C:\TurdMOD";
/// @dep: must match the folder name under UE4SS\Mods and the mods.txt entry.
pub const MOD_NAME: &str = "TurdMODEngineBridge";

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub step: String,
    pub ok: bool,
    pub detail: String,
}

impl StepResult {
    pub fn ok(step: &str, detail: impl Into<String>) -> Self {
        Self { step: step.into(), ok: true, detail: detail.into() }
    }
    pub fn fail(step: &str, detail: impl Into<String>) -> Self {
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
///
/// @dep: apps/turdmod-service/src/config.rs::Config — key names must match
///   exactly. `scum_server_exe` has no serde default, so a wrong key name makes
///   the service fail to parse the config entirely.
/// @inv: inject_dlls order is loader-then-UE4SS, matching Config::load's own
///   fallback. Don't reorder.
pub fn build_service_config(server_root: &str, token: &str, port: u16) -> serde_json::Value {
    let w = win64(server_root);
    serde_json::json!({
        "port": port,
        "token": token,
        "scum_server_exe": w.join("GameServer.exe").display().to_string(),
        "scum_server_args": ["-log", "-port=7042", "-QueryPort=7044"],
        "inject_dlls": [
            w.join("turdmod_server_loader.dll").display().to_string(),
            w.join("UE4SS").join("UE4SS.dll").display().to_string(),
        ],
        "auto_restart": true,
        "restart_delay_secs": 10,
        "scumdb_path": Path::new(server_root)
            .join("SCUM").join("Saved").join("SaveFiles").join("SCUM.db")
            .display().to_string(),
    })
}

/// The service.json of an install that's already here, if there is one.
/// @inv: path must match turdmod-service's config::config_path("default").
pub fn read_existing_config() -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(PathBuf::from(TURDMOD_DIR).join("service.json")).ok()?;
    // Tolerate a UTF-8 BOM — hand-edited configs pick these up on Windows.
    serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()
}

/// Merge a freshly-derived config over an existing one for an UPDATE.
///
/// @inv: the token must survive. Regenerating it silently breaks every Manager
///   dashboard and script already pointed at this server — the single most
///   damaging thing an "update" could do.
/// Only the fields that describe where files now live get overwritten; every
/// other key the operator set (ports, args, scummap_*, phantom_population,
/// instance_id) is carried through untouched, including keys this version of
/// Setup doesn't know about.
pub fn merge_config(existing: &serde_json::Value, fresh: &serde_json::Value) -> serde_json::Value {
    let mut out = existing.clone();
    let (Some(out_map), Some(fresh_map)) = (out.as_object_mut(), fresh.as_object()) else {
        return fresh.clone();
    };

    for key in ["scum_server_exe", "inject_dlls"] {
        if let Some(v) = fresh_map.get(key) {
            out_map.insert(key.to_string(), v.clone());
        }
    }
    // Fill only what's missing — never clobber an operator's choice.
    for key in ["port", "token", "scum_server_args", "auto_restart", "restart_delay_secs", "scumdb_path"] {
        if !out_map.contains_key(key) {
            if let Some(v) = fresh_map.get(key) {
                out_map.insert(key.to_string(), v.clone());
            }
        }
    }
    out
}

/// @inv: every write goes through the manifest first. If the original can't be
///   backed up we REFUSE to write rather than destroy a file we can't restore.
fn copy_into(
    src: &Path,
    dst: &Path,
    results: &mut Vec<StepResult>,
    label: &str,
    mf: &mut Manifest,
) -> bool {
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
    if let Err(e) = mf.before_write(dst) {
        results.push(StepResult::fail(label, format!("{e} — refusing to overwrite it")));
        return false;
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
pub fn place_artifacts(server_root: &str, artifacts: &Path, mf: &mut Manifest) -> Vec<StepResult> {
    let mut r = Vec::new();
    let w = win64(server_root);

    copy_into(
        &artifacts.join("turdmod_server_loader.dll"),
        &w.join("turdmod_server_loader.dll"),
        &mut r,
        "Loader DLL",
        mf,
    );

    let ue4ss_src = artifacts.join("UE4SS").join("UE4SS.dll");
    if ue4ss_src.exists() {
        copy_into(&ue4ss_src, &w.join("UE4SS").join("UE4SS.dll"), &mut r, "UE4SS", mf);
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
        mf,
    );

    r.push(enable_bridge_mod(&w, mf));

    r
}

/// Turn the bridge on in UE4SS.
///
/// @inv: mods.txt is the CONTROLLING list — UE4SS-settings.ini calls it exactly
///   that. A mod folder with only enabled.txt and no mods.txt entry is NOT
///   loaded, which looks like "installed fine, engine dead" with no error
///   anywhere. Write both: enabled.txt for UE4SS builds that honour it, and the
///   mods.txt line for the ones that don't.
/// @brk: if UE4SS ever changes the `Name : 1` line format, this silently
///   stops enabling the bridge.
fn enable_bridge_mod(win64: &Path, mf: &mut Manifest) -> StepResult {
    let mods_dir = win64.join("UE4SS").join("Mods");
    let _ = std::fs::create_dir_all(mods_dir.join(MOD_NAME));

    // Belt: some UE4SS builds look for this marker file.
    let enabled = mods_dir.join(MOD_NAME).join("enabled.txt");
    if mf.before_write(&enabled).is_ok() {
        let _ = std::fs::write(&enabled, "");
    }

    // Braces: the controlling list. Preserve every other entry and comment —
    // operators run other mods (UsmapDumper etc.) and we must not disable them.
    let path = mods_dir.join("mods.txt");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let existing = existing.trim_start_matches('\u{feff}');

    let mut lines: Vec<String> = Vec::new();
    let mut found = false;
    for line in existing.lines() {
        let name = line.split(':').next().unwrap_or("").trim();
        if name.eq_ignore_ascii_case(MOD_NAME) {
            found = true;
            lines.push(format!("{MOD_NAME} : 1"));
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(format!("{MOD_NAME} : 1"));
    }

    if let Err(e) = mf.before_write(&path) {
        return StepResult::fail("Enable bridge", format!("{e} — refusing to edit it"));
    }
    let out = format!("{}\r\n", lines.join("\r\n"));
    match std::fs::write(&path, out) {
        Ok(_) => StepResult::ok(
            "Enable bridge",
            if found {
                format!("{} — already listed, set to enabled", path.display())
            } else {
                format!("{} — added to the mod list", path.display())
            },
        ),
        Err(e) => StepResult::fail("Enable bridge", format!("{}: {e}", path.display())),
    }
}

/// Write C:\TurdMOD\service.json and copy the service exe next to it.
pub fn place_service(
    artifacts: &Path,
    config: &serde_json::Value,
    mf: &mut Manifest,
) -> Vec<StepResult> {
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
        mf,
    );

    let cfg_path = dir.join("service.json");
    if let Err(e) = mf.before_write(&cfg_path) {
        r.push(StepResult::fail("Configuration", format!("{e} — refusing to overwrite it")));
        return r;
    }
    match serde_json::to_string_pretty(config) {
        Ok(s) => match std::fs::write(&cfg_path, s) {
            Ok(_) => r.push(StepResult::ok("Configuration", cfg_path.display().to_string())),
            Err(e) => r.push(StepResult::fail("Configuration", format!("{e}"))),
        },
        Err(e) => r.push(StepResult::fail("Configuration", format!("serialize failed: {e}"))),
    }

    r
}

/// @dep: apps/turdmod-service/src/service.rs::service_name("default")
const SERVICE_NAME: &str = "TurdMODService";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    Missing,
    Stopped,
    Running,
}

#[cfg(windows)]
pub fn service_state() -> ServiceState {
    use std::process::Command;
    match Command::new("sc").args(["query", SERVICE_NAME]).output() {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.contains("RUNNING") || s.contains("START_PENDING") {
                ServiceState::Running
            } else {
                ServiceState::Stopped
            }
        }
        _ => ServiceState::Missing,
    }
}

#[cfg(not(windows))]
pub fn service_state() -> ServiceState {
    ServiceState::Missing
}

/// Stop the service before replacing files.
///
/// @ctx: the service is the game server's PARENT process — stopping it takes
///   SCUM down too. That's unavoidable for an update (the DLLs are loaded into
///   SCUM and locked), but it means an update is a real outage, not a hot swap.
#[cfg(windows)]
pub fn stop_service_for_update() -> Vec<StepResult> {
    use std::process::Command;
    use std::{thread, time::Duration};
    let mut r = Vec::new();

    if service_state() != ServiceState::Running {
        return r;
    }

    match Command::new("net").args(["stop", SERVICE_NAME]).output() {
        Ok(out) if out.status.success() => {
            r.push(StepResult::ok("Stop service", "stopped so files can be replaced"))
        }
        Ok(out) => {
            let msg = String::from_utf8_lossy(&out.stdout);
            let hint = if msg.contains("Access") || msg.contains("denied") {
                " — run TurdMOD Setup as Administrator"
            } else {
                ""
            };
            r.push(StepResult::fail("Stop service", format!("{}{hint}", msg.trim())));
            return r;
        }
        Err(e) => {
            r.push(StepResult::fail("Stop service", format!("{e}")));
            return r;
        }
    }

    // Windows reports the service stopped before the game process has fully
    // exited and released its DLL handles; copying immediately hits error 32.
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(500));
        if service_state() != ServiceState::Running {
            break;
        }
    }
    thread::sleep(Duration::from_secs(2));
    r
}

#[cfg(not(windows))]
pub fn stop_service_for_update() -> Vec<StepResult> {
    Vec::new()
}

/// Register the service if it isn't already, then start it. Requires elevation.
/// Safe to call on an update — an existing registration is left alone.
#[cfg(windows)]
pub fn install_service(mf: &mut Manifest) -> Vec<StepResult> {
    use std::process::Command;
    let mut r = Vec::new();
    let exe = PathBuf::from(TURDMOD_DIR).join("turdmod-service.exe");

    if !exe.exists() {
        r.push(StepResult::fail("Install service", "turdmod-service.exe not found"));
        return r;
    }

    if service_state() == ServiceState::Missing {
        // @inv: only set when WE register it. A service that predates us is not
        // ours to unregister on uninstall.
        mf.service_registered = true;
        match Command::new(&exe).arg("--install").output() {
            Ok(out) if out.status.success() => {
                r.push(StepResult::ok("Install service", "registered as TurdMODService"))
            }
            Ok(out) => {
                let msg = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stderr),
                    String::from_utf8_lossy(&out.stdout)
                );
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
    } else {
        r.push(StepResult::ok("Install service", "already registered — kept it"));
    }

    match Command::new("net").args(["start", SERVICE_NAME]).output() {
        Ok(out) if out.status.success() => r.push(StepResult::ok("Start service", "running")),
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
pub fn install_service(_mf: &mut Manifest) -> Vec<StepResult> {
    vec![StepResult::fail("Install service", "Windows only")]
}

// Backups are handled by manifest::Manifest::before_write — every write is
// snapshotted, not just the two files the old backup_existing() knew about.

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

    // @dep: apps/turdmod-service/src/config.rs — these key names are load-bearing.
    #[test]
    fn config_uses_the_key_names_the_service_actually_parses() {
        let cfg = build_service_config(r"C:\SCUMServer", "tok", 9090);
        // scum_server_exe has no serde default — a wrong name here means the
        // service refuses to parse the config at all.
        assert!(cfg.get("scum_server_exe").is_some(), "must be scum_server_exe, not scum_exe");
        assert!(cfg.get("scum_exe").is_none(), "scum_exe is not a key the service knows");
        assert!(cfg.get("restart_delay_secs").is_some());
        assert!(cfg.get("scumdb_path").is_some());
    }

    #[test]
    fn inject_order_is_loader_then_ue4ss() {
        let cfg = build_service_config(r"C:\SCUMServer", "tok", 9090);
        let dlls: Vec<&str> = cfg["inject_dlls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        assert!(dlls[0].ends_with("turdmod_server_loader.dll"), "loader injects first");
        assert!(dlls[1].ends_with("UE4SS.dll"));
    }

    // @dep: UE4SS-settings.ini calls mods.txt "the controlling mod list" — a
    // mod that isn't listed there does not load, however many marker files it has.
    #[test]
    fn enabling_the_bridge_preserves_other_mods() {
        let root = std::env::temp_dir().join("tm-setup-modstxt-test");
        let _ = std::fs::remove_dir_all(&root);
        let mods = root.join("UE4SS").join("Mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(
            mods.join("mods.txt"),
            "; my mod list\r\nUsmapDumper : 1\r\nSomethingOff : 0\r\n",
        )
        .unwrap();

        let res = enable_bridge_mod(&root, &mut Manifest::new_in("test", root.clone()));
        assert!(res.ok, "{}", res.detail);

        let txt = std::fs::read_to_string(mods.join("mods.txt")).unwrap();
        assert!(txt.contains("TurdMODEngineBridge : 1"), "bridge must be listed: {txt}");
        assert!(txt.contains("UsmapDumper : 1"), "other mods must survive: {txt}");
        assert!(txt.contains("SomethingOff : 0"), "must not enable what the operator disabled");
        assert!(txt.contains("; my mod list"), "comments must survive");
        assert!(mods.join("TurdMODEngineBridge").join("enabled.txt").exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn enabling_the_bridge_flips_an_existing_disabled_entry() {
        let root = std::env::temp_dir().join("tm-setup-modstxt-test2");
        let _ = std::fs::remove_dir_all(&root);
        let mods = root.join("UE4SS").join("Mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("mods.txt"), "TurdMODEngineBridge : 0\r\n").unwrap();

        assert!(enable_bridge_mod(&root, &mut Manifest::new_in("test", root.clone())).ok);
        let txt = std::fs::read_to_string(mods.join("mods.txt")).unwrap();
        assert!(txt.contains("TurdMODEngineBridge : 1"));
        assert!(!txt.contains(": 0"), "the disabled entry must be replaced, not duplicated: {txt}");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn enabling_the_bridge_creates_mods_txt_from_nothing() {
        let root = std::env::temp_dir().join("tm-setup-modstxt-test3");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("UE4SS").join("Mods")).unwrap();

        assert!(enable_bridge_mod(&root, &mut Manifest::new_in("test", root.clone())).ok);
        let txt = std::fs::read_to_string(root.join("UE4SS").join("Mods").join("mods.txt")).unwrap();
        assert_eq!(txt, "TurdMODEngineBridge : 1\r\n");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn update_never_regenerates_the_token() {
        let existing = serde_json::json!({
            "port": 9099,
            "token": "operators-real-token",
            "scum_server_exe": r"D:\Old\GameServer.exe",
            "inject_dlls": [r"D:\Old\loader.dll"],
            "scum_server_args": ["-log", "-port=8000"],
            "phantom_population": 50,
            "scummap_api_url": "https://scummymap.com/api",
        });
        let fresh = build_service_config(r"C:\New", "brand-new-token", 9090);
        let merged = merge_config(&existing, &fresh);

        assert_eq!(merged["token"], "operators-real-token", "token must survive an update");
        // Operator settings — including keys Setup doesn't know about — carry through.
        assert_eq!(merged["port"], 9099);
        assert_eq!(merged["phantom_population"], 50);
        assert_eq!(merged["scummap_api_url"], "https://scummymap.com/api");
        assert_eq!(merged["scum_server_args"][1], "-port=8000");
        // Paths DO update — they describe where the files we just copied live.
        assert!(merged["scum_server_exe"].as_str().unwrap().starts_with(r"C:\New"));
        assert!(merged["inject_dlls"][0].as_str().unwrap().starts_with(r"C:\New"));
    }

    #[test]
    fn merge_fills_missing_keys_from_a_partial_config() {
        let sparse = serde_json::json!({ "token": "t", "scum_server_exe": "x" });
        let merged = merge_config(&sparse, &build_service_config(r"C:\S", "new", 9090));
        assert_eq!(merged["token"], "t");
        assert_eq!(merged["port"], 9090, "missing keys get filled");
        assert!(merged["scumdb_path"].is_string());
    }

    #[test]
    fn config_points_at_real_paths() {
        let cfg = build_service_config(r"C:\SCUMServer", "tok", 9090);
        assert_eq!(cfg["port"], 9090);
        assert_eq!(cfg["token"], "tok");
        let exe = cfg["scum_server_exe"].as_str().unwrap();
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
