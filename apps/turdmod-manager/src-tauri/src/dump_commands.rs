//! Tauri commands for the Dump Management page.
//!
//! Front-end calls these via the snake_case literal name (per the
//! Tauri naming gotcha — `rename_all = "camelCase"` renames parameters,
//! not the function itself). See `apps/turdmod-manager/CLAUDE.md`.
//!
//! All commands are thin GUI wrappers; the heavy lifting lives in the
//! sibling `scumdump` CLI at `C:/Development/Claude/scumdump/`. The only
//! local work this module does is reading the sibling's config + meta
//! files and shelling out to `pnpm <phase>` with stdout streamed back
//! to the JS layer over a Tauri event.

use std::path::PathBuf;
use std::process::Stdio;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::dump::{
    self, ArchiveEntry, DiffReport, DumpConfig, DumpMeta, PhaseAResults, PhaseBMeta, PhaseCMeta,
};
use crate::engine;

pub const DUMP_LOG_EVENT: &str = "dump://log";

// ---------------------------------------------------------------------------
// Status payload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpStatus {
    /// Whether the sibling scumdump repo is reachable on disk.
    pub scumdump_present: bool,
    /// Root of the sibling repo (`C:/Development/Claude/scumdump/` by default).
    pub scumdump_root: Option<String>,
    /// SCUM SERVER build id Steam currently has installed
    /// (from `appmanifest_3792580.acf`).
    pub steam_build: Option<String>,
    /// SCUM CLIENT build id Steam currently has installed
    /// (from `appmanifest_513710.acf`).
    pub steam_client_build: Option<String>,
    /// Build id of the newest extracted dump (parsed from `v<id>` dir name).
    pub extracted_build: Option<String>,
    /// `true` iff Steam build differs from the latest extracted dump.
    pub update_available: bool,
    /// AES key fingerprint, e.g. `0x0B1F4E54…CA81`, for the status pane.
    pub aes_fingerprint: Option<String>,
    /// Per-phase counts + timestamps.
    pub phase_a: Option<PhaseAResults>,
    pub phase_a_dumped_at: Option<String>,
    /// Server-side Dumper-7 SDK (target = SCUMServer.exe).
    pub phase_b: Option<PhaseBMeta>,
    /// Client-side Dumper-7 SDK (target = SCUM.exe). Added 2026-05-21
    /// for full dual-target coverage.
    pub phase_b_client: Option<PhaseBMeta>,
    pub phase_c: Option<PhaseCMeta>,
    /// Absolute path to the latest extracted build directory; useful for
    /// the "Open Dump Folder" button.
    pub latest_build_dir: Option<String>,
}

/// Snapshot of everything the GUI needs in one round-trip — Steam build,
/// extracted build, per-phase counts, AES fingerprint.
///
/// Designed to never throw: missing scumdump / missing manifest / malformed
/// JSON all fall through to `Some(None)` fields rather than `Err`, so the
/// page can render a useful empty state instead of an error toast.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_status() -> Result<DumpStatus, String> {
    let root = dump::scumdump_root();
    if root.is_none() {
        return Ok(DumpStatus {
            scumdump_present: false,
            scumdump_root: None,
            steam_build: dump::read_steam_buildid(),
            steam_client_build: dump::read_steam_client_buildid(),
            extracted_build: None,
            update_available: false,
            aes_fingerprint: None,
            phase_a: None,
            phase_a_dumped_at: None,
            phase_b: None,
            phase_b_client: None,
            phase_c: None,
            latest_build_dir: None,
        });
    }

    let cfg = DumpConfig::load();
    let aes_fp = cfg.as_ref().and_then(|c| c.aes_fingerprint());

    let latest = dump::latest_build_dir();
    let extracted_build = latest.as_deref().and_then(dump::extracted_buildid);
    let steam_build = dump::read_steam_buildid();
    let update_available = match (&steam_build, &extracted_build) {
        (Some(s), Some(e)) => s != e,
        _ => false,
    };
    let meta = latest.as_deref().and_then(DumpMeta::load);

    Ok(DumpStatus {
        scumdump_present: true,
        scumdump_root: root.map(|p| p.to_string_lossy().into_owned()),
        steam_build,
        steam_client_build: dump::read_steam_client_buildid(),
        extracted_build,
        update_available,
        aes_fingerprint: aes_fp,
        phase_a: meta.as_ref().map(|m| m.results.clone()),
        phase_a_dumped_at: meta.as_ref().and_then(|m| m.dumped_at.clone()),
        phase_b: meta.as_ref().and_then(|m| m.phase_b.clone()),
        phase_b_client: meta.as_ref().and_then(|m| m.phase_b_client.clone()),
        phase_c: meta.as_ref().and_then(|m| m.phase_c.clone()),
        latest_build_dir: latest.map(|p| p.to_string_lossy().into_owned()),
    })
}

