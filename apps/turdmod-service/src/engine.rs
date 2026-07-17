// SCUMServer lifecycle — start (CREATE_SUSPENDED + DLL inject), stop, monitor.
// Tracks PID, uptime, restart count. Monitor loop auto-restarts on crash.

use crate::config::Config;
use crate::inject::inject_dll;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    CreateProcessA, GetExitCodeProcess, OpenProcess, ResumeThread,
    CREATE_SUSPENDED, PROCESS_ALL_ACCESS, PROCESS_INFORMATION, STARTUPINFOA,
};
use windows::core::PSTR;

const MONITOR_INTERVAL: Duration = Duration::from_secs(5);
const KILL_SETTLE_SECS: u64 = 3;

// @inv: serialize ALL SCUM lifecycle ops (start + stop). Two start_server calls racing — the
// restart handler's start AND the monitor_loop's auto-restart (it snapshots the old pid as
// "crashed" while a restart is killing it) — both passed kill_existing() before EITHER launched,
// so BOTH spawned. Result: two ~10GB SCUMServer.exe, only one binds the port, the orphan eats
// RAM until OOM (OVH near-crash 2026-06-17). Holding this lock for the WHOLE of start_server
// (through setting running=true) + the alive-guard makes a duplicate launch impossible: the 2nd
// caller acquires the lock only after the 1st finished, finds the process alive, and no-ops.
static LIFECYCLE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Default, Clone)]
pub struct EngineState {
    pub running: bool,
    pub pid: Option<u32>,
    pub start_time: Option<Instant>,
    pub restarts: u32,
}

impl EngineState {
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0)
    }
}

pub type SharedState = Arc<Mutex<EngineState>>;

pub fn new_state() -> SharedState {
    Arc::new(Mutex::new(EngineState::default()))
}

pub async fn start_server(cfg: &Config, state: SharedState) -> Result<u32> {
    // Held for the whole fn — serializes against any concurrent start/stop (see LIFECYCLE_LOCK).
    let _lifecycle = LIFECYCLE_LOCK.lock().await;

    // Anti-duplicate guard: if a SCUM we're already tracking is alive, NEVER spawn a second one.
    // Legit restarts call stop_server first (running=false), so this only trips on the race where
    // a 2nd start_server arrives after the 1st already launched. Returns the existing pid.
    {
        let (running, pid) = { let s = state.lock().unwrap(); (s.running, s.pid) };
        if running {
            if let Some(p) = pid {
                if is_alive(p) {
                    tracing::warn!("start_server: pid {} already alive — skipping duplicate launch (race guard)", p);
                    return Ok(p);
                }
            }
        }
    }

    kill_existing().await?;
    sleep(Duration::from_secs(KILL_SETTLE_SECS)).await;

    // MOTD: INI edits don't work — SCUM regenerates defaults.
    // Welcome message will be handled via binary patch (NOP the send call).
    // See memory: project_welcome_pipeline_takeover.md

    // PRE-START: advance the rotating-PvP-zone state + edit SCUM.db while SCUM is
    // stopped (zones load at startup, so it must be applied before launch).
    crate::pvp_rotation::apply_next_rotation();
    // Vehicle repossession is now LIVE (vehicle_repo runs DestroyVehicle on its
    // tick — no restart, no DB surgery). No pre-start hook needed.
    // PRE-START: clear regrown trees inside base areas (cleared bases stay clear).
    crate::tree_preserve::sweep_base_areas();
    // PRE-START: flush queued elevated/dev-user changes (elevated_users is boot-read; DB free now).
    crate::elevated::sync_to_db(&cfg.scumdb_path);
    // PRE-START: clean daily keeper snapshot — SCUM is stopped here, so this is a coherent
    // (non-torn) restore point, unlike the hot 30-min checkpoints. Self-gated to ~once/day.
    crate::checkpoint::do_keeper_if_due();

    let pid = launch_and_inject(cfg).await.context("launch_and_inject")?;
    if let Err(e) = write_pipe_discovery(pid) {
        tracing::warn!("pipe.txt write failed: {} — events may not connect", e);
    }

    {
        let mut s = state.lock().unwrap();
        s.running = true;
        s.pid = Some(pid);
        s.start_time = Some(Instant::now());
    }

    tracing::info!("server started, pid={}", pid);

    // Auto-push trader zones to scummymap after DB settles
    if cfg.scummap_api_url.is_some() {
        let db_path = cfg.scumdb_path.clone();
        let api_url = cfg.scummap_api_url.clone().unwrap();
        let api_token = cfg.scummap_api_token.clone().unwrap_or_default();
        let map_id = cfg.scummap_map_id.clone().unwrap_or_default();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            match crate::scumdb::query_traders(&db_path) {
                Ok(traders) if !traders.is_empty() => {
                    let zones = crate::scummap_push::entities_to_zones(&traders, 35000);
                    match crate::scummap_push::push_trader_zones(&api_url, &api_token, &map_id, &zones).await {
                        Ok(n) => tracing::info!("auto-pushed {} trader zones to scummymap", n),
                        Err(e) => tracing::warn!("auto-push traders failed: {}", e),
                    }
                }
                Ok(_) => tracing::info!("no traders in SCUM.db — skipping auto-push"),
                Err(e) => tracing::warn!("scumdb read for auto-push: {}", e),
            }
        });
    }

    Ok(pid)
}

