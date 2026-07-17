// Engine lifecycle: install + start/stop of the UE4SS + bridge + loader DLL
// trio against a target GameServer.exe install.
//
// Install step copies UE4SS.dll + TurdMODEngineBridge.dll into the canonical
// UE4SS layout under the SCUMServer install. Start step shells out to
// turdmod-launcher.exe which does suspended-process inject of UE4SS.dll +
// turdmod_server_loader.dll into GameServer.exe.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::task::JoinHandle;

pub const STDOUT_EVENT: &str = "engine://stdout";
pub const LOG_EVENT: &str = "engine://log";

// ---------------------------------------------------------------------------
// Default build artifact paths — overridable in EnginePaths
// ---------------------------------------------------------------------------

const DEFAULT_UE4SS_DLL: &str =
    r"C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\UE4SS.dll";
const DEFAULT_BRIDGE_DLL: &str =
    r"C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll";

const LOADER_LOG_PATH: &str = r"C:\ProgramData\TurdMOD\server-loader.log";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePaths {
    pub ue4ss_dll: Option<PathBuf>,
    pub bridge_dll: Option<PathBuf>,
    pub loader_dll: Option<PathBuf>,
    pub launcher_exe: Option<PathBuf>,
}

impl EnginePaths {
    fn resolve(&self) -> ResolvedPaths {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        // ../../../.. from src-tauri => repo root, then walk to the canonical default locations.
        let repo_root = here.ancestors().nth(3).unwrap_or(here).to_path_buf();
        let loader_default = repo_root.join("apps")
            .join("turdmod-server-loader")
            .join("target").join("release")
            .join("turdmod_server_loader.dll");
        let launcher_default = repo_root.join("apps")
            .join("turdmod-loader").join("launcher")
            .join("target").join("release")
            .join("turdmod-launcher.exe");

        ResolvedPaths {
            ue4ss_dll:    self.ue4ss_dll.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_UE4SS_DLL)),
            bridge_dll:   self.bridge_dll.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_BRIDGE_DLL)),
            loader_dll:   self.loader_dll.clone().unwrap_or(loader_default),
            launcher_exe: self.launcher_exe.clone().unwrap_or(launcher_default),
        }
    }
}

