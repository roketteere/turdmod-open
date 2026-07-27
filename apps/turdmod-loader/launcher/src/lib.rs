//! Shared launch + injection core for the TurdMOD client launcher.
//!
//! Both the CLI binary (`src/main.rs`) and the desktop launcher's Tauri
//! backend (`apps/turdmod-launcher`) call into this so the unsafe
//! spawn-suspended + CreateRemoteThread injection lives in exactly one
//! place. @inv: keep this crate the SINGLE owner of the injection code —
//! a second copy is how the two paths drift and one ships a bug the other
//! already fixed.
//!
//! Launch flow (`launch`):
//!   1. Resolve SCUM.exe + loader DLL + extra DLLs.
//!   2. Safety gate: refuse if BattlEye is present in the install, and —
//!      when a server target is supplied — refuse unless it's a known
//!      BE-off ("our") server. This is the hard "our-servers-only" rule.
//!   3. Write launch-mode.json so the loader's detect.rs knows the mode
//!      before its DllMain runs (the handshake detect.rs reads).
//!   4. CreateProcess SUSPENDED → inject loader (+extras) → ResumeThread.
//!
//! @dep: ../../turdmod-loader/src/detect.rs:read_launch_mode (reader of
//! launch-mode.json) — the `mode` strings here MUST match the ones it
//! recognizes: "solo" | "private-be-off" | "private-be-on".

use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, CreateRemoteThread, GetExitCodeThread, ResumeThread, TerminateProcess,
    WaitForSingleObject, CREATE_SUSPENDED, INFINITE, PROCESS_INFORMATION, STARTUPINFOW,
};

/// The mode written into launch-mode.json. String forms MUST stay in sync
/// with detect.rs::read_launch_mode in the loader crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Solo,
    PrivateBeOff,
    PrivateBeOn,
}

impl LaunchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LaunchMode::Solo => "solo",
            LaunchMode::PrivateBeOff => "private-be-off",
            LaunchMode::PrivateBeOn => "private-be-on",
        }
    }
}

/// A server the launcher can point the client at. `battle_eye == false` is
/// the gate: only BE-off ("our") servers are ever a valid launch target.
#[derive(Debug, Clone)]
pub struct ServerTarget {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub battle_eye: bool,
}

/// Everything `launch` needs. Mirrors the CLI flags so main.rs is a thin
/// translation layer.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub scum_exe: PathBuf,
    pub dll: PathBuf,
    pub extra_dlls: Vec<PathBuf>,
    /// Raw args forwarded to SCUM.exe (e.g. `+connect ip:port`). When
    /// `server` is set, `launch` appends the connect arg itself.
    pub game_args: Vec<String>,
    /// Selected server. When `Some`, the allowlist gate applies and the
    /// `+connect` arg is built from it.
    pub server: Option<ServerTarget>,
    /// Skip the BattlEye-present static check. Dev-only escape hatch.
    pub skip_safety_check: bool,
}

// ---------------------------------------------------------------------------
// Paths — single source of truth for the on-disk contract with the loader.
// ---------------------------------------------------------------------------

/// `%LOCALAPPDATA%/TurdMOD`. Returns `None` only if LOCALAPPDATA is unset
/// (never on a real Windows session).
pub fn turdmod_data_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let mut p = PathBuf::from(local);
    p.push("TurdMOD");
    Some(p)
}

/// `%LOCALAPPDATA%/TurdMOD/mods` — where the loader discovers mod folders.
pub fn mods_dir() -> Option<PathBuf> {
    turdmod_data_dir().map(|mut p| {
        p.push("mods");
        p
    })
}

fn launch_mode_path() -> Option<PathBuf> {
    turdmod_data_dir().map(|mut p| {
        p.push("launch-mode.json");
        p
    })
}

/// Writes `launch-mode.json` so the loader's detect.rs can read the mode at
/// DllMain time. Called BEFORE ResumeThread so the flag is on disk before
/// the loader's init thread runs. The server block is informational; the
/// loader only reads `mode`.
pub fn write_launch_mode(mode: LaunchMode, server: Option<&ServerTarget>) -> Result<(), String> {
    let dir = turdmod_data_dir().ok_or("LOCALAPPDATA not set")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create data dir: {e}"))?;
    let path = launch_mode_path().ok_or("LOCALAPPDATA not set")?;

    // Hand-built JSON to avoid a serde_json dependency in the launcher
    // crate. The shape is tiny and stable.
    let server_json = match server {
        Some(s) => format!(
            "{{\"id\":{},\"name\":{},\"ip\":{},\"port\":{}}}",
            json_str(&s.id),
            json_str(&s.name),
            json_str(&s.ip),
            s.port
        ),
        None => "null".to_string(),
    };
    let body = format!(
        "{{\"mode\":{},\"server\":{}}}\n",
        json_str(mode.as_str()),
        server_json
    );
    std::fs::write(&path, body).map_err(|e| format!("write launch-mode.json: {e}"))?;
    Ok(())
}