pub async fn stop_server(state: SharedState) -> Result<()> {
    // Kill EVERY SCUMServer.exe (the tracked one + any orphan from a prior restart that didn't die)
    // and VERIFY they're actually gone. @ctx 2026-06-11: an orphaned SCUM holding ~17.6GB of commit
    // alongside a freshly-launched one is what blew OVH past its commit limit and wedged the box.
    // Killing only the tracked PID would leave an orphan; we kill by image-name + confirm-dead.
    // @ctx: no pre-kill "save" — SCUM commits to its DB (EntitySystem auto-save) within seconds,
    // durable across a hard kill (verified on local).
    let _lifecycle = LIFECYCLE_LOCK.lock().await; // serialize against start_server (anti-double-spawn)

    // Flip state to stopped BEFORE killing. kill_existing takes seconds; if running stayed true
    // across that window the monitor_loop would see (running=true, process-dead) and "auto-restart"
    // an intentionally-stopped server — the exact double-spawn race that wedged OVH (2026-06-17).
    {
        let mut s = state.lock().unwrap();
        s.running = false;
        s.pid = None;
        s.start_time = None;
    }

    kill_existing().await?;

    Ok(())
}

// True if any SCUMServer.exe is still alive (image-name check via tasklist).
async fn scum_alive() -> bool {
    match tokio::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq SCUMServer.exe", "/NH"])
        .output()
        .await
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains("scumserver.exe"),
        Err(_) => false,
    }
}

pub async fn monitor_loop(cfg: Config, state: SharedState, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = sleep(MONITOR_INTERVAL) => {}
            _ = shutdown_rx.changed() => {
                tracing::info!("monitor_loop: shutdown");
                return;
            }
        }

        let (pid, was_running) = {
            let s = state.lock().unwrap();
            (s.pid, s.running)
        };

        if let Some(p) = pid {
            if !is_alive(p) {
                tracing::warn!("pid {} died", p);
                {
                    let mut s = state.lock().unwrap();
                    s.running = false;
                    s.pid = None;
                    s.start_time = None;
                }

                if was_running && cfg.auto_restart {
                    tracing::info!("auto-restart in {}s", cfg.restart_delay_secs);
                    tokio::select! {
                        _ = sleep(Duration::from_secs(cfg.restart_delay_secs)) => {}
                        _ = shutdown_rx.changed() => return,
                    }
                    match start_server(&cfg, Arc::clone(&state)).await {
                        Ok(new_pid) => {
                            let mut s = state.lock().unwrap();
                            s.restarts += 1;
                            tracing::info!("restarted, new pid={}", new_pid);
                        }
                        Err(e) => tracing::error!("restart failed: {}", e),
                    }
                }
            }
        }
    }
}