struct ResolvedPaths {
    ue4ss_dll:    PathBuf,
    bridge_dll:   PathBuf,
    loader_dll:   PathBuf,
    launcher_exe: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EngineStatus {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineLogLine {
    pub ts:     String,
    pub source: &'static str, // "launcher" | "ue4ss" | "loader"
    pub level:  &'static str, // "info" | "warn" | "error"
    pub raw:    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub ue4ss_dll_path:  PathBuf,
    pub bridge_dll_path: PathBuf,
    pub settings_ini:    PathBuf,
    pub mods_txt:        PathBuf,
    pub wrote_ini:       bool,
    pub wrote_mods_line: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct EngineState {
    inner: Arc<Mutex<StateInner>>,
}

struct StateInner {
    scum_pid: Option<u32>,
    status:   EngineStatus,
    tailers:  Vec<JoinHandle<()>>,
}

impl EngineState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateInner {
                scum_pid: None,
                status:   EngineStatus::Stopped,
                tailers:  Vec::new(),
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------

pub fn install_engine(
    server_install: &Path,
    paths: &EnginePaths,
) -> Result<InstallReport, String> {
    let resolved = paths.resolve();

    if !resolved.ue4ss_dll.exists() {
        return Err(format!("UE4SS.dll not found at {}", resolved.ue4ss_dll.display()));
    }
    if !resolved.bridge_dll.exists() {
        return Err(format!("TurdMODEngineBridge.dll not found at {}", resolved.bridge_dll.display()));
    }

    let win64 = server_install.join("SCUM").join("Binaries").join("Win64");
    if !win64.is_dir() {
        return Err(format!(
            "SCUMServer install does not have SCUM/Binaries/Win64: {}",
            win64.display()
        ));
    }

    let ue4ss_dir   = win64.join("UE4SS");
    let mods_dir    = ue4ss_dir.join("Mods");
    let bridge_mod  = mods_dir.join("TurdMODEngineBridge");
    let bridge_dlls = bridge_mod.join("dlls");

    for d in [&ue4ss_dir, &mods_dir, &bridge_mod, &bridge_dlls] {
        std::fs::create_dir_all(d).map_err(|e| format!("mkdir {}: {}", d.display(), e))?;
    }

    let ue4ss_dst  = ue4ss_dir.join("UE4SS.dll");
    let bridge_dst = bridge_dlls.join("main.dll");
    std::fs::copy(&resolved.ue4ss_dll, &ue4ss_dst)
        .map_err(|e| format!("copy UE4SS.dll: {}", e))?;
    std::fs::copy(&resolved.bridge_dll, &bridge_dst)
        .map_err(|e| format!("copy bridge: {}", e))?;

    let settings_ini = ue4ss_dir.join("UE4SS-settings.ini");
    let wrote_ini = if !settings_ini.exists() {
        std::fs::write(&settings_ini,
            "[Debug]\nConsoleEnabled = 1\nGuiConsoleEnabled = 0\nGuiConsoleVisible = 0\n",
        ).map_err(|e| format!("write settings.ini: {}", e))?;
        true
    } else {
        false
    };

    let mods_txt = mods_dir.join("mods.txt");
    let mods_contents = std::fs::read_to_string(&mods_txt).unwrap_or_default();
    let wrote_mods_line = if !mods_contents.contains("TurdMODEngineBridge") {
        let mut next = mods_contents;
        if !next.ends_with('\n') && !next.is_empty() { next.push('\n'); }
        next.push_str("TurdMODEngineBridge : 1\n");
        std::fs::write(&mods_txt, next)
            .map_err(|e| format!("write mods.txt: {}", e))?;
        true
    } else {
        false
    };

    Ok(InstallReport {
        ue4ss_dll_path:  ue4ss_dst,
        bridge_dll_path: bridge_dst,
        settings_ini,
        mods_txt,
        wrote_ini,
        wrote_mods_line,
    })
}

// ---------------------------------------------------------------------------
// Spawn / stop
// ---------------------------------------------------------------------------

pub async fn spawn_engine<R: Runtime>(
    state: &EngineState,
    app: &AppHandle<R>,
    server_install: &Path,
    paths: &EnginePaths,
    skip_safety_check: bool,
) -> Result<(), String> {
    let resolved = paths.resolve();

    for (label, p) in [
        ("UE4SS.dll", &resolved.ue4ss_dll),
        ("turdmod_server_loader.dll", &resolved.loader_dll),
        ("turdmod-launcher.exe", &resolved.launcher_exe),
    ] {
        if !p.exists() {
            return Err(format!("{} missing: {}", label, p.display()));
        }
    }

    let win64 = server_install.join("SCUM").join("Binaries").join("Win64");
    let scum_server = win64.join("GameServer.exe");
    if !scum_server.exists() {
        return Err(format!("GameServer.exe not found at {}", scum_server.display()));
    }
    // Critical: load UE4SS.dll from the INSTALLED location so UE4SS finds
    // its Mods/ folder (relative to the DLL's directory). If we pass the
    // build-tree path here, UE4SS looks for Mods alongside the build
    // artifact and never finds TurdMODEngineBridge.
    let installed_ue4ss = win64.join("UE4SS").join("UE4SS.dll");
    if !installed_ue4ss.exists() {
        return Err(format!(
            "Installed UE4SS.dll not found at {} — run Install DLLs first",
            installed_ue4ss.display()
        ));
    }

    // Stop anything currently running before launching.
    stop_engine(state).ok();
    {
        let mut inner = state.inner.lock();
        inner.status = EngineStatus::Starting;
    }

    // GameServer.exe requires elevation (requireAdministrator manifest), so the
    // launcher MUST run elevated for its CreateProcessW to succeed. We trigger
    // UAC via ShellExecuteExW with verb="runas". This crosses the elevation
    // boundary, so launcher stdout/stderr is not pipe-able — we lose live
    // launcher output but retain ue4ss.log + server-loader.log file tailers.
    emit_log(app, "launcher", "info",
        "requesting Windows UAC elevation to launch GameServer.exe…".to_string());

    let launcher_path = resolved.launcher_exe.clone();
    let mut launcher_args: Vec<String> = vec![
        "--scum".into(),       scum_server.to_string_lossy().into_owned(),
        "--dll".into(),        installed_ue4ss.to_string_lossy().into_owned(),
        "--extra-dll".into(),  resolved.loader_dll.to_string_lossy().into_owned(),
    ];
    if skip_safety_check {
        launcher_args.push("--skip-safety-check".into());
    }

    let exit_code = tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = launcher_args.iter().map(String::as_str).collect();
        elevate_launch(&launcher_path, &refs)
    })
    .await
    .map_err(|e| format!("blocking task: {}", e))?
    .map_err(|e| {
        let mut inner = state.inner.lock();
        inner.status = EngineStatus::Crashed { exit_code: None };
        e
    })?;

    if exit_code != 0 {
        let mut inner = state.inner.lock();
        inner.status = EngineStatus::Crashed { exit_code: Some(exit_code) };
        return Err(format!(
            "elevated launcher exited with status {} — SCUMServer may not have started",
            exit_code,
        ));
    }

    // Launcher exited OK. The SCUM PID is no longer in our pipe (UAC boundary),
    // so find it by scanning the running process list.
    emit_log(app, "launcher", "info",
        "launcher OK — locating GameServer.exe in process list".to_string());
    let pid = find_scum_server_pid().ok_or_else(|| {
        let mut inner = state.inner.lock();
        inner.status = EngineStatus::Crashed { exit_code: Some(0) };
        "launcher succeeded but GameServer.exe not visible in tasklist".to_string()
    })?;

    let started_at_iso = iso_now();
    emit_log(app, "launcher", "info",
        format!("tracking GameServer.exe pid={}", pid));

    // File tailers for UE4SS log + loader log + SCUM's own engine log.
    // Adding SCUM.log so the Console shows raid manager spawns, quest
    // activity, kill events, etc. — the stuff needed to validate the
    // bridge smoke-test handlers in real time.
    let ue4ss_log = server_install.join("SCUM").join("Binaries").join("Win64").join("UE4SS").join("ue4ss.log");
    let loader_log = PathBuf::from(LOADER_LOG_PATH);
    let scum_log = server_install.join("SCUM").join("Saved").join("Logs").join("SCUM.log");
    emit_log(app, "launcher", "info",
        format!("spawning tailer for {}", ue4ss_log.display()));
    let tail_ue4ss = spawn_file_tailer(app.clone(), ue4ss_log, "ue4ss");
    emit_log(app, "launcher", "info",
        format!("spawning tailer for {}", loader_log.display()));
    let tail_loader = spawn_file_tailer(app.clone(), loader_log, "loader");
    emit_log(app, "launcher", "info",
        format!("spawning tailer for {}", scum_log.display()));
    let tail_scum = spawn_file_tailer(app.clone(), scum_log, "scum");

    // Watcher — polls GameServer.exe; flips Running → Crashed when it dies.
    let app_w = app.clone();
    let state_w = state.inner.clone();
    let watcher: JoinHandle<()> = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            if let Some(code) = process_exit_code(pid) {
                emit_log(&app_w, "launcher", "warn",
                    format!("GameServer.exe (pid {}) exited with code {}", pid, code));
                let mut inner = state_w.lock();
                if matches!(inner.status, EngineStatus::Running { .. }) {
                    inner.status = EngineStatus::Crashed { exit_code: Some(code) };
                }
                inner.scum_pid = None;
                for t in inner.tailers.drain(..) { t.abort(); }
                break;
            }
        }
    });

    let mut inner = state.inner.lock();
    inner.scum_pid = Some(pid);
    inner.status = EngineStatus::Running { pid, started_at_iso };
    inner.tailers.push(tail_ue4ss);
    inner.tailers.push(tail_loader);
    inner.tailers.push(tail_scum);
    inner.tailers.push(watcher);

    Ok(())
}