// ---------------------------------------------------------------------------
// Update check — light variant of dump_status, returns just the canary.
// Called once at Manager session start.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpUpdateCheck {
    pub steam_build: Option<String>,
    pub extracted_build: Option<String>,
    pub update_available: bool,
}

/// Fast "should we show the update banner?" check. Reads only the Steam
/// manifest + the newest extracted dir's name (no JSON parsing). Designed
/// to be called once at boot.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_check_updates() -> Result<DumpUpdateCheck, String> {
    let steam_build = dump::read_steam_buildid();
    let extracted_build = dump::latest_build_dir()
        .as_deref()
        .and_then(dump::extracted_buildid);
    let update_available = match (&steam_build, &extracted_build) {
        (Some(s), Some(e)) => s != e,
        _ => false,
    };
    Ok(DumpUpdateCheck {
        steam_build,
        extracted_build,
        update_available,
    })
}

// ---------------------------------------------------------------------------
// Run a phase — shells out to `pnpm phase-<x>` in the sibling repo and
// streams stdout/stderr lines as Tauri events. Returns the exit code.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpLogLine {
    pub phase: String,
    pub stream: &'static str, // "stdout" | "stderr"
    pub line: String,
}

/// Valid `phase` values: `"detect"`, `"phase-a"`, `"phase-b"`,
/// `"phase-b-client"`, `"phase-c"`. Anything else returns an error
/// rather than shelling out a bad arg.
fn validate_phase(phase: &str) -> Result<&'static str, String> {
    match phase {
        "detect" => Ok("detect"),
        "phase-a" => Ok("phase-a"),
        "phase-b" => Ok("phase-b"),
        "phase-b-client" => Ok("phase-b-client"),
        "phase-c" => Ok("phase-c"),
        other => Err(format!(
            "unknown phase '{}'; expected detect|phase-a|phase-b|phase-b-client|phase-c",
            other
        )),
    }
}

async fn run_phase_inner<R: Runtime>(
    app: &AppHandle<R>,
    phase: &str,
) -> Result<i32, String> {
    let phase = validate_phase(phase)?;
    let root = dump::scumdump_root()
        .ok_or_else(|| "scumdump repo not found; expected at C:/Development/Claude/scumdump/".to_string())?;

    // `pnpm <script>` runs the script from package.json. The scumdump
    // package.json maps `phase-a` / `phase-b` / `phase-c` / `detect`
    // each to `tsx src/cli.ts <phase>`.
    //
    // Windows gotcha: on Windows, pnpm is installed as `pnpm.cmd` (a
    // batch wrapper), not `pnpm.exe`. `tokio::process::Command::new`
    // looks for the exact program name on PATH and won't find the
    // .cmd. Invoking via `cmd /c pnpm <phase>` lets cmd.exe do the
    // PATHEXT resolution. On non-Windows we still use `pnpm` directly.
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/c").arg("pnpm").arg(phase);
        c
    } else {
        let mut c = Command::new("pnpm");
        c.arg(phase);
        c
    };
    cmd.current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn `pnpm {}`: {}", phase, e))?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let phase_owned = phase.to_string();
    let app_out = app.clone();
    let app_err = app.clone();
    let phase_out = phase_owned.clone();
    let phase_err = phase_owned.clone();

    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = app_out.emit(
                DUMP_LOG_EVENT,
                DumpLogLine {
                    phase: phase_out.clone(),
                    stream: "stdout",
                    line,
                },
            );
        }
    });
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = app_err.emit(
                DUMP_LOG_EVENT,
                DumpLogLine {
                    phase: phase_err.clone(),
                    stream: "stderr",
                    line,
                },
            );
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("waiting on `pnpm {}`: {}", phase, e))?;
    let _ = stdout_task.await;
    let _ = stderr_task.await;
    Ok(status.code().unwrap_or(-1))
}

