//! Runtime DLL injection — Manager-side port of the
//! `paliaplay/scripts/inject-dll.py` injector, used to inject
//! `Dumper-7.dll` into a running game / server process so Phase B
//! (server) and Phase B-Client can advance without dropping to a
//! shell.
//!
//! Flow (same as inject-dll.py + apps/turdmod-loader/launcher's
//! `inject_dll` helper):
//!   1. Find PID by name (CreateToolhelp32Snapshot + Process32First/Next).
//!   2. OpenProcess(PROCESS_ALL_ACCESS).
//!   3. VirtualAllocEx for the DLL path string.
//!   4. WriteProcessMemory the path.
//!   5. GetProcAddress(GetModuleHandleA("kernel32.dll"), "LoadLibraryA").
//!   6. CreateRemoteThread targeting LoadLibraryA with path pointer.
//!   7. WaitForSingleObject + GetExitCodeThread (non-zero exit = HMODULE
//!      truncated to 32-bit = success).
//!   8. VirtualFreeEx + CloseHandle.
//!
//! All Win32 APIs accessed via `windows-sys` — same crate the launcher
//! uses. No external Python dependency.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use serde::Serialize;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, INVALID_HANDLE_VALUE, MAX_PATH, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, INFINITE,
    PROCESS_ALL_ACCESS,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectResult {
    pub pid: u32,
    pub process_name: String,
    pub dll_path: String,
    pub remote_thread_exit_code: u32,
}

/// Find the first PID with `image_name` in the running snapshot.
///
/// `image_name` matches the `szExeFile` field from
/// PROCESSENTRY32 (the bare exe filename, e.g. `SCUM.exe`).
/// Case-insensitive — Windows process names are typically capitalised
/// the same as on disk but we don't want to fail on a mismatch.
pub fn find_pid_by_name(image_name: &str) -> Option<u32> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;
        let mut ok = Process32First(snap, &mut entry);
        let needle = image_name.to_ascii_lowercase();
        let mut found: Option<u32> = None;
        while ok != 0 {
            // szExeFile is [u8; MAX_PATH] (ANSI). Trim to first NUL.
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            // `szExeFile` is [CHAR; MAX_PATH] where CHAR = i8 on
            // windows-sys. Reinterpret as &[u8] so String::from_utf8_lossy
            // accepts it. The bytes are the same on the wire — only the
            // Rust type signs differ.
            let name_bytes: &[u8] =
                std::slice::from_raw_parts(entry.szExeFile.as_ptr() as *const u8, len);
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            if name.to_ascii_lowercase() == needle {
                found = Some(entry.th32ProcessID);
                break;
            }
            ok = Process32Next(snap, &mut entry);
        }
        CloseHandle(snap);
        found
    }
}

/// Try several candidate process names in order, return the first
/// running PID + the matching name. Useful when the user might be
/// running either `SCUM.exe` or `SCUM-Win64-Shipping.exe`.
pub fn find_first_running(candidates: &[&str]) -> Option<(u32, String)> {
    for c in candidates {
        if let Some(pid) = find_pid_by_name(c) {
            return Some((pid, (*c).to_string()));
        }
    }
    None
}