pub fn stop_engine(state: &EngineState) -> Result<(), String> {
    let pid_to_kill = {
        let mut inner = state.inner.lock();
        for t in inner.tailers.drain(..) { t.abort(); }
        let p = inner.scum_pid.take();
        inner.status = EngineStatus::Stopped;
        p
    };
    if let Some(pid) = pid_to_kill {
        terminate_process(pid);
    }
    Ok(())
}

pub fn current_status(state: &EngineState) -> EngineStatus {
    state.inner.lock().status.clone()
}

// ---------------------------------------------------------------------------
// SCUM PID parsing + Win32 process helpers
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn process_exit_code(pid: u32) -> Option<i32> {
    use windows_sys::Win32::Foundation::{CloseHandle, STATUS_PENDING};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            // OpenProcess can fail even with PROCESS_QUERY_LIMITED_INFORMATION
            // when crossing an elevation boundary (Manager runs as user,
            // SCUMServer runs as Administrator). Old behavior: assume the
            // process is gone and trip the crash watcher — which aborts the
            // log tailers and makes the Console show "Nothing running" while
            // SCUMServer is still happily alive. Real fix: fall back to a
            // tasklist scan to distinguish "can't open" from "doesn't exist".
            return if pid_in_tasklist(pid) { None } else { Some(-1) };
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        if ok == 0 { return None; }
        // STILL_ACTIVE (259 / STATUS_PENDING) means the process is still running.
        if code == STATUS_PENDING as u32 { return None; }
        Some(code as i32)
    }
}

