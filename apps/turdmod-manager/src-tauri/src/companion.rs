// Companion process lifecycle manager.
//
// Spawns `pnpm --filter @turdmod/turdmod-companion dev` from the repo root,
// streams stdout/stderr line-by-line to the frontend as Tauri events, and
// tracks the process handle so the Manager can start/stop/restart it.
//
// TODO(Phase 4): replace with a bundled companion .exe so users don't need
// pnpm/Node.js installed. At that point, swap `spawn_command()` for a
// `Command::new(app_handle.path().resource_dir()?.join("companion.exe"))`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::settings::ManagerSettings;

pub const STDOUT_EVENT: &str = "companion://stdout";

// ---------------------------------------------------------------------------
// Public status type (serialized to the frontend)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CompanionStatus {
    Stopped,
    Starting,
    Running {
        pid: u32,
        #[serde(rename = "startedAtIso")]
        started_at_iso: String,
    },
    Crashed {
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
    },
}

// ---------------------------------------------------------------------------
// Event payload emitted on every stdout/stderr line
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StdoutLine {
    pub ts: String,
    pub level: &'static str,
    pub raw: String,
}

fn parse_level(line: &str) -> &'static str {
    let first = line.trim_start().split_whitespace().next().unwrap_or("");
    let upper = first.to_ascii_uppercase();
    if upper.contains("ERROR") || upper.contains("ERR") {
        "error"
    } else if upper.contains("WARN") {
        "warn"
    } else {
        "info"
    }
}

fn iso_now() -> String {
    // Millisecond-precision ISO-8601 without pulling in chrono.
    // Format: 2026-05-15T16:09:00.000Z (UTC).
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();

    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    // Gregorian calendar — good enough for log timestamps.
    let mut year = 1970u64;
    let mut remaining_days = days;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let month_lengths = month_lengths(year);
    let mut month = 1u64;
    for &ml in &month_lengths {
        if remaining_days < ml {
            break;
        }
        remaining_days -= ml;
        month += 1;
    }
    let day = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, h, m, s, millis
    )
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_lengths(year: u64) -> [u64; 12] {
    let feb = if is_leap(year) { 29 } else { 28 };
    [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

// ---------------------------------------------------------------------------
// Internal handle — carries the live child and streaming tasks
// ---------------------------------------------------------------------------

struct CompanionHandle {
    child: Child,
    _stdout_task: JoinHandle<()>,
    _stderr_task: JoinHandle<()>,
    started_at_iso: String,
}

impl Drop for CompanionHandle {
    fn drop(&mut self) {
        // `start_kill` is the non-async variant — safe to call from a
        // destructor/Drop impl. We ignore the error: if the process is
        // already dead there's nothing to do; if the kill fails there's
        // nothing we can do from a destructor.
        let _ = self.child.start_kill();
    }
}

// ---------------------------------------------------------------------------
// Shared state managed via `app.manage()`
// ---------------------------------------------------------------------------

pub struct CompanionState {
    inner: Arc<Mutex<StateInner>>,
}

struct StateInner {
    handle: Option<CompanionHandle>,
    status: CompanionStatus,
}

impl CompanionState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateInner {
                handle: None,
                status: CompanionStatus::Stopped,
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Resolve repo root from the exe location
// ---------------------------------------------------------------------------
//
// Layout on disk:
//   <repo>/apps/turdmod-manager/src-tauri/target/<profile>/turdmod-manager.exe
//                                                   ^^^^^ 5 levels up to repo root
//
// In dev (`cargo tauri dev`) Tauri may set the cwd to the workspace root, so
// we try the exe path first and fall back to cwd.
fn resolve_repo_root() -> Option<PathBuf> {
    // Try from the exe path (5 parents up).
    if let Ok(exe) = std::env::current_exe() {
        let mut p = exe.as_path();
        let mut depth = 0;
        loop {
            if depth == 5 {
                return Some(p.to_path_buf());
            }
            p = p.parent()?;
            depth += 1;
        }
    }
    // Fall back to cwd — works in `cargo tauri dev`.
    std::env::current_dir().ok()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub async fn spawn_companion<R: Runtime>(
    state: &CompanionState,
    app_handle: &AppHandle<R>,
    settings: &ManagerSettings,
    rcon_password: Option<String>,
) -> Result<(), String> {
    let mut inner = state.inner.lock();

    // Kill any existing process first.
    inner.handle = None;
    inner.status = CompanionStatus::Starting;
    drop(inner);

    let repo_root = resolve_repo_root()
        .ok_or_else(|| "could not resolve repo root".to_string())?;

    tracing::info!("spawning companion from repo root: {}", repo_root.display());

    let mut cmd = Command::new("pnpm");
    cmd.arg("--filter")
        .arg("@turdmod/turdmod-companion")
        .arg("dev")
        .current_dir(&repo_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    // Pass settings as env vars.
    if let Some(dir) = &settings.scum_server_logs_dir {
        cmd.env("SCUM_SERVER_LOGS_DIR", dir.to_string_lossy().as_ref());
    }
    if let Some(host) = &settings.rcon_host {
        cmd.env("TURDMOD_RCON_HOST", host);
    }
    if let Some(port) = settings.rcon_port {
        cmd.env("TURDMOD_RCON_PORT", port.to_string());
    }
    if let Some(pass) = rcon_password {
        cmd.env("TURDMOD_RCON_PASS", pass);
    }
    if let Some(webhook) = &settings.discord_webhook {
        cmd.env("TURDMOD_DISCORD_WEBHOOK", webhook);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn companion: {}", e))?;

    let pid = child.id().unwrap_or(0);
    let started_at_iso = iso_now();

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let app_clone = app_handle.clone();
    let state_clone = state.inner.clone();
    let started_iso_clone = started_at_iso.clone();

    // Stdout streaming task.
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let payload = StdoutLine {
                ts: iso_now(),
                level: parse_level(&line),
                raw: line,
            };
            let _ = app_clone.emit(STDOUT_EVENT, &payload);
        }
        // Stdout EOF means the process exited. Update status.
        let mut inner = state_clone.lock();
        if matches!(inner.status, CompanionStatus::Running { .. }) {
            inner.status = CompanionStatus::Crashed { exit_code: None };
        }
        inner.handle = None;
    });

    // Stderr streaming task (also emit as error-level lines).
    let app_clone2 = app_handle.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let payload = StdoutLine {
                ts: iso_now(),
                level: "error",
                raw: line,
            };
            let _ = app_clone2.emit(STDOUT_EVENT, &payload);
        }
    });

    let mut inner = state.inner.lock();
    inner.status = CompanionStatus::Running {
        pid,
        started_at_iso: started_iso_clone,
    };
    inner.handle = Some(CompanionHandle {
        child,
        _stdout_task: stdout_task,
        _stderr_task: stderr_task,
        started_at_iso,
    });

    Ok(())
}

pub async fn stop_companion(state: &CompanionState) -> Result<(), String> {
    let mut inner = state.inner.lock();
    // Dropping the handle triggers Drop::drop which calls start_kill().
    inner.handle = None;
    inner.status = CompanionStatus::Stopped;
    Ok(())
}

pub async fn restart_companion<R: Runtime>(
    state: &CompanionState,
    app_handle: &AppHandle<R>,
    settings: &ManagerSettings,
    rcon_password: Option<String>,
) -> Result<(), String> {
    stop_companion(state).await?;
    spawn_companion(state, app_handle, settings, rcon_password).await
}

pub fn current_status(state: &CompanionState) -> CompanionStatus {
    state.inner.lock().status.clone()
}