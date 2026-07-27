// TurdMOD Setup — Tauri backend.
//
// Commands are grouped by wizard step. The AI assistant calls the SAME commands
// the UI buttons do — there's no separate "agent path" that could drift from
// what a manual install does.

mod capability;
mod client;
mod detect;
mod download;
mod handoff;
mod install_local;
mod manifest;
mod uninstall;
mod update;
mod verify;

use capability::{CapabilityReport, HostKind};
use detect::DetectedInstalls;
use install_local::StepResult;
use manifest::Manifest;
use serde::Serialize;
use std::path::PathBuf;
use uninstall::UninstallPlan;
use verify::VerifyReport;

// ─── Detect ────────────────────────────────────────────────────────────────

#[tauri::command]
fn detect_installs() -> DetectedInstalls {
    detect::detect_all()
}

#[tauri::command]
fn validate_path(path: String) -> bool {
    detect::validate_install_path(&path)
}

#[tauri::command]
fn find_server_exe(root: String) -> Option<String> {
    detect::server_exe_in(&root)
}

// ─── Capability ────────────────────────────────────────────────────────────

#[tauri::command]
fn capability_report(host_kind: HostKind, can_execute: bool) -> CapabilityReport {
    capability::report_for(host_kind, can_execute)
}

// ─── Configure ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PreparedConfig {
    token: String,
    port: u16,
    config: serde_json::Value,
    /// Where the Server Pack artifacts were found, if any.
    artifacts_dir: Option<String>,
    /// True when TurdMOD is already here — this run updates, not installs fresh.
    is_update: bool,
    /// True when the existing access key was reused rather than regenerated.
    token_preserved: bool,
    service_state: install_local::ServiceState,
    /// True when the operator currently has BattlEye ON and installing will
    /// turn it off. The UI MUST say so before they commit.
    battleye_will_be_disabled: bool,
}

