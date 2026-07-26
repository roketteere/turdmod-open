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
}

#[tauri::command]
fn prepare_config(server_root: String, port: Option<u16>) -> PreparedConfig {
    let token = install_local::generate_token();
    let port = port.unwrap_or(9090);
    PreparedConfig {
        config: install_local::build_service_config(&server_root, &token, port),
        token,
        port,
        artifacts_dir: install_local::find_artifacts_dir().map(|p| p.display().to_string()),
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

    let mut results = install_local::place_artifacts(&server_root, &artifacts);
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
async fn verify_install(port: u16, token: String) -> VerifyReport {
    verify::run(port, &token).await
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
        assert_eq!(cfg["scum_exe"].as_str().unwrap(), exe, "config must point at the real exe");
        for d in cfg["inject_dlls"].as_array().unwrap() {
            let p = d.as_str().unwrap();
            assert!(p.starts_with(&root), "inject path escaped the server root: {p}");
        }
        println!("config: {}", serde_json::to_string_pretty(&cfg).unwrap());
        println!("artifacts dir: {:?}", install_local::find_artifacts_dir());
    }
}