/// Clears launch-mode.json. Useful so a stale "private-be-off" flag from a
/// prior modded session can't carry into a later Steam-vanilla launch that
/// bypassed us. (detect.rs already hard-overrides to PrivateBeOn whenever a
/// BE module is loaded, so this is defense-in-depth, not the primary gate.)
pub fn clear_launch_mode() -> Result<(), String> {
    if let Some(path) = launch_mode_path() {
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| format!("remove launch-mode.json: {e}"))?;
        }
    }
    Ok(())
}

/// Minimal JSON string escaper for the few fields we emit.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Resolution helpers.
// ---------------------------------------------------------------------------

pub fn resolve_scum(arg: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = arg {
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        return Err(format!("--scum not found: {}", p.display()));
    }
    if let Ok(env) = std::env::var("SCUM_EXE") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
    }
    // @inv: modded launcher prefers the ISOLATED modded client copy over the
    // Steam-managed vanilla install. NEVER mod the Steam copy in-place —
    // hijacking its pak slots corrupts vanilla + forces a multi-GB Steam
    // re-validate. Vanilla = Steam "Play"; modded = this launcher.
    // See reference_modded_client_isolated_copy.
    //
    // @dep: TurdMOD Setup writes %LOCALAPPDATA%\TurdMOD\client.json when it
    //   builds the modded copy, so whichever drive the user picked is honoured.
    //   The hardcoded list below is a last-resort fallback — it used to be the
    //   ONLY source, which meant a modded copy anywhere but F: was silently
    //   ignored and the launcher fell through to the Steam install.
    if let Some(p) = scum_from_client_config() {
        if p.is_file() {
            return Ok(p);
        }
    }
    let candidates = [
        r"F:\SCUM-Modded\SCUM\Binaries\Win64\SCUM.exe",
        r"C:\Program Files (x86)\Steam\steamapps\common\SCUM\SCUM\Binaries\Win64\SCUM.exe",
        r"C:\Program Files\Steam\steamapps\common\SCUM\SCUM\Binaries\Win64\SCUM.exe",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err("could not locate SCUM.exe — pass --scum <path> or set SCUM_EXE".into())
}

/// The modded-client location TurdMOD Setup recorded, if any.
///
/// Hand-rolled rather than pulling serde into the launcher core for two string
/// fields. @dep: key name must stay in sync with
/// apps/turdmod-setup/src-tauri/src/client.rs::write_client_config.
fn scum_from_client_config() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let raw = std::fs::read_to_string(PathBuf::from(local).join("TurdMOD").join("client.json")).ok()?;
    parse_scum_exe(&raw).map(PathBuf::from)
}

