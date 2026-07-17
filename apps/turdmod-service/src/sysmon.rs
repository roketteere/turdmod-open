//! System monitor — box + key-process CPU/mem sampled on a 5s interval into a global snapshot that
//! `/monitor/system` serves. Feeds turdmod-manager's load view + the optimization hints (e.g. flag
//! the SCUM memory-leak climb between restarts). @dep [[server]] route handle_monitor_system.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Default, Clone, Serialize)]
pub struct SystemStats {
    pub cpu_pct: f32,        // box-wide CPU
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub svc_mem_mb: u64,     // this service process
    pub scum_cpu_pct: f32,   // GameServer.exe
    pub scum_mem_mb: u64,
    pub scum_running: bool,
    // Commit charge vs the commit LIMIT (RAM + pagefile). @ctx 2026-06-11: this — NOT physical
    // mem — is what wedged OVH: SCUM commits ~17.6GB, the pagefile was a fixed 2GB so the limit was
    // only ~34GB, and a restart's old+new SCUM overlap blew past it ("paging file too small"). Watch
    // committed_mb climbing toward commit_limit_mb to catch it BEFORE the box locks up.
    pub committed_mb: u64,
    pub commit_limit_mb: u64,
    pub pagefile_total_mb: u64,
    pub updated_secs: u64,
}

// Commit charge + limit via GlobalMemoryStatusEx: ullTotalPageFile = commit LIMIT (RAM+pagefile),
// ullAvailPageFile = free commit. committed = limit - avail.
fn commit_mb() -> (u64, u64) {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut ms = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if GlobalMemoryStatusEx(&mut ms).is_ok() {
            let limit = ms.ullTotalPageFile / 1_048_576;
            let avail = ms.ullAvailPageFile / 1_048_576;
            (limit.saturating_sub(avail), limit)
        } else { (0, 0) }
    }
}

static STATS: OnceLock<Mutex<SystemStats>> = OnceLock::new();
fn cell() -> &'static Mutex<SystemStats> { STATS.get_or_init(|| Mutex::new(SystemStats::default())) }
pub fn snapshot() -> SystemStats { cell().lock().map(|s| s.clone()).unwrap_or_default() }

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn spawn_sysmon() {
    tokio::spawn(async move {
        let mut sys = System::new();
        let my_pid = Pid::from_u32(std::process::id());
        // prime CPU counters (first reading needs a delta window)
        sys.refresh_cpu_all();
        tokio::time::sleep(Duration::from_millis(300)).await;
        loop {
            sys.refresh_cpu_all();
            sys.refresh_memory();
            sys.refresh_processes(ProcessesToUpdate::All, true);

            let cpu = sys.global_cpu_usage();
            let mem_used = sys.used_memory() / 1_048_576;
            let mem_total = sys.total_memory() / 1_048_576;
            let svc_mem = sys.process(my_pid).map(|p| p.memory() / 1_048_576).unwrap_or(0);

            let mut scum_cpu = 0.0f32;
            let mut scum_mem = 0u64;
            let mut scum_run = false;
            for p in sys.processes().values() {
                if p.name().to_string_lossy().eq_ignore_ascii_case("scumserver.exe") {
                    scum_cpu = p.cpu_usage();
                    scum_mem = p.memory() / 1_048_576;
                    scum_run = true;
                    break;
                }
            }
            let (committed, commit_limit) = commit_mb();
            let pagefile_total = sys.total_swap() / 1_048_576;
            if let Ok(mut s) = cell().lock() {
                *s = SystemStats {
                    cpu_pct: cpu, mem_used_mb: mem_used, mem_total_mb: mem_total,
                    svc_mem_mb: svc_mem, scum_cpu_pct: scum_cpu, scum_mem_mb: scum_mem,
                    scum_running: scum_run,
                    committed_mb: committed, commit_limit_mb: commit_limit,
                    pagefile_total_mb: pagefile_total,
                    updated_secs: now_secs(),
                };
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}