/// Cheap "is this PID currently a live process?" check that doesn't need
/// any access rights on the target. Used as a fallback when OpenProcess
/// is denied (commonly: non-elevated caller, elevated target).
#[cfg(windows)]
fn pid_in_tasklist(pid: u32) -> bool {
    let needle = format!(",\"{}\",", pid);
    let out = match std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    let s = String::from_utf8_lossy(&out.stdout);
    s.contains(&needle)
}

#[cfg(not(windows))]
fn process_exit_code(_pid: u32) -> Option<i32> { Some(-1) }

#[cfg(windows)]
fn terminate_process(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_TERMINATE,
    };
    // Fast path: direct terminate (works for processes our token can touch).
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            let ok = TerminateProcess(h, 1);
            CloseHandle(h);
            if ok != 0 { return; }
        }
    }
    // Fallback: GameServer.exe is elevated — invoke taskkill via UAC.
    let taskkill = PathBuf::from(r"C:\Windows\System32\taskkill.exe");
    let pid_str = pid.to_string();
    let _ = elevate_launch(&taskkill, &["/F", "/PID", &pid_str]);
}

#[cfg(not(windows))]
fn terminate_process(_pid: u32) {}

// ---------------------------------------------------------------------------
// UAC elevation via ShellExecuteExW
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn quote_arg(s: &str) -> String {
    if s.contains(' ') || s.is_empty() {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Launch `exe` with `args`, triggering Windows UAC. Blocks the calling
/// thread until the elevated process exits. Returns the exit code.
#[cfg(windows)]
pub(crate) fn elevate_launch(exe: &Path, args: &[&str]) -> Result<i32, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let verb_w = to_wide("runas");
    let exe_w  = to_wide(&exe.to_string_lossy());
    let params = args.iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" ");
    let params_w = to_wide(&params);

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask  = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb       = verb_w.as_ptr();
    info.lpFile       = exe_w.as_ptr();
    info.lpParameters = params_w.as_ptr();
    info.nShow        = SW_HIDE as i32;

    unsafe {
        if ShellExecuteExW(&mut info) == 0 {
            let err = GetLastError();
            if err == ERROR_CANCELLED {
                return Err("user declined the UAC elevation prompt".to_string());
            }
            return Err(format!("ShellExecuteExW failed: GetLastError={}", err));
        }
        if info.hProcess.is_null() {
            return Err("ShellExecuteExW returned no process handle (no elevation?)".to_string());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if ok == 0 {
            return Err("GetExitCodeProcess failed after elevated launch".to_string());
        }
        Ok(code as i32)
    }
}

#[cfg(not(windows))]
fn elevate_launch(_exe: &Path, _args: &[&str]) -> Result<i32, String> {
    Err("UAC elevation only supported on Windows".to_string())
}

/// Find a running GameServer.exe via `tasklist`. Returns the first match.
fn find_scum_server_pid() -> Option<u32> {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq GameServer.exe", "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    // Format: "GameServer.exe","8932","Services","0","..."
    for line in s.lines() {
        let parts: Vec<&str> = line.split("\",\"").collect();
        if parts.len() < 2 { continue; }
        let pid_str = parts[1].trim_matches('"');
        if let Ok(pid) = pid_str.parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// File tailer — polls a file for new lines and emits engine://log
// ---------------------------------------------------------------------------

fn spawn_file_tailer<R: Runtime>(
    app: AppHandle<R>,
    path: PathBuf,
    source: &'static str,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        eprintln!("[tailer] start source={} path={}", source, path.display());
        let mut last_size: u64 = 0;
        let mut buffer = String::new();
        let mut first_pass = true;
        let mut last_metadata_err: Option<String> = None;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let meta = match tokio::fs::metadata(&path).await {
                Ok(m) => {
                    if last_metadata_err.take().is_some() {
                        emit_log(&app, source, "info",
                            format!("[tailer] file now accessible at {}", path.display()));
                    }
                    m
                }
                Err(e) => {
                    let msg = format!("metadata error: {}", e);
                    if last_metadata_err.as_deref() != Some(&msg) {
                        eprintln!("[tailer] {} {}: {}", source, path.display(), msg);
                        emit_log(&app, source, "warn",
                            format!("[tailer] cannot stat {}: {} (will retry)",
                                    path.display(), e));
                        last_metadata_err = Some(msg);
                    }
                    continue;
                }
            };
            let cur_size = meta.len();
            if first_pass {
                // Show the last ~32 KB of recent log content on startup
                // (typically a few hundred lines — enough to see what
                // the engine has been doing without flooding). Then
                // continue tailing live from current offset. Without
                // this, the Console looks empty after Start Engine
                // unless something new happens to be logged.
                const PRELOAD_BYTES: u64 = 32 * 1024;
                let start_from = cur_size.saturating_sub(PRELOAD_BYTES);
                if start_from > 0 {
                    // We're starting mid-file. The first partial line up
                    // to the first '\n' will be incomplete — discard it
                    // so we don't emit a malformed leading entry.
                    if let Some(bytes) = read_range(&path, start_from, cur_size).await {
                        let text = String::from_utf8_lossy(&bytes);
                        let mut found_newline = false;
                        for (i, c) in text.char_indices() {
                            if c == '\n' {
                                buffer.push_str(&text[i + 1..]);
                                found_newline = true;
                                break;
                            }
                        }
                        if !found_newline {
                            // No newline in the slice — the whole tail
                            // is one (partial) line. Drop it.
                        }
                    }
                } else if let Some(bytes) = read_range(&path, 0, cur_size).await {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                }
                // Drain whatever we just buffered as proper lines.
                while let Some(nl) = buffer.find('\n') {
                    let line: String = buffer.drain(..=nl).collect();
                    let trimmed = line.trim_end_matches(&['\r', '\n'][..]).to_string();
                    if !trimmed.is_empty() {
                        emit_log(&app, source, parse_level(&trimmed), trimmed);
                    }
                }
                last_size = cur_size;
                first_pass = false;
                emit_log(&app, source, "info",
                    format!("[tailer] live from offset {} (preloaded {} bytes)",
                            cur_size, cur_size - start_from));
                continue;
            }
            if cur_size < last_size {
                last_size = 0; // truncation / rotation
            }
            if cur_size == last_size {
                continue;
            }
            let new_bytes = read_range(&path, last_size, cur_size).await;
            last_size = cur_size;
            if let Some(bytes) = new_bytes {
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(nl) = buffer.find('\n') {
                    let line: String = buffer.drain(..=nl).collect();
                    let trimmed = line.trim_end_matches(&['\r', '\n'][..]).to_string();
                    if !trimmed.is_empty() {
                        emit_log(&app, source, parse_level(&trimmed), trimmed);
                    }
                }
            }
        }
    })
}

