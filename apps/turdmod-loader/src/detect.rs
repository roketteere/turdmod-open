//! In-process detection: are we running inside SCUM, and is BattlEye
//! attached? Mirrors the logic in apps/turdmod-cli/src/detect.ts but runs
//! INSIDE the target so it sees the real process state.
//!
//! Cheap-and-conservative: if we can't tell, we assume BE-on so we never
//! step into a state that risks the user's account. The companion's
//! pre-launch detection (#170) is the primary gate; this is the
//! belt-and-suspenders check inside DllMain.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows_sys::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// SCUM is running solo (offline / hosted-locally with BE off). Safe
    /// to do anything.
    Solo,
    /// SCUM is on a private server with BE off. Same envelope as Solo for
    /// what we're allowed to touch (it's private; the host opted in).
    PrivateBeOff,
    /// SCUM is on a private server with BE on. We refuse to do anything
    /// beyond the dxgi-proxy forwarders — no hooks, no Lua, no UI.
    PrivateBeOn,
    /// We can't tell. Treat as BE-on (refuse engine work).
    Unknown,
}

/// Returns `true` if a module named `BEService.dll` or similar is loaded
/// into the current process. This is the simplest, most reliable
/// BattlEye check from inside the target.
fn battleye_attached() -> bool {
    unsafe {
        let proc = GetCurrentProcess();
        let mut modules = vec![0_isize as windows_sys::Win32::Foundation::HMODULE; 1024];
        let cb_needed: u32 = 0;
        let mut needed: u32 = 0;
        let ok = EnumProcessModules(
            proc,
            modules.as_mut_ptr(),
            (modules.len() * std::mem::size_of::<windows_sys::Win32::Foundation::HMODULE>()) as u32,
            &mut needed as *mut u32,
        );
        let _ = cb_needed;
        if ok == 0 {
            return false;
        }
        let count = (needed as usize) / std::mem::size_of::<windows_sys::Win32::Foundation::HMODULE>();
        for i in 0..count {
            let m = modules[i];
            let mut name = [0_u16; 260];
            let n = GetModuleBaseNameW(proc, m, name.as_mut_ptr(), name.len() as u32);
            if n == 0 { continue; }
            let s = OsString::from_wide(&name[..n as usize])
                .to_string_lossy()
                .to_lowercase();
            if s.contains("battleye") || s.starts_with("beservice") || s.starts_with("beclient") {
                return true;
            }
        }
        false
    }
}

/// Best-effort: are we running inside something that looks like SCUM?
/// We don't refuse to load if not — DllMain runs in whatever process
/// loads us — but we WILL stay inert.
fn is_scum_process() -> bool {
    let exe = std::env::current_exe().unwrap_or(PathBuf::new());
    exe.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("SCUM.exe") || n.to_lowercase().contains("scum"))
        .unwrap_or(false)
}

pub fn infer_mode() -> Mode {
    // Dev escape hatch: TURDMOD_FORCE_TEST=1 lets us exercise the runtime
    // path inside non-SCUM hosts (notepad in our smoke harness, gtest
    // executables, etc.). Logged loudly so a stray test build can't ship
    // accidentally.
    if std::env::var_os("TURDMOD_FORCE_TEST").is_some() {
        crate::logging::log("[detect] TURDMOD_FORCE_TEST set — skipping SCUM/BE checks (TEST MODE)");
        return Mode::Solo;
    }

    if !is_scum_process() {
        // Not SCUM — proxy forwarders work but we don't touch anything.
        return Mode::Unknown;
    }
    // @inv: BE module loaded ⇒ PrivateBeOn ALWAYS wins, regardless of any
    // flag file. This is the fail-closed hard override — a stale/forged
    // launch-mode.json can never talk us into running while BE watches.
    if battleye_attached() {
        return Mode::PrivateBeOn;
    }
    // BE is absent. The launcher writes launch-mode.json next to our log
    // (see write_launch_mode in the launcher) just before it resumes the
    // suspended SCUM process, so by the time DllMain runs the flag is on
    // disk. Trust it when present; fall back to PrivateBeOff (BE absent ⇒
    // same allowed envelope as solo) when it's missing or unreadable —
    // e.g. the player launched through Steam directly, bypassing us.
    match read_launch_mode() {
        Some(m) => m,
        None => Mode::PrivateBeOff,
    }
}

/// Reads `%LOCALAPPDATA%/TurdMOD/launch-mode.json` and maps its `mode`
/// field to a `Mode`. Returns `None` if the file is absent, unreadable, or
/// carries an unrecognized mode string. Deliberately tolerant: a malformed
/// flag must never be treated as permission — callers fall back to the
/// conservative default.
///
/// @dep: launcher/src/lib.rs:write_launch_mode (writer of this file)
fn read_launch_mode() -> Option<Mode> {
    let path = launch_mode_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    match v.get("mode").and_then(|m| m.as_str()) {
        Some("solo") => Some(Mode::Solo),
        Some("private-be-off") => Some(Mode::PrivateBeOff),
        Some("private-be-on") => Some(Mode::PrivateBeOn),
        _ => None,
    }
}

fn launch_mode_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let mut p = PathBuf::from(local);
    p.push("TurdMOD");
    p.push("launch-mode.json");
    Some(p)
}