/// Run a single dump phase. Lines from stdout + stderr are emitted as
/// `dump://log` events with the originating phase tagged so the UI can
/// filter / colour by stream.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_run_phase<R: Runtime>(
    app: AppHandle<R>,
    phase: String,
) -> Result<i32, String> {
    run_phase_inner(&app, &phase).await
}

/// Sequentially run `detect → phase-a → phase-b → phase-c`. Stops on
/// the first non-zero exit. Returns the exit code of the last attempted
/// phase, so the UI can distinguish "all four succeeded" (0) from
/// "stopped at phase B" (non-zero).
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_run_all<R: Runtime>(app: AppHandle<R>) -> Result<i32, String> {
    for phase in &["detect", "phase-a", "phase-b", "phase-c"] {
        let code = run_phase_inner(&app, phase).await?;
        if code != 0 {
            return Ok(code);
        }
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// Open the latest build dir in Explorer.
// ---------------------------------------------------------------------------

/// Opens the latest extracted build dir in Explorer. Errors if scumdump
/// isn't installed or no `v*` dir has been produced yet.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_open_folder() -> Result<String, String> {
    let dir = dump::latest_build_dir().ok_or_else(|| {
        "no extracted build directory found; run phase-a first".to_string()
    })?;
    let path_str = dir.to_string_lossy().into_owned();

    #[cfg(windows)]
    {
        // `cmd /c start "" "<path>"` is the reliable Windows-default-app
        // opener. The empty `""` is the window title that `start` requires
        // when the path argument is quoted (otherwise it interprets the
        // quoted path as the title).
        use std::process::Command as StdCommand;
        StdCommand::new("cmd")
            .args(["/c", "start", "", &path_str])
            .spawn()
            .map_err(|e| format!("spawn explorer: {}", e))?;
    }

    Ok(path_str)
}

// ---------------------------------------------------------------------------
// Re-extract AES key (calls into scumdump's aes_finder tool).
// ---------------------------------------------------------------------------

/// Runs scumdump's `tools/aes_finder` against SCUMServer.exe and rewrites
/// `scumdump.config.json` with the discovered key. The tool itself is the
/// canonical implementation — Manager just shells out and waits.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_extract_aes<R: Runtime>(app: AppHandle<R>) -> Result<i32, String> {
    // Reuse the same `pnpm detect` script path — the scumdump CLI's
    // `detect` subcommand handles AES extraction as part of its setup
    // pass (re-reads the executable, computes the key, persists it). If
    // a dedicated `pnpm aes` script ships later, switch this string.
    run_phase_inner(&app, "detect").await
}

// ---------------------------------------------------------------------------
// DLL injection helpers — runtime CreateRemoteThread(LoadLibraryA).
// Wraps the inject.rs port of paliaplay/scripts/inject-dll.py.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectResult {
    pub target: String,
    pub dll_path: String,
    pub exit_code: i32,
    pub message: String,
}

fn dumper7_dll_path(target: &str) -> Result<PathBuf, String> {
    let root = dump::scumdump_root()
        .ok_or_else(|| "scumdump repo not found".to_string())?;
    // Per-target Dumper-7 fork. The server build hardcodes GObjects RVA
    // 0x712ED20 for SCUMServer.exe; the client build reverts to
    // auto-detect (or its own hardcoded RVA later) for SCUM.exe.
    // Mirrors scumdump's phase-b-sdk.ts dispatch.
    let repo = match target {
        "client" => "Dumper-7-Client",
        _ => "Dumper-7",
    };
    Ok(root
        .join("tools")
        .join(repo)
        .join("x64")
        .join("Release")
        .join("Dumper-7.dll"))
}

