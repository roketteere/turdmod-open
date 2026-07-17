//! `turdmod-injector` — runtime DLL injector.
//!
//! Standalone CLI that injects a DLL into a running Windows process by
//! image name (or PID). Designed to be invoked **elevated** by the
//! Manager (via ShellExecuteExW with verb="runas") so it can target
//! processes that require admin — most notably `SCUMServer.exe`.
//!
//! Mirrors the flow in `paliaplay/scripts/inject-dll.py`:
//!   1. Find PID by image name (CreateToolhelp32Snapshot).
//!   2. OpenProcess(PROCESS_ALL_ACCESS).
//!   3. VirtualAllocEx for the DLL path string.
//!   4. WriteProcessMemory the path.
//!   5. GetProcAddress(GetModuleHandleA("kernel32.dll"), "LoadLibraryA").
//!   6. CreateRemoteThread on LoadLibraryA, arg = the remote path.
//!   7. WaitForSingleObject + GetExitCodeThread.
//!   8. VirtualFreeEx + CloseHandle.
//!
//! Exit codes:
//!   0 = success (LoadLibraryA returned a non-zero HMODULE)
//!   1 = bad args
//!   2 = target process not running
//!   3 = OpenProcess failed (likely Access Denied — re-run elevated)
//!   4 = VirtualAllocEx failed
//!   5 = WriteProcessMemory failed
//!   6 = GetProcAddress(LoadLibraryA) failed
//!   7 = CreateRemoteThread failed
//!   8 = LoadLibraryA returned NULL (DLL load refused)
//!
//! Diagnostic output goes to stdout; Manager hides the stdout window
//! when invoking elevated (SW_HIDE) but can still parse the exit code.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use clap::Parser;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
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

#[derive(Parser, Debug)]
#[command(
    about = "turdmod-injector: CreateRemoteThread(LoadLibraryA) DLL injector. Run elevated for elevated targets.",
    version
)]
struct Cli {
    /// Image name of the target process (e.g. SCUMServer.exe).
    /// One of --target or --pid must be set.
    #[arg(long, conflicts_with = "pid")]
    target: Option<String>,

    /// Numeric PID of the target.
    #[arg(long)]
    pid: Option<u32>,

    /// Additional candidate names to try if --target isn't running.
    /// e.g. --target SCUM-Win64-Shipping.exe --fallback SCUM.exe
    #[arg(long)]
    fallback: Vec<String>,

    /// Absolute path to the DLL to inject.
    #[arg(long)]
    dll: PathBuf,
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    if cli.target.is_none() && cli.pid.is_none() {
        eprintln!("error: --target <name> or --pid <pid> is required");
        std::process::exit(1);
    }
    if !cli.dll.is_file() {
        eprintln!("error: --dll not found: {}", cli.dll.display());
        std::process::exit(1);
    }

    // Resolve a target PID.
    let (pid, name): (u32, String) = if let Some(pid) = cli.pid {
        (pid, format!("pid:{}", pid))
    } else {
        let primary = cli.target.clone().unwrap();
        let mut candidates: Vec<String> = vec![primary];
        candidates.extend(cli.fallback.iter().cloned());
        let mut found: Option<(u32, String)> = None;
        for c in &candidates {
            if let Some(pid) = find_pid_by_name(c) {
                found = Some((pid, c.clone()));
                break;
            }
        }
        match found {
            Some(x) => x,
            None => {
                eprintln!(
                    "error: no matching process running. Tried: {}. Launch the target first.",
                    candidates.join(", ")
                );
                std::process::exit(2);
            }
        }
    };

    println!("[inject] target: {} pid={}", name, pid);
    println!("[inject] dll:    {}", cli.dll.display());

    let exit = inject(pid, &cli.dll);
    std::process::exit(exit);
}

fn find_pid_by_name(image_name: &str) -> Option<u32> {
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
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
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

fn inject(pid: u32, dll_path: &std::path::Path) -> i32 {
    let dll_str = dll_path.to_string_lossy().into_owned();
    let dll_c = match CString::new(dll_str) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("error: DLL path contains a NUL byte");
            return 1;
        }
    };
    let dll_bytes = dll_c.as_bytes_with_nul();

    unsafe {
        let h_proc = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
        if h_proc.is_null() {
            eprintln!(
                "error: OpenProcess(pid={}) failed: GetLastError={}",
                pid,
                GetLastError()
            );
            return 3;
        }

        let remote_buf = VirtualAllocEx(
            h_proc,
            ptr::null(),
            dll_bytes.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote_buf.is_null() {
            eprintln!("error: VirtualAllocEx failed: GetLastError={}", GetLastError());
            CloseHandle(h_proc);
            return 4;
        }

        let mut written: usize = 0;
        let ok = WriteProcessMemory(
            h_proc,
            remote_buf,
            dll_bytes.as_ptr() as *const _,
            dll_bytes.len(),
            &mut written,
        );
        if ok == 0 || written != dll_bytes.len() {
            eprintln!(
                "error: WriteProcessMemory failed (wrote {}/{}): GetLastError={}",
                written,
                dll_bytes.len(),
                GetLastError()
            );
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            CloseHandle(h_proc);
            return 5;
        }

        let kernel32_name = CString::new("kernel32.dll").expect("static");
        let h_kernel32 = GetModuleHandleA(kernel32_name.as_ptr() as *const u8);
        let load_library = CString::new("LoadLibraryA").expect("static");
        let load_library_addr =
            GetProcAddress(h_kernel32, load_library.as_ptr() as *const u8);
        if load_library_addr.is_none() {
            eprintln!(
                "error: GetProcAddress(LoadLibraryA) failed: GetLastError={}",
                GetLastError()
            );
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            CloseHandle(h_proc);
            return 6;
        }

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
            eprintln!(
                "error: CreateRemoteThread failed: GetLastError={}",
                GetLastError()
            );
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            CloseHandle(h_proc);
            return 7;
        }

        let wait = WaitForSingleObject(h_thread, INFINITE);
        if wait != WAIT_OBJECT_0 {
            eprintln!(
                "error: WaitForSingleObject returned 0x{:X}; GetLastError={}",
                wait,
                GetLastError()
            );
            CloseHandle(h_thread);
            VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
            CloseHandle(h_proc);
            return 7;
        }

        let mut exit_code: u32 = 0;
        GetExitCodeThread(h_thread, &mut exit_code);
        CloseHandle(h_thread);
        VirtualFreeEx(h_proc, remote_buf, 0, MEM_RELEASE);
        CloseHandle(h_proc);

        if exit_code == 0 {
            eprintln!("error: LoadLibraryA returned NULL — DLL load refused");
            return 8;
        }

        println!(
            "[inject] LoadLibraryA returned 0x{:X} — injection complete",
            exit_code
        );
        0
    }
}
