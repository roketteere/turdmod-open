// DLL injection — injects UE4SS + turdmod_server_loader into GameServer.exe.
// Uses CreateRemoteThread + LoadLibraryA pattern. Called by engine.rs after
// CREATE_SUSPENDED process creation.

use anyhow::{bail, Context, Result};
use std::ffi::CString;
use windows::core::PCSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_ALL_ACCESS,
};

const INJECT_TIMEOUT_MS: u32 = 15_000;

pub fn inject_dll(pid: u32, dll_path: &str) -> Result<()> {
    tracing::info!("injecting {} into pid {}", dll_path, pid);

    let dll_cstr = CString::new(dll_path).context("dll path to cstring")?;
    let dll_bytes = dll_cstr.as_bytes_with_nul();

    let proc = unsafe {
        OpenProcess(PROCESS_ALL_ACCESS, false, pid).context("OpenProcess")?
    };

    let result = do_inject(proc, dll_bytes);
    unsafe { let _ = CloseHandle(proc); }
    result
}

fn do_inject(proc: HANDLE, dll_bytes: &[u8]) -> Result<()> {
    let remote_buf = unsafe {
        VirtualAllocEx(proc, None, dll_bytes.len(), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
    };
    if remote_buf.is_null() {
        bail!("VirtualAllocEx failed");
    }

    let write_ok = unsafe {
        windows::Win32::System::Diagnostics::Debug::WriteProcessMemory(
            proc,
            remote_buf,
            dll_bytes.as_ptr() as _,
            dll_bytes.len(),
            None,
        )
    };
    if write_ok.is_err() {
        unsafe { let _ = VirtualFreeEx(proc, remote_buf, 0, MEM_RELEASE); }
        bail!("WriteProcessMemory failed");
    }

    let k32_name = CString::new("kernel32.dll").unwrap();
    let loadlib_name = CString::new("LoadLibraryA").unwrap();

    let k32 = unsafe {
        GetModuleHandleA(PCSTR(k32_name.as_ptr() as _))
            .context("GetModuleHandleA kernel32")?
    };
    let loadlib_fn = unsafe {
        GetProcAddress(k32, PCSTR(loadlib_name.as_ptr() as _))
            .context("GetProcAddress LoadLibraryA")?
    };

    // @inv: loadlib_fn address is valid across processes (kernel32 base is fixed per boot)
    let thread_fn: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
        unsafe { std::mem::transmute(loadlib_fn) };

    let thread = unsafe {
        CreateRemoteThread(proc, None, 0, Some(thread_fn), Some(remote_buf), 0, None)
            .context("CreateRemoteThread")?
    };

    let wait_result = unsafe { WaitForSingleObject(thread, INJECT_TIMEOUT_MS) };
    unsafe {
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(proc, remote_buf, 0, MEM_RELEASE);
    }

    if wait_result.0 != 0 {
        bail!("WaitForSingleObject returned 0x{:x}", wait_result.0);
    }

    tracing::info!("injection complete");
    Ok(())
}