pub fn tail_log(cfg: &Config, lines: usize) -> Vec<String> {
    let exe_path = PathBuf::from(&cfg.scum_server_exe);
    let log_path = exe_path
        .parent()
        .unwrap_or(&exe_path)
        .join("UE4SS")
        .join("UE4SS.log");

    let Ok(file) = std::fs::File::open(&log_path) else {
        return vec![format!("log not found: {}", log_path.display())];
    };

    let reader = BufReader::new(file);
    let all: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let skip = all.len().saturating_sub(lines);
    all[skip..].to_vec()
}

// ─── internals ───────────────────────────────────────────────────────────────

// Kill all SCUMServer.exe and CONFIRM they're dead before returning, retrying a stubborn/orphaned
// process. @inv: a new SCUM must never launch while an old one still holds its commit (the OVH
// wedge). @brk: if a process survives all attempts (kernel-stuck), we log loudly — that orphan
// would overcommit until a box reboot.
async fn kill_existing() -> Result<()> {
    for attempt in 1..=8 {
        tokio::process::Command::new("taskkill")
            .args(["/F", "/IM", "SCUMServer.exe"])
            .output()
            .await
            .ok();
        sleep(Duration::from_millis(700)).await;
        if !scum_alive().await {
            if attempt > 1 { tracing::info!("SCUMServer.exe confirmed dead after {} attempts", attempt); }
            return Ok(());
        }
        tracing::warn!("SCUMServer.exe still alive after kill attempt {} — retrying", attempt);
    }
    tracing::error!("SCUMServer.exe survived 8 kill attempts (possible kernel-stuck orphan) — \
                     a new launch could overcommit; needs a box reboot");
    Ok(())
}

async fn launch_and_inject(cfg: &Config) -> Result<u32> {
    // Re-read args from service.json so a staged BattlEye toggle (or any arg edit)
    // applies on this launch without a full service restart. @dep: Config::current_args.
    let cmdline = format!(
        "\"{}\" {}",
        cfg.scum_server_exe,
        cfg.current_args().join(" ")
    );
    tracing::info!("launching: {}", cmdline);

    let mut cmdline_buf: Vec<u8> = cmdline.into_bytes();
    cmdline_buf.push(0);

    let mut si: STARTUPINFOA = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOA>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    // @inv: CREATE_SUSPENDED lets us inject before game code executes
    unsafe {
        CreateProcessA(
            None,
            PSTR(cmdline_buf.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_SUSPENDED,
            None,
            None,
            &si,
            &mut pi,
        )
        .context("CreateProcessA")?;
    }

    let pid = pi.dwProcessId;

    for dll in &cfg.inject_dlls {
        if let Err(e) = inject_dll(pid, dll) {
            tracing::error!("inject {} failed: {}", dll, e);
        }
    }

    unsafe {
        ResumeThread(pi.hThread);
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }

    Ok(pid)
}

fn is_alive(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_ALL_ACCESS, false, pid) else {
            return false;
        };
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(h, &mut code);
        let _ = CloseHandle(h);
        ok.is_ok() && code == 259 // STILL_ACTIVE
    }
}

fn write_pipe_discovery(pid: u32) -> Result<()> {
    let appdata = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".into());
    let dir = PathBuf::from(appdata).join("TurdMOD").join("engine");
    std::fs::create_dir_all(&dir).ok();
    let pipe_txt = dir.join("pipe.txt");
    std::fs::write(&pipe_txt, format!(r"\\.\pipe\turdmod-engine-{}", pid))
        .context("write pipe.txt")?;
    tracing::info!("pipe discovery: {}", pipe_txt.display());
    Ok(())
}