async fn read_range(path: &Path, from: u64, to: u64) -> Option<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
    let mut f = tokio::fs::File::open(path).await.ok()?;
    f.seek(SeekFrom::Start(from)).await.ok()?;
    let want = (to - from) as usize;
    let mut buf = vec![0u8; want];
    let n = f.read(&mut buf).await.ok()?;
    buf.truncate(n);
    Some(buf)
}

// ---------------------------------------------------------------------------
// Emit helpers
// ---------------------------------------------------------------------------

fn emit_log<R: Runtime>(app: &AppHandle<R>, source: &'static str, level: &'static str, raw: String) {
    let payload = EngineLogLine { ts: iso_now(), source, level, raw: raw.clone() };
    let log_r = app.emit(LOG_EVENT, &payload);
    let std_r = app.emit(STDOUT_EVENT, &payload);
    if log_r.is_err() || std_r.is_err() {
        eprintln!("[emit_log] FAILED source={} log={:?} stdout={:?} raw={}",
                  source, log_r, std_r, raw);
    }
}

fn parse_level(line: &str) -> &'static str {
    let upper = line.to_ascii_uppercase();
    if upper.contains("ERROR") || upper.contains(" ERR ") { "error" }
    else if upper.contains("WARN") { "warn" }
    else { "info" }
}

// Reuse the companion's no-dep ISO formatter. Copied inline to avoid
// cross-module coupling on a private helper.
fn iso_now() -> String {
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if remaining < dy { break; }
        remaining -= dy;
        year += 1;
    }
    let feb = if is_leap(year) { 29 } else { 28 };
    let months = [31u64, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u64;
    for &ml in &months {
        if remaining < ml { break; }
        remaining -= ml;
        month += 1;
    }
    let day = remaining + 1;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, h, m, s, millis)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