fn injector_exe_path() -> PathBuf {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = here.ancestors().nth(3).unwrap_or(here);
    repo_root
        .join("apps")
        .join("turdmod-loader")
        .join("launcher")
        .join("target")
        .join("release")
        .join("turdmod-injector.exe")
}

fn message_for_exit_code(code: i32) -> &'static str {
    match code {
        0 => "injection complete",
        1 => "bad args passed to turdmod-injector",
        2 => "target process not running — launch it first",
        3 => "OpenProcess failed (Access Denied? UAC cancelled?)",
        4 => "VirtualAllocEx failed in target process",
        5 => "WriteProcessMemory failed",
        6 => "GetProcAddress(LoadLibraryA) failed",
        7 => "CreateRemoteThread failed",
        8 => "LoadLibraryA returned NULL — DLL load refused",
        -1 => "UAC prompt cancelled / elevation refused",
        _ => "injector exited with unknown error",
    }
}

/// Inject Dumper-7.dll into the currently-running SCUM server or
/// client process. `target` is `"server"` (SCUMServer.exe) or
/// `"client"` (SCUM.exe / SCUM-Win64-Shipping.exe — tries both).
///
/// **Always elevated** — invokes `turdmod-injector.exe` via
/// ShellExecuteExW with verb="runas" so it works against both
/// non-elevated client and elevated SCUMServer.exe uniformly. UAC
/// will prompt; the elevated child runs the actual CreateRemoteThread
/// flow and exits with a status code mapped back to a user message.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_inject_dumper7(target: String) -> Result<InjectResult, String> {
    let (primary, fallback): (&str, Option<&str>) = match target.as_str() {
        "server" => ("SCUMServer.exe", None),
        "client" => ("SCUM-Win64-Shipping.exe", Some("SCUM.exe")),
        other => return Err(format!("unknown target '{}' (expected server|client)", other)),
    };
    let dll = dumper7_dll_path(&target)?;
    if !dll.is_file() {
        return Err(format!(
            "Dumper-7.dll not found at {}. Build it first (see scumdump README — \
             the {} target uses the {} variant).",
            dll.display(),
            target,
            if target == "client" { "Dumper-7-Client" } else { "Dumper-7" }
        ));
    }
    let injector = injector_exe_path();
    if !injector.is_file() {
        return Err(format!(
            "turdmod-injector.exe not found at {}. Build the launcher crate first.",
            injector.display()
        ));
    }

    let dll_str = dll.to_string_lossy().into_owned();
    let mut args: Vec<String> = vec![
        "--target".into(),
        primary.to_string(),
        "--dll".into(),
        dll_str.clone(),
    ];
    if let Some(f) = fallback {
        args.push("--fallback".into());
        args.push(f.to_string());
    }

    let injector_owned = injector.clone();
    let exit_code = tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        engine::elevate_launch(&injector_owned, &refs)
    })
    .await
    .map_err(|e| format!("blocking task: {}", e))??;

    Ok(InjectResult {
        target: target.clone(),
        dll_path: dll_str,
        exit_code,
        message: message_for_exit_code(exit_code).to_string(),
    })
}

/// Generic DLL inject command — exposed for future tooling (UE4SS,
/// custom mod DLLs). Same elevated path as `dump_inject_dumper7`.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_inject_dll(
    target: String,
    dll_path: String,
) -> Result<InjectResult, String> {
    let dll = std::path::PathBuf::from(&dll_path);
    if !dll.is_file() {
        return Err(format!("DLL not found at {}", dll.display()));
    }
    let injector = injector_exe_path();
    if !injector.is_file() {
        return Err(format!(
            "turdmod-injector.exe not found at {}. Build the launcher crate first.",
            injector.display()
        ));
    }
    let args: Vec<String> = vec![
        "--target".into(),
        target.clone(),
        "--dll".into(),
        dll_path.clone(),
    ];
    let injector_owned = injector.clone();
    let exit_code = tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        engine::elevate_launch(&injector_owned, &refs)
    })
    .await
    .map_err(|e| format!("blocking task: {}", e))??;

    Ok(InjectResult {
        target,
        dll_path,
        exit_code,
        message: message_for_exit_code(exit_code).to_string(),
    })
}