fn parse_scum_exe(json: &str) -> Option<String> {
    let key = "\"scum_exe\"";
    let after = json.find(key).map(|i| &json[i + key.len()..])?;
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    let body = after.strip_prefix('"')?;
    let end = body.find('"')?;
    let val = body[..end].replace("\\\\", "\\");
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

pub fn resolve_dll(arg: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = arg {
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        return Err(format!("--dll not found: {}", p.display()));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("launcher has no parent dir")?;
    for name in ["turdmod_loader.dll", "turdmod-loader.dll"] {
        let p = dir.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    // Dev fallback: look one dir up alongside the launcher.
    let p = dir.join("..").join("turdmod_loader.dll");
    if p.is_file() {
        return Ok(p.canonicalize().unwrap_or(p));
    }
    Err("could not locate turdmod_loader.dll — pass --dll <path>".into())
}

/// SCUM ships its BattlEye binaries under `<install>/BattlEye/`. Their
/// presence does NOT mean BE is active — every vanilla install has this
/// folder, and our launches spawn SCUM.exe directly (no BE bootstrap), so an
/// on-disk BattlEye/ folder is inert for us. Kept for diagnostics only; the
/// real pre-flight is `battleye_process_active` below. @dep: not used as a
/// gate anymore (Joel's option-b decision 2026-05-31).
pub fn scum_install_has_battleye(scum_exe: &Path) -> bool {
    if let Some(root) = scum_exe.ancestors().nth(3) {
        return root.join("BattlEye").exists();
    }
    false
}

/// True if a BattlEye process is ACTUALLY RUNNING on the system right now —
/// the correct "is BE watching" signal (vs. folder-on-disk, which is always
/// present in a vanilla install). We refuse to spawn only when BE is live,
/// because joining a BE server with our BE-off client = instant ban.
///
/// Implemented via `tasklist` (per Fred's spec): a once-per-launch pre-flight,
/// not a hot path, so a tiny silent process spawn is fine — and it avoids the
/// windows-sys ToolHelp binding entirely. CREATE_NO_WINDOW keeps it invisible.
/// Matches the known BE process names case-insensitively: `BEService.exe`,
/// `BEClient_x64.exe`/`BEClient.exe`, any `BattlEye*`.
///
/// @inv: this is hygiene; the AUTHORITATIVE runtime gate is the loader's
/// in-process `detect::battleye_attached()` at DllMain. If tasklist can't run
/// we fail OPEN (return false) and rely on that in-process gate.
pub fn battleye_process_active() -> bool {
    use std::os::windows::process::CommandExt;
    const NEEDLES: [&str; 3] = ["beservice", "beclient", "battleye"];
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
            NEEDLES.iter().any(|n| text.contains(n))
        }
        Err(e) => {
            // Fail OPEN, but LOUDLY — a field failure must be visible. The real
            // ban protection is the direct CreateProcessW spawn (no BE
            // bootstrap), not this hygiene check, so allowing here is safe; the
            // loader's in-process detect::battleye_attached() is the net.
            eprintln!(
                "[turdmod-launcher] WARN: tasklist failed ({e}) — BE pre-flight \
                 skipped (fail-open). Relying on direct-spawn + in-process gate."
            );
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Launch + inject.
// ---------------------------------------------------------------------------

/// The "our-servers-only" allowlist gate: refuse any server flagged
/// BattlEye-on. Extracted as a pure function so the safety boundary is
/// unit-testable without spawning a process or writing files.
///
/// @inv: this is the hard safety boundary — joining a BE server with the
/// BE-off modded client is an instant ban. `None` (solo / no target) is
/// allowed; only an explicit `battle_eye == true` target is refused.
pub fn assert_server_allowed(server: Option<&ServerTarget>) -> Result<(), String> {
    if let Some(server) = server {
        if server.battle_eye {
            return Err(format!(
                "refuse: '{}' is a BattlEye server. The modded client must never join \
                 an official/BE server — instant ban. Pick one of our BE-off servers.",
                server.name
            ));
        }
    }
    Ok(())
}

/// Spawn SCUM suspended, inject the loader (+extras), resume. Returns the
/// new process id on success. `log` receives human-readable progress lines
/// (the CLI prints them; the Tauri backend collects them for the UI).
///
/// @inv: launch-mode.json is written BEFORE ResumeThread — the loader reads
/// it in its init thread, which only starts running after resume.
pub fn launch(opts: &LaunchOptions, log: &mut dyn FnMut(&str)) -> Result<u32, String> {
    // --- allowlist gate: only BE-off ("our") servers are a valid target.
    assert_server_allowed(opts.server.as_ref())?;

    // --- pre-flight: refuse only if BattlEye is ACTUALLY RUNNING. Folder-on-
    // disk is NOT a gate (every vanilla install has it; we spawn SCUM.exe
    // directly so the BE bootstrap never runs). Joel's option-b decision.
    if !opts.skip_safety_check && battleye_process_active() {
        return Err(
            "refuse: BattlEye is running on this system — we don't inject while BE is \
             live (instant ban). Close BattlEye / the vanilla BE-launched SCUM first, \
             then launch through TurdMOD."
                .into(),
        );
    }

    // Validate every extra DLL up-front so we don't spawn SCUM then fail
    // mid-injection.
    let mut extras: Vec<PathBuf> = Vec::with_capacity(opts.extra_dlls.len());
    for ed in &opts.extra_dlls {
        if !ed.is_file() {
            return Err(format!(
                "--extra-dll path does not exist or is not a file: {}",
                ed.display()
            ));
        }
        extras.push(ed.canonicalize().unwrap_or_else(|_| ed.clone()));
    }

    log(&format!("scum  : {}", opts.scum_exe.display()));
    log(&format!("loader: {}", opts.dll.display()));
    for (i, ed) in extras.iter().enumerate() {
        log(&format!("extra-dll[{i}]: {}", ed.display()));
    }

    // Build the final game-arg list. When a server is selected, append its
    // connect target so the player lands directly on an allowlisted server.
    // UE4 auto-connect: the server URL is the FIRST positional arg (no "+").
    // `SCUM.exe <ip>:<port>` makes the engine Browse() straight to the server
    // at startup instead of loading the MainMenu frontend map — so the player
    // goes launcher → server, never seeing SCUM's main menu. (The old
    // "+connect" form is a Source-engine convention; UE4 ignored it, which is
    // why the menu still showed — confirmed in SCUM.log: MainMenu loaded then
    // The_Island.) URL must come BEFORE any other flags.
    let mut game_args = Vec::with_capacity(opts.game_args.len() + 1);
    if let Some(server) = &opts.server {
        game_args.push(format!("{}:{}", server.ip, server.port));
        log(&format!("connect (UE4 url): {}:{} ({})", server.ip, server.port, server.name));
    }
    game_args.extend(opts.game_args.iter().cloned());

    // Write the mode flag the loader reads at DllMain. BE-off server (or no
    // server / solo) ⇒ private-be-off envelope.
    let mode = if opts.server.is_some() {
        LaunchMode::PrivateBeOff
    } else {
        LaunchMode::Solo
    };
    write_launch_mode(mode, opts.server.as_ref())?;
    log(&format!("launch-mode.json written: {}", mode.as_str()));

    // --- CreateProcess suspended.
    let mut cmdline = build_cmdline(&opts.scum_exe, &game_args);
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let exe_w = wide(opts.scum_exe.as_os_str());
    let ok = unsafe {
        CreateProcessW(
            exe_w.as_ptr(),
            cmdline.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            CREATE_SUSPENDED,
            ptr::null_mut(),
            ptr::null(),
            &mut si as *mut _,
            &mut pi as *mut _,
        )
    };
    if ok == 0 {
        return Err(format!("CreateProcessW failed: GetLastError={}", unsafe {
            GetLastError()
        }));
    }
    log(&format!("spawned SCUM (suspended) pid={}", pi.dwProcessId));

    // Inject the primary loader first; terminate the suspended process if
    // it fails (don't resume a half-injected game).
    if let Err(e) = inject_dll(pi.hProcess, &opts.dll, log) {
        unsafe {
            TerminateProcess(pi.hProcess, 1);
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
        }
        return Err(e);
    }
    // Extras are best-effort: a failure is a warning, not fatal.
    for (i, ed) in extras.iter().enumerate() {
        if let Err(e) = inject_dll(pi.hProcess, ed, log) {
            log(&format!("warning: extra-dll[{i}] inject failed: {e}"));
        } else {
            log(&format!("injected extra-dll[{i}] OK"));
        }
    }

    let resume = unsafe { ResumeThread(pi.hThread) };
    if resume == u32::MAX {
        log(&format!("warning: ResumeThread failed (GetLastError={})", unsafe {
            GetLastError()
        }));
    }
    let pid = pi.dwProcessId;
    unsafe {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
    Ok(pid)
}

pub fn build_cmdline(exe: &Path, extra: &[String]) -> Vec<u16> {
    let mut s = String::new();
    s.push('"');
    s.push_str(&exe.display().to_string());
    s.push('"');
    for a in extra {
        s.push(' ');
        // +connect args carry a space ("+connect ip:port") and must NOT be
        // wrapped as a single quoted token, or SCUM sees one malformed arg.
        // Only quote a token that contains a space AND isn't a +command.
        if a.contains(' ') && !a.starts_with('+') {
            s.push('"');
            s.push_str(a);
            s.push('"');
        } else {
            s.push_str(a);
        }
    }
    let mut w = wide(OsStr::new(&s));
    w.push(0);
    w
}

pub fn wide(s: &OsStr) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_wide().collect();
    v.push(0);
    v
}

/// CreateRemoteThread(LoadLibraryW) injection into an already-open process
/// handle. The launcher holds a suspended SCUM handle; this maps the DLL
/// before the game's first instruction runs.
pub fn inject_dll(proc: HANDLE, dll_path: &Path, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let dll_w = wide(dll_path.as_os_str());
    let dll_bytes = dll_w.len() * 2; // wide chars

    // ASLR keeps kernel32 at the same base across processes on one boot, so
    // LoadLibraryW resolved here is valid in the target.
    let kernel32 = unsafe { GetModuleHandleW(wide(OsStr::new("kernel32.dll")).as_ptr()) };
    if kernel32.is_null() {
        return Err("could not resolve kernel32.dll handle".into());
    }
    let load_lib = unsafe { GetProcAddress(kernel32, b"LoadLibraryW\0".as_ptr()) };
    if load_lib.is_none() {
        return Err("could not resolve LoadLibraryW".into());
    }

    let remote_buf = unsafe {
        VirtualAllocEx(
            proc,
            ptr::null(),
            dll_bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote_buf.is_null() {
        return Err(format!("VirtualAllocEx failed: GetLastError={}", unsafe {
            GetLastError()
        }));
    }

    let mut written: usize = 0;
    let ok = unsafe {
        WriteProcessMemory(
            proc,
            remote_buf,
            dll_w.as_ptr() as *const c_void,
            dll_bytes,
            &mut written as *mut usize,
        )
    };
    if ok == 0 {
        unsafe { VirtualFreeEx(proc, remote_buf, 0, MEM_RELEASE) };
        return Err(format!("WriteProcessMemory failed: GetLastError={}", unsafe {
            GetLastError()
        }));
    }

    let thread = unsafe {
        CreateRemoteThread(
            proc,
            ptr::null(),
            0,
            Some(std::mem::transmute::<_, unsafe extern "system" fn(*mut c_void) -> u32>(load_lib)),
            remote_buf,
            0,
            ptr::null_mut(),
        )
    };
    if thread.is_null() {
        unsafe { VirtualFreeEx(proc, remote_buf, 0, MEM_RELEASE) };
        return Err(format!("CreateRemoteThread failed: GetLastError={}", unsafe {
            GetLastError()
        }));
    }

    let wait = unsafe { WaitForSingleObject(thread, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        unsafe {
            CloseHandle(thread);
            VirtualFreeEx(proc, remote_buf, 0, MEM_RELEASE);
        }
        return Err(format!("WaitForSingleObject returned {wait}"));
    }

    let mut exit: u32 = 0;
    let _ = unsafe { GetExitCodeThread(thread, &mut exit as *mut u32) };
    log(&format!(
        "injected: LoadLibraryW returned 0x{:08x} ({})",
        exit,
        if exit == 0 { "FAILED — DLL did not load" } else { "ok" }
    ));

    unsafe {
        CloseHandle(thread);
        VirtualFreeEx(proc, remote_buf, 0, MEM_RELEASE);
    }

    if exit == 0 {
        return Err("LoadLibraryW returned NULL — DLL load failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(battle_eye: bool) -> ServerTarget {
        ServerTarget {
            id: "test".into(),
            name: "Test Server".into(),
            ip: "203.0.113.1".into(),
            port: 7042,
            battle_eye,
        }
    }

    // The safety-critical branch: a BattlEye-on target MUST be refused. This
    // is the "our-servers-only" boundary the README calls the whole ballgame.
    #[test]
    fn allowlist_refuses_battleye_server() {
        let r = assert_server_allowed(Some(&server(true)));
        assert!(r.is_err(), "BE-on server must be refused");
        let msg = r.unwrap_err();
        assert!(
            msg.contains("BattlEye") && msg.starts_with("refuse"),
            "refusal must name BattlEye and start with 'refuse', got: {msg}"
        );
        // The offending server's name is surfaced so the UI/CLI can show it.
        assert!(msg.contains("Test Server"), "refusal should name the server");
    }

    // Guard against a gate that refuses everything: a BE-off ("our") server
    // must pass.
    #[test]
    fn allowlist_allows_be_off_server() {
        assert!(assert_server_allowed(Some(&server(false))).is_ok());
    }

    // Solo / no selected server (None) is a valid launch — the gate only
    // rejects an explicit BE-on target, never absence of one.
    #[test]
    fn allowlist_allows_none() {
        assert!(assert_server_allowed(None).is_ok());
    }

    /// @dep: apps/turdmod-setup/src-tauri/src/client.rs::write_client_config
    /// writes this file. If the key name drifts, the launcher silently falls
    /// back to the hardcoded paths and ignores the user's modded copy.
    #[test]
    fn reads_the_modded_client_path_setup_wrote() {
        let json = r#"{
  "modded_root": "D:\SCUM-Modded",
  "scum_exe": "D:\SCUM-Modded\SCUM\Binaries\Win64\SCUM.exe"
}"#;
        assert_eq!(
            parse_scum_exe(json).as_deref(),
            Some(r"D:\SCUM-Modded\SCUM\Binaries\Win64\SCUM.exe")
        );
    }

    #[test]
    fn a_missing_or_broken_client_config_is_ignored_not_fatal() {
        assert!(parse_scum_exe("{}").is_none());
        assert!(parse_scum_exe("not json at all").is_none());
        assert!(parse_scum_exe(r#"{"scum_exe": ""}"#).is_none());
    }
}