/// Build the config for this install.
///
/// @inv: on an update the existing token and operator settings must survive. A
///   regenerated token silently breaks every Manager dashboard and script
///   already pointed at this server, with no clue as to why.
#[tauri::command]
fn prepare_config(server_root: String, port: Option<u16>) -> PreparedConfig {
    let existing = install_local::read_existing_config();
    let fresh_token = install_local::generate_token();

    // An explicit port from the UI wins; otherwise keep whatever is already set.
    let existing_port = existing
        .as_ref()
        .and_then(|c| c.get("port"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u16);
    let port = port.or(existing_port).unwrap_or(9090);

    let fresh = install_local::build_service_config(&server_root, &fresh_token, port);

    let (config, token, token_preserved) = match &existing {
        Some(prev) => {
            let mut merged = install_local::merge_config(prev, &fresh);
            if let Some(m) = merged.as_object_mut() {
                m.insert("port".into(), serde_json::json!(port));
            }
            let tok = merged
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or(&fresh_token)
                .to_string();
            let preserved = prev.get("token").and_then(|v| v.as_str()) == Some(tok.as_str());
            (merged, tok, preserved)
        }
        None => (fresh, fresh_token, false),
    };

    // @inv: TurdMOD needs BattlEye off. We change it, say so up front, and put
    //   it back on uninstall — never a silent anticheat change.
    let mut config = config;
    let battleye_will_be_disabled = install_local::ensure_no_battleye(&mut config);

    let svc = install_local::service_state();
    PreparedConfig {
        config,
        token,
        port,
        artifacts_dir: install_local::find_artifacts_dir().map(|p| p.display().to_string()),
        is_update: existing.is_some() || svc != install_local::ServiceState::Missing,
        token_preserved,
        service_state: svc,
        battleye_will_be_disabled,
    }
}

// ─── Install ───────────────────────────────────────────────────────────────

#[tauri::command]
fn install_local_full(
    server_root: String,
    config: serde_json::Value,
    artifacts_dir: Option<String>,
) -> Vec<StepResult> {
    let artifacts = match artifacts_dir
        .map(PathBuf::from)
        .or_else(install_local::find_artifacts_dir)
    {
        Some(d) => d,
        None => {
            return vec![StepResult {
                step: "Locate artifacts".into(),
                ok: false,
                detail: "Couldn't find turdmod-service.exe. Extract the Server Pack and put TurdMOD Setup next to it, or into C:\\TurdMOD.".into(),
            }]
        }
    };

    // Stop the service first: the engine DLLs are loaded into the running game
    // and can't be replaced while it holds them.
    let mut results = install_local::stop_service_for_update();
    if results.iter().any(|r| !r.ok) {
        results.push(StepResult {
            step: "Install".into(),
            ok: false,
            detail: "Stopped before touching any files — fix the error above and run it again."
                .into(),
        });
        return results;
    }

    // @inv: every write below is recorded here first, so uninstall can reverse
    // it. The manifest is saved even on failure — a half-done install still
    // needs to be undoable.
    let mut mf = Manifest::new(&server_root);
    // Only claim we changed BattlEye if the operator actually had it on.
    mf.added_no_battleye = install_local::read_existing_config()
        .map(|prev| !install_local::has_no_battleye(&prev))
        .unwrap_or(false)
        && install_local::has_no_battleye(&config);

    results.extend(install_local::place_artifacts(&server_root, &artifacts, &mut mf));
    results.extend(install_local::place_service(&artifacts, &config, &mut mf));

    // Only attempt the service if every file landed — a half-copied install
    // that starts is worse than one that stops here with a clear error.
    if results.iter().all(|r| r.ok) {
        results.extend(install_local::install_service(&mut mf));
    } else {
        results.push(StepResult {
            step: "Install service".into(),
            ok: false,
            detail: "Skipped — fix the errors above first.".into(),
        });
    }

    if let Err(e) = mf.save() {
        results.push(StepResult {
            step: "Install record".into(),
            ok: false,
            detail: format!("{e} — uninstall won't be able to reverse this automatically"),
        });
    }

    // Remember which pack this was, so the update check has something to
    // compare against on later runs.
    update::record_installed_version(&artifacts);

    // Point Manager at what we just installed. Never fails the install.
    results.push(handoff::configure_manager(&server_root));

    results
}

// ─── Modded client ─────────────────────────────────────────────────────────

/// What building a modded copy would cost, and which drives can take it.
#[tauri::command]
fn client_plan(source: String) -> client::ClientPlan {
    client::plan(&source)
}

/// Build the isolated copy. @inv: never touches the Steam install.
#[tauri::command]
fn client_create_copy(source: String, dest: String) -> Vec<StepResult> {
    let mut mf = Manifest::load().unwrap_or_else(|| Manifest::new(&source));
    let mut r = client::create_modded_copy(&source, &dest, &mut mf);
    if r.iter().all(|s| s.ok) {
        r.push(client::write_client_config(&dest));
    }
    if let Err(e) = mf.save() {
        r.push(StepResult::fail(
            "Install record",
            format!("{e} — uninstall won't know about this copy"),
        ));
    }
    r
}

// ─── Download the Server Pack ──────────────────────────────────────────────

/// Fetch + extract the pack from turdmod.com so a bare Setup.exe can install.
#[tauri::command]
async fn download_pack() -> download::DownloadResult {
    download::fetch_pack().await
}

// ─── Update check ──────────────────────────────────────────────────────────

#[tauri::command]
async fn check_for_update() -> update::UpdateReport {
    update::check().await
}

// ─── Uninstall ─────────────────────────────────────────────────────────────

#[tauri::command]
fn uninstall_plan() -> UninstallPlan {
    uninstall::plan()
}

#[tauri::command]
fn uninstall_run(remove_settings: Option<bool>) -> Vec<StepResult> {
    uninstall::run(remove_settings.unwrap_or(false))
}

// ─── Verify ────────────────────────────────────────────────────────────────

#[tauri::command]
async fn verify_install(port: u16, token: String, server_root: Option<String>) -> VerifyReport {
    verify::run(port, &token, server_root.as_deref()).await
}

// ─── AI assistant tools ────────────────────────────────────────────────────
// Read-only helpers the assistant uses to diagnose. Anything destructive goes
// through the same commands above, gated by a confirm card in the UI.

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map(|s| s.chars().take(20_000).collect())
        .map_err(|e| format!("{path}: {e}"))
}

/// Tail the last N lines of a log — how the assistant diagnoses failures.
#[tauri::command]
fn tail_log(path: String, lines: Option<usize>) -> Result<String, String> {
    let n = lines.unwrap_or(80).min(500);
    let content = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(n);
    Ok(all[start..].join("\n"))
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, contents).map_err(|e| format!("{path}: {e}"))
}