/// Inject a DLL into the given PID via CreateRemoteThread(LoadLibraryA).
///
/// Returns the remote thread's exit code on success (= LoadLibraryA's
/// return value, truncated to 32 bits; non-zero means HMODULE
/// returned which is the success signal).
pub fn inject_dll_into_pid(pid: u32, dll_path: &Path) -> Result<InjectResult, String> {
    // Pre-flight: the DLL must exist.
    if !dll_path.is_file() {
        return Err(format!(
            "DLL not found at {}",
            dll_path.display()
        ));
    }
    let dll_str = dll_path.to_string_lossy().into_owned();
    let dll_c = CString::new(dll_str.clone())
        .map_err(|_| "DLL path contains a NUL byte".to_string())?;
    let dll_bytes_with_null = dll_c.as_bytes_with_nul();

    unsafe {
        // 1. OpenProcess
        let h_proc = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
        if h_proc.is_null() {
            return Err(format!(
                "OpenProcess(pid={}) failed: GetLastError={}",
                pid,
                GetLastError()
            ));
        }

        // Wrapper to ensure CloseHandle runs no matter how we exit.
        let close = || {
            CloseHandle(h_proc);
        };

        // 2. VirtualAllocEx
        let remote_buf = VirtualAllocEx(
            h_proc,
            ptr::null(),
            dll_bytes_with_null.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote_buf.is_null() {
            let e = GetLastError();
            close();
            return Err(format!("VirtualAllocEx failed: GetLastError={}", e));
        }

        // 3. WriteProcessMemory
        let mut written: usize = 0;
        let ok = WriteProcessMemory(
            h_proc,
            remote_buf,
            dll_bytes_with_null.as_ptr() as *const _,
            dll_bytes_with_null.len(),
            &mut written,
        );
        if ok == 0 || written != dll_bytes_with_null.len() {
            let e = GetLastError();
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            close();
            return Err(format!(
                "WriteProcessMemory failed (written={}/{}): GetLastError={}",
                written,
                dll_bytes_with_null.len(),
                e
            ));
        }

        // 4. Resolve LoadLibraryA address. ASLR keeps this address
        //    consistent across all processes on the same boot, so
        //    OUR LoadLibraryA address is valid in the target.
        let kernel32_name =
            CString::new("kernel32.dll").expect("static string");
        let h_kernel32 = GetModuleHandleA(kernel32_name.as_ptr() as *const u8);
        if h_kernel32.is_null() {
            let e = GetLastError();
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            close();
            return Err(format!(
                "GetModuleHandleA(kernel32.dll) failed: GetLastError={}",
                e
            ));
        }
        let load_library_name =
            CString::new("LoadLibraryA").expect("static string");
        let load_library_addr =
            GetProcAddress(h_kernel32, load_library_name.as_ptr() as *const u8);
        if load_library_addr.is_none() {
            let e = GetLastError();
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            close();
            return Err(format!(
                "GetProcAddress(LoadLibraryA) failed: GetLastError={}",
                e
            ));
        }

        // 5. CreateRemoteThread
        let h_thread = CreateRemoteThread(
            h_proc,
            ptr::null(),
            0,
            Some(std::mem::transmute(load_library_addr)),
            remote_buf,
            0,
            ptr::null_mut(),
        );
        if h_thread.is_null() {
            let e = GetLastError();
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            close();
            return Err(format!(
                "CreateRemoteThread failed: GetLastError={}",
                e
            ));
        }

        // 6. WaitForSingleObject
        let wait = WaitForSingleObject(h_thread, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let e = GetLastError();
            CloseHandle(h_thread);
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            close();
            return Err(format!(
                "WaitForSingleObject returned 0x{:X}; GetLastError={}",
                wait, e
            ));
        }

        // 7. Exit code = HMODULE truncated to 32-bit. Non-zero =
        //    LoadLibraryA returned a real handle = injection worked.
        let mut exit_code: u32 = 0;
        GetExitCodeThread(h_thread, &mut exit_code);

        CloseHandle(h_thread);
        VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
        close();

        if exit_code == 0 {
            return Err(
                "Remote thread exited with 0 — LoadLibraryA returned NULL. \
                 DLL path may be wrong or the target process refused the load."
                    .to_string(),
            );
        }

        Ok(InjectResult {
            pid,
            process_name: String::new(), // caller fills in
            dll_path: dll_str,
            remote_thread_exit_code: exit_code,
        })
    }
}

/// Inject a DLL by image name. Tries the candidate names in order
/// until one is running; reports an error if none are. Helper for the
/// Tauri command layer.
pub fn inject_dll_by_name(
    candidates: &[&str],
    dll_path: &Path,
) -> Result<InjectResult, String> {
    let (pid, name) = find_first_running(candidates).ok_or_else(|| {
        format!(
            "no matching process running. Tried: {}. Launch the target first.",
            candidates.join(", ")
        )
    })?;
    let mut r = inject_dll_into_pid(pid, dll_path)?;
    r.process_name = name;
    Ok(r)
}

// Suppress unused-warning on MAX_PATH (re-exported for future use).
const _: u32 = MAX_PATH;