// ---------------------------------------------------------------------------
// Diff between two builds — reads _diff.json written by scumdump diff
// ---------------------------------------------------------------------------

/// Read `_diff.json` for the specified build. If `build` is `None`,
/// reads the most-recent build's diff. Returns `Ok(None)` if the diff
/// file isn't present (e.g. only one build exists, or diff hasn't been
/// run yet); returns an error only on actual failure conditions like
/// missing scumdump install.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_diff_summary(
    build: Option<String>,
) -> Result<Option<DiffReport>, String> {
    let target_build = match build {
        Some(b) => b,
        None => match dump::list_build_ids().first() {
            Some(b) => b.clone(),
            None => return Ok(None),
        },
    };
    let Some(extracted) = dump::extracted_root() else {
        return Err("scumdump data/extracted/ not found".to_string());
    };
    let dir = extracted.join(format!("v{}", target_build));
    Ok(dump::read_diff_report(&dir))
}

/// All extracted build ids, newest first. Lets the front-end build a
/// "diff from X to Y" picker.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_list_builds() -> Result<Vec<String>, String> {
    Ok(dump::list_build_ids())
}

// ---------------------------------------------------------------------------
// Forensic archive viewer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveListing {
    pub entries: Vec<ArchiveEntry>,
    /// Total entry count BEFORE filtering — useful for the UI to
    /// distinguish "no matches" from "empty archive."
    pub total: usize,
    /// Distinct keyType values present in the archive (sorted). Lets the
    /// UI build a filter dropdown without parsing all entries client-side.
    pub key_types: Vec<String>,
    /// Distinct scumBuild values present (sorted descending).
    pub builds: Vec<String>,
    /// Path to the archive file on disk, for the "Open in Notepad" affordance.
    pub archive_path: Option<String>,
}

/// Read the forensic keys archive from
/// `scumdump/data/archive/keys.jsonl`. Optional filter narrows by
/// `key_type` substring (case-insensitive) and/or `build` exact match.
/// Returns up to `limit` most-recent matching entries, plus aggregate
/// totals + the unique keyType / build sets for filter UIs.
#[tauri::command(rename_all = "camelCase")]
pub async fn dump_archive_entries(
    key_type: Option<String>,
    build: Option<String>,
    limit: Option<usize>,
) -> Result<ArchiveListing, String> {
    let all = dump::read_archive_entries();
    let total = all.len();

    // Collect unique values for the filter UIs BEFORE applying filters.
    let mut key_types: Vec<String> = all.iter().map(|e| e.key_type.clone()).collect();
    key_types.sort();
    key_types.dedup();

    let mut builds: Vec<String> = all
        .iter()
        .filter_map(|e| e.scum_build.clone())
        .collect();
    builds.sort_by(|a, b| b.cmp(a)); // descending — newest first
    builds.dedup();

    let kt_needle = key_type.as_deref().map(|s| s.to_lowercase());
    let build_eq = build.as_deref();

    let mut filtered: Vec<ArchiveEntry> = all
        .into_iter()
        .filter(|e| {
            if let Some(kt) = &kt_needle {
                if !e.key_type.to_lowercase().contains(kt) {
                    return false;
                }
            }
            if let Some(b) = build_eq {
                if e.scum_build.as_deref() != Some(b) {
                    return false;
                }
            }
            true
        })
        .collect();

    // Newest first.
    filtered.reverse();
    let cap = limit.unwrap_or(500);
    if filtered.len() > cap {
        filtered.truncate(cap);
    }

    Ok(ArchiveListing {
        entries: filtered,
        total,
        key_types,
        builds,
        archive_path: dump::archive_path().map(|p| p.to_string_lossy().into_owned()),
    })
}