// ─── Entry ─────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            detect_installs,
            validate_path,
            find_server_exe,
            capability_report,
            prepare_config,
            install_local_full,
            client_plan,
            client_create_copy,
            check_for_update,
            download_pack,
            uninstall_plan,
            uninstall_run,
            verify_install,
            read_text_file,
            tail_log,
            path_exists,
            write_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TurdMOD Setup");
}

// ─── Live smoke test ───────────────────────────────────────────────────────
// Runs the whole non-destructive path against THIS machine's real SCUM install:
// detect -> validate -> capability -> config. Writes nothing, installs nothing.
// #[ignore] because it needs a real SCUM server present.
//   cargo test --lib live_ -- --ignored --nocapture

#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "requires a real SCUM dedicated server install on this machine"]
    fn live_detect_through_config() {
        let found = detect::detect_all();
        println!("game:   {:?}", found.game);
        println!("server: {:?}", found.server);
        println!("looked in: {:?}", found.searched);

        let root = found.server.expect("no SCUM dedicated server found on this machine");
        assert!(detect::validate_install_path(&root), "detected root failed validation: {root}");

        let exe = detect::server_exe_in(&root).expect("GameServer.exe missing under detected root");
        println!("exe:    {exe}");

        let rep = capability::report_for(capability::HostKind::Local, true);
        assert!(rep.engine_supported, "local install must support the engine");
        println!("verdict: {}", rep.verdict);

        let token = install_local::generate_token();
        let cfg = install_local::build_service_config(&root, &token, 9090);
        assert_eq!(cfg["scum_server_exe"].as_str().unwrap(), exe, "config must point at the real exe");
        for d in cfg["inject_dlls"].as_array().unwrap() {
            let p = d.as_str().unwrap();
            assert!(p.starts_with(&root), "inject path escaped the server root: {p}");
        }
        println!("config: {}", serde_json::to_string_pretty(&cfg).unwrap());
        println!("artifacts dir: {:?}", install_local::find_artifacts_dir());
    }

    /// Full artifact placement against the REAL packaged Server Pack, into a
    /// throwaway server root. Proves the copy layout end-to-end without
    /// touching a live install or registering a Windows service.
    ///
    /// Extract releases\TurdMOD-Server-Pack-*.zip somewhere first, then:
    ///   set TURDMOD_PACK=C:\path\to\extracted
    ///   cargo test --lib live_install -- --ignored --nocapture
    #[test]
    #[ignore = "needs TURDMOD_PACK pointing at an extracted Server Pack"]
    fn live_install_into_scratch_root() {
        let pack = std::env::var("TURDMOD_PACK")
            .expect("set TURDMOD_PACK to an extracted Server Pack directory");
        let pack = PathBuf::from(pack);
        assert!(
            pack.join("turdmod-service.exe").exists(),
            "TURDMOD_PACK has no turdmod-service.exe: {}",
            pack.display()
        );

        let root = std::env::temp_dir().join("turdmod-setup-scratch-root");
        let _ = std::fs::remove_dir_all(&root);
        let w = root.join("SCUM").join("Binaries").join("Win64");
        std::fs::create_dir_all(&w).unwrap();
        // Make it look like a real install so validate_install_path agrees.
        std::fs::write(w.join("GameServer.exe"), b"stub").unwrap();
        assert!(detect::validate_install_path(&root.display().to_string()));

        let mut mf = Manifest::new(&root.display().to_string());
        let results = install_local::place_artifacts(&root.display().to_string(), &pack, &mut mf);
        for r in &results {
            println!("{:<16} {:<5} {}", r.step, r.ok, r.detail);
        }
        assert!(results.iter().all(|r| r.ok), "every placement step must succeed");

        // The exact layout the loader and UE4SS look for.
        for rel in [
            "turdmod_server_loader.dll",
            "UE4SS/UE4SS.dll",
            "UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll",
            "UE4SS/Mods/TurdMODEngineBridge/enabled.txt",
        ] {
            let p = w.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            assert!(p.exists(), "missing after install: {}", p.display());
            println!("placed  {}", p.display());
        }

        // The config we'd write must point at the files we just placed.
        let cfg = install_local::build_service_config(&root.display().to_string(), "tok", 9090);
        for d in cfg["inject_dlls"].as_array().unwrap() {
            let p = PathBuf::from(d.as_str().unwrap());
            assert!(p.exists(), "config references a file that wasn't placed: {}", p.display());
        }

        std::fs::remove_dir_all(&root).unwrap();
        println!("cleaned up {}", root.display());
    }

    /// Read-only check that an update against THIS machine's existing
    /// C:\TurdMOD\service.json keeps the operator's token and settings.
    #[test]
    #[ignore = r"requires an existing C:\TurdMOD\service.json"]
    fn live_update_preserves_existing_config() {
        let existing = install_local::read_existing_config()
            .expect(r"no C:\TurdMOD\service.json on this machine");
        let old_token = existing["token"].as_str().expect("existing config has no token");

        let fresh = install_local::build_service_config(r"C:\Somewhere\Else", "REGENERATED", 9090);
        let merged = install_local::merge_config(&existing, &fresh);

        assert_eq!(merged["token"].as_str().unwrap(), old_token, "update must keep the token");
        // Every key the operator had is still there.
        for (k, v) in existing.as_object().unwrap() {
            if k == "scum_server_exe" || k == "inject_dlls" {
                continue; // intentionally repointed at the new file locations
            }
            assert_eq!(&merged[k], v, "update dropped or changed operator key: {k}");
        }
        println!("preserved {} keys, token intact", existing.as_object().unwrap().len());
    }

    /// THE REAL THING: full install onto this machine's actual SCUM server.
    /// Registers and starts the Windows Service. Requires an elevated shell.
    ///   set TURDMOD_PACK=<extracted Server Pack>
    ///   cargo test --lib live_real_install -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "installs and starts a Windows service on this machine"]
    async fn live_real_install() {
        let pack = std::env::var("TURDMOD_PACK").expect("set TURDMOD_PACK");
        let found = detect::detect_all();
        let root = found.server.expect("no SCUM dedicated server found");
        println!("server root: {root}");

        let prep = prepare_config(root.clone(), None);
        println!("is_update={} token_preserved={} service={:?}",
                 prep.is_update, prep.token_preserved, prep.service_state);
        println!("config:
{}", serde_json::to_string_pretty(&prep.config).unwrap());

        let root2 = root.clone();
        let results = install_local_full(root, prep.config.clone(), Some(pack));
        println!("
--- install steps ---");
        for r in &results {
            println!("{:<20} {:<5} {}", r.step, r.ok, r.detail);
        }
        assert!(results.iter().all(|r| r.ok), "install had failing steps");

        println!("
--- verify ---");
        let rep = verify_install(prep.port, prep.token.clone(), Some(root2)).await;
        for c in &rep.checks {
            println!("{:<26} {:<5} {}", c.label, c.ok, c.detail);
            if !c.ok { println!("{:<26}   fix: {}", "", c.fix); }
        }
        println!("
summary: {}", rep.summary);
    }

    /// Re-run verification only, against whatever state the machine is in now.
    ///   cargo test --lib live_verify_only -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "reads live service state on this machine"]
    async fn live_verify_only() {
        let root = detect::detect_all().server.expect("no server");
        let cfg = install_local::read_existing_config().expect("no service.json");
        let token = cfg["token"].as_str().unwrap().to_string();
        let port = cfg["port"].as_u64().unwrap_or(9090) as u16;

        let rep = verify_install(port, token, Some(root)).await;
        for c in &rep.checks {
            println!("{:<28} {:<5} {}", c.label, c.ok, c.detail);
            if !c.ok { println!("{:<28}   FIX: {}", "", c.fix); }
        }
        println!("
summary: {}", rep.summary);
    }

    /// Show what an uninstall would do on this machine, without doing it.
    ///   cargo test --lib live_uninstall_plan -- --ignored --nocapture
    #[test]
    #[ignore = "reads live install state on this machine"]
    fn live_uninstall_plan() {
        let p = uninstall::plan();
        println!("has_manifest: {}", p.has_manifest);
        println!("service: {:?}", p.service_state);
        println!("restore: {}  remove: {}", p.files_to_restore, p.files_to_remove);
        if !p.warning.is_empty() { println!("WARNING: {}", p.warning); }
        for (i, s) in p.steps.iter().enumerate() { println!("  {}. {}", i + 1, s); }
    }

    /// Actually reverse the install on this machine.
    ///   cargo test --lib live_uninstall_run -- --ignored --nocapture
    #[test]
    #[ignore = "removes TurdMOD from this machine"]
    fn live_uninstall_run() {
        for r in uninstall::run(false) {
            println!("{:<22} {:<5} {}", r.step, r.ok, r.detail);
        }
    }

    /// Real sizing + drive scan for the modded client copy on this machine.
    ///   cargo test --lib live_client_plan -- --ignored --nocapture
    #[test]
    #[ignore = "reads the real SCUM client install and all drives"]
    fn live_client_plan() {
        let game = detect::detect_all().game.expect("no SCUM client found");
        let p = client::plan(&game);
        let gb = |b: u64| b as f64 / 1024.0 / 1024.0 / 1024.0;
        println!("source     : {}", p.source);
        println!("files      : {}", p.file_count);
        println!("total      : {:.1} GB", gb(p.total_bytes));
        println!("linkable   : {:.1} GB  ({:.1}%)", gb(p.linkable_bytes),
                 p.linkable_bytes as f64 / p.total_bytes as f64 * 100.0);
        println!("real copy  : {:.1} GB", gb(p.copy_bytes));
        println!("--- drives ---");
        for d in &p.drives {
            println!("  {}  free {:.0} GB  fits={} hardlink={}  {}",
                     d.name, gb(d.free_bytes), d.fits, d.can_hardlink, d.note);
        }
        assert!(p.total_bytes > 0, "must measure something");
        assert!(!p.drives.is_empty(), "must find at least one drive");
    }

    /// Build a REAL modded copy on this machine and prove the safety
    /// properties, then clean it up.
    ///   set TURDMOD_CLIENT_DEST=C:\SCUM-Modded-Test
    ///   cargo test --lib live_client_copy -- --ignored --nocapture
    #[test]
    #[ignore = "creates a real modded game copy on this machine"]
    fn live_client_copy() {
        let dest = std::env::var("TURDMOD_CLIENT_DEST")
            .expect("set TURDMOD_CLIENT_DEST to a folder that does not exist yet");
        let game = detect::detect_all().game.expect("no SCUM client found");
        let dest_p = PathBuf::from(&dest);
        assert!(!dest_p.exists(), "{dest} already exists — pick a fresh path");

        // Fingerprint a source pak so we can prove the copy never mutates it.
        let paks = PathBuf::from(&game).join("SCUM").join("Content").join("Paks");
        let sample = std::fs::read_dir(&paks).unwrap().flatten()
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("pak"))
            .expect("no pak in the source install");
        let before_len = std::fs::metadata(&sample).unwrap().len();
        println!("source pak: {} ({} bytes)", sample.display(), before_len);

        let mut mf = Manifest::new_in("client-test", std::env::temp_dir().join("tm-client-live"));
        let start = std::time::Instant::now();
        let results = client::create_modded_copy(&game, &dest, &mut mf);
        let elapsed = start.elapsed();
        for r in &results { println!("{:<14} {:<5} {}", r.step, r.ok, r.detail); }
        assert!(results.iter().all(|r| r.ok), "copy must succeed");
        println!("elapsed: {:.1}s", elapsed.as_secs_f64());

        // The copy must be a usable game install.
        let exe = dest_p.join("SCUM").join("Binaries").join("Win64").join("SCUM.exe");
        assert!(exe.is_file(), "copy has no SCUM.exe");

        // The source pak must be untouched.
        assert_eq!(std::fs::metadata(&sample).unwrap().len(), before_len,
                   "source pak changed size — the copy corrupted the vanilla install");

        let cfg = client::write_client_config(&dest);
        println!("{:<14} {:<5} {}", cfg.step, cfg.ok, cfg.detail);

        // Tear down and prove deleting the copy leaves the source intact —
        // hardlinks are equal references, so this is the property that matters.
        std::fs::remove_dir_all(&dest_p).unwrap();
        assert!(!dest_p.exists());
        assert!(sample.is_file(), "deleting the copy removed the source pak!");
        assert_eq!(std::fs::metadata(&sample).unwrap().len(), before_len,
                   "source pak damaged by deleting the copy");
        println!("cleaned up; source pak intact at {} bytes", before_len);
    }

    /// Exercise the update check against the LIVE turdmod.com endpoint in all
    /// three states, restoring whatever was there before.
    ///   cargo test --lib live_update_check -- --ignored --nocapture
    #[tokio::test]
    #[ignore = r"hits turdmod.com and touches C:\TurdMOD\VERSION.json"]
    async fn live_update_check() {
        let vpath = PathBuf::from(r"C:\TurdMOD").join("VERSION.json");
        let saved = std::fs::read(&vpath).ok();
        let _ = std::fs::create_dir_all(r"C:\TurdMOD");

        // 1. No local version at all -> must NOT claim we're current.
        let _ = std::fs::remove_file(&vpath);
        let r = update::check().await;
        println!("[no local version] {:?} :: {}", r.state, r.summary);
        assert_ne!(r.state, update::UpdateState::Current, "unknown local must never read as current");
        let live = r.latest.clone().expect("turdmod.com should be serving latest.json");

        // 2. Local matches what's published -> current.
        std::fs::write(&vpath, serde_json::to_string(&live).unwrap()).unwrap();
        let r = update::check().await;
        println!("[matching]         {:?} :: {}", r.state, r.summary);
        assert_eq!(r.state, update::UpdateState::Current);

        // 3. Local is an older build -> update offered.
        let old = update::VersionInfo { build: "19700101-0000".into(), ..live.clone() };
        std::fs::write(&vpath, serde_json::to_string(&old).unwrap()).unwrap();
        let r = update::check().await;
        println!("[stale]            {:?} :: {}", r.state, r.summary);
        assert_eq!(r.state, update::UpdateState::Available);
        assert!(r.summary.contains(&live.build), "must name the new build");

        match saved {
            Some(bytes) => std::fs::write(&vpath, bytes).unwrap(),
            None => { let _ = std::fs::remove_file(&vpath); }
        }
        println!("restored prior state");
    }

    /// Download + extract the REAL Server Pack from turdmod.com, then clean up.
    ///   cargo test --lib live_download_pack -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "downloads ~16 MB from turdmod.com"]
    async fn live_download_pack() {
        let r = download::fetch_pack().await;
        for s in &r.steps { println!("{:<10} {:<5} {}", s.step, s.ok, s.detail); }
        let dir = r.artifacts_dir.expect("download should yield an artifacts dir");
        let p = PathBuf::from(&dir);

        // Everything install_local_full needs must be present.
        for rel in [
            "turdmod-service.exe",
            "TurdMOD-Setup.exe",
            "turdmod_server_loader.dll",
            "VERSION.json",
            "UE4SS/UE4SS.dll",
            "UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll",
        ] {
            let f = p.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            assert!(f.is_file(), "pack is missing {rel}");
            println!("  ok {rel}");
        }

        // find_artifacts_dir keys on turdmod-service.exe adjacency.
        println!("VERSION.json: {}", std::fs::read_to_string(p.join("VERSION.json")).unwrap().trim());

        std::fs::remove_dir_all(&p).unwrap();
        println!("cleaned up {dir}");
    }

    /// FIRST-TIME install on a folder with no prior TurdMOD/UE4SS files, using
    /// a pack downloaded from turdmod.com. Exercises the Created branches that
    /// an update never touches.
    ///   cargo test --lib live_clean_install -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "installs onto this machine from a downloaded pack"]
    async fn live_clean_install() {
        let root = detect::detect_all().server.expect("no SCUM server found");

        let dl = download::fetch_pack().await;
        for s in &dl.steps { println!("{:<10} {:<5} {}", s.step, s.ok, s.detail); }
        let pack = dl.artifacts_dir.expect("download failed");

        let prep = prepare_config(root.clone(), None);
        println!("\nis_update={} token_preserved={} service={:?}",
                 prep.is_update, prep.token_preserved, prep.service_state);
        assert!(!prep.is_update, "a clean folder must NOT look like an update");
        assert!(!prep.token_preserved, "there is no prior token to preserve");
        assert_ne!(prep.token, "local-dev-token", "must generate a fresh token");
        assert_eq!(prep.token.len(), 40);

        let results = install_local_full(root.clone(), prep.config.clone(), Some(pack.clone()));
        println!("\n--- install ---");
        for r in &results { println!("{:<20} {:<5} {}", r.step, r.ok, r.detail); }
        assert!(results.iter().all(|r| r.ok), "clean install must succeed");

        // Everything should be recorded as Created, not Replaced — nothing was here.
        let mf = Manifest::load().expect("manifest must exist");
        let replaced: Vec<_> = mf.replaced().map(|e| e.path.clone()).collect();
        println!("\ncreated={} replaced={}", mf.created().count(), replaced.len());
        assert!(replaced.is_empty(), "nothing pre-existed, so nothing should be Replaced: {replaced:?}");
        assert!(mf.service_registered, "we registered the service, so we own removing it");

        // mods.txt built from nothing must contain exactly our entry.
        let mods = PathBuf::from(&root)
            .join("SCUM").join("Binaries").join("Win64")
            .join("UE4SS").join("Mods").join("mods.txt");
        let txt = std::fs::read_to_string(&mods).unwrap();
        println!("mods.txt: {:?}", txt);
        assert!(txt.contains("TurdMODEngineBridge : 1"));

        let rep = verify_install(prep.port, prep.token.clone(), Some(root)).await;
        println!("\n--- verify ---");
        for c in &rep.checks { println!("{:<28} {:<5} {}", c.label, c.ok, c.detail); }

        let _ = std::fs::remove_dir_all(&pack);
    }

    /// BattlEye round trip against a real service.json: install must turn it
    /// OFF and announce it; uninstall must put it back ON.
    ///   cargo test --lib live_battleye_round_trip -- --ignored --nocapture
    #[test]
    #[ignore = r"rewrites C:\TurdMOD\service.json"]
    fn live_battleye_round_trip() {
        let vpath = PathBuf::from(r"C:\TurdMOD").join("service.json");
        let saved = std::fs::read(&vpath).ok();
        let root = detect::detect_all().server.expect("no server");

        // Operator config with BattlEye ON and a flag of their own.
        let theirs = serde_json::json!({
            "port": 9090,
            "token": "their-token",
            "scum_server_exe": "x",
            "scum_server_args": ["-log", "-port=7042", "-TheirOwnFlag"],
        });
        let _ = std::fs::create_dir_all(r"C:\TurdMOD");
        std::fs::write(&vpath, serde_json::to_string_pretty(&theirs).unwrap()).unwrap();

        let prep = prepare_config(root, None);
        println!("battleye_will_be_disabled = {}", prep.battleye_will_be_disabled);
        assert!(prep.battleye_will_be_disabled, "must announce the change");
        assert!(install_local::has_no_battleye(&prep.config), "config must have BE off");
        assert_eq!(prep.token, "their-token", "token still preserved");
        println!("args after install: {}", prep.config["scum_server_args"]);

        // Simulate the installed state, then reverse it.
        std::fs::write(&vpath, serde_json::to_string_pretty(&prep.config).unwrap()).unwrap();
        let mut live: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&vpath).unwrap()).unwrap();
        assert!(install_local::remove_no_battleye(&mut live), "uninstall removes our flag");
        println!("args after uninstall: {}", live["scum_server_args"]);

        let after: Vec<String> = live["scum_server_args"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert_eq!(after, vec!["-log", "-port=7042", "-TheirOwnFlag"],
                   "must land exactly back on their original args");

        // Already-off config must NOT be announced as a change.
        let off = serde_json::json!({
            "port": 9090, "token": "t", "scum_server_exe": "x",
            "scum_server_args": ["-log", install_local::NO_BATTLEYE],
        });
        std::fs::write(&vpath, serde_json::to_string_pretty(&off).unwrap()).unwrap();
        let prep2 = prepare_config(detect::detect_all().server.unwrap(), None);
        assert!(!prep2.battleye_will_be_disabled, "already off is their choice — stay quiet");
        println!("already-off announced? {}", prep2.battleye_will_be_disabled);

        match saved {
            Some(b) => std::fs::write(&vpath, b).unwrap(),
            None => { let _ = std::fs::remove_file(&vpath); }
        }
        println!("restored original service.json");
    }
}
