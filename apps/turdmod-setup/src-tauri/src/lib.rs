// TurdMOD Setup — Tauri backend.
//
// Commands are grouped by wizard step. The AI assistant calls the SAME commands
// the UI buttons do — there's no separate "agent path" that could drift from
// what a manual install does.

mod capability;
mod detect;
mod install_local;
mod verify;

use capability::{CapabilityReport, HostKind};
use detect::DetectedInstalls;
use install_local::StepResult;
use serde::Serialize;
use std::path::PathBuf;
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

    let svc = install_local::service_state();
    PreparedConfig {
        config,
        token,
        port,
        artifacts_dir: install_local::find_artifacts_dir().map(|p| p.display().to_string()),
        is_update: existing.is_some() || svc != install_local::ServiceState::Missing,
        token_preserved,
        service_state: svc,
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

    // Update path: back up what's there, then stop the service so the DLLs it
    // loaded into the game aren't locked when we replace them.
    let mut results = install_local::backup_existing();
    results.extend(install_local::stop_service_for_update());
    if results.iter().any(|r| !r.ok) {
        results.push(StepResult {
            step: "Install".into(),
            ok: false,
            detail: "Stopped before touching any files — fix the error above and run it again."
                .into(),
        });
        return results;
    }

    results.extend(install_local::place_artifacts(&server_root, &artifacts));
    results.extend(install_local::place_service(&artifacts, &config));

    // Only attempt the service if every file landed — a half-copied install
    // that starts is worse than one that stops here with a clear error.
    if results.iter().all(|r| r.ok) {
        results.extend(install_local::install_service());
    } else {
        results.push(StepResult {
            step: "Install service".into(),
            ok: false,
            detail: "Skipped — fix the errors above first.".into(),
        });
    }
    results
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

        let results = install_local::place_artifacts(&root.display().to_string(), &pack);
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
}
