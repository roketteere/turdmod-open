//! Signature scanner — finds UE4 globals (`GEngine`, `GWorld`, etc.) in
//! the loaded SCUM module by matching byte patterns against the .text
//! section, then dereferencing RIP-relative addresses.
//!
//! Patterns live as data, not code: a runtime-mutable table the dossier
//! at [docs/scum-internals/16-memory-signatures.md] mirrors. When SCUM
//! patches and the patterns drift, swap the bytes — no recompile of the
//! engine layer.
//!
//! How a pattern resolves:
//!
//!   1. Walk SCUM's main module's `.text` section.
//!   2. Find the first byte sequence matching the pattern (with `??`
//!      wildcards). The match address is the start of the matched bytes.
//!   3. Apply `rip_offset`: read the 4-byte signed displacement at
//!      `match + rip_offset` and resolve as `match + rip_offset + 4 +
//!      displacement`. That target IS the global's address.
//!
//! All addresses are absolute pointers in SCUM.exe's address space, so
//! safe to read with normal pointer dereferences (we're already running
//! inside the process via the loader DLL).

#![allow(dead_code)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::ProcessStatus::{
    GetModuleInformation, MODULEINFO,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::logging;

/// One signature pattern, as kept in
/// [docs/scum-internals/16-memory-signatures.md].
///
/// `bytes` is the byte template. `mask` is parallel: `b'x'` means "match
/// the corresponding byte exactly", `b'?'` means "any byte". When
/// `rip_offset` is `Some(n)`, after a match resolve a 4-byte RIP-relative
/// displacement starting at `match + n`.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub mask: Vec<u8>,
    pub rip_offset: Option<usize>,
}

impl Pattern {
    /// Build a pattern from the IDA-style hex string used in the dossier:
    ///   "48 8B 05 ?? ?? ?? ?? 48 85 C0"
    /// Each `??` becomes a wildcard byte.
    pub fn parse(name: &'static str, sig: &str, rip_offset: Option<usize>) -> Result<Self, String> {
        let mut bytes = Vec::new();
        let mut mask = Vec::new();
        for tok in sig.split_whitespace() {
            if tok == "??" || tok == "?" {
                bytes.push(0u8);
                mask.push(b'?');
            } else {
                let b = u8::from_str_radix(tok, 16)
                    .map_err(|e| format!("bad hex byte `{tok}` in pattern `{name}`: {e}"))?;
                bytes.push(b);
                mask.push(b'x');
            }
        }
        if bytes.is_empty() {
            return Err(format!("pattern `{name}` is empty"));
        }
        Ok(Pattern { name, bytes, mask, rip_offset })
    }
}

/// Bounds of a loaded module's executable mapping.
#[derive(Debug, Clone, Copy)]
pub struct ModuleSpan {
    pub base: *const u8,
    pub size: usize,
}

unsafe impl Send for ModuleSpan {}
unsafe impl Sync for ModuleSpan {}

/// Get the span of `SCUM.exe` (or any module by name) loaded into the
/// current process.
pub fn module_span(name: &str) -> Option<ModuleSpan> {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    let h: HMODULE = unsafe { GetModuleHandleW(wide.as_ptr()) };
    if h.is_null() {
        return None;
    }
    let mut info: MODULEINFO = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        GetModuleInformation(
            GetCurrentProcess(),
            h,
            &mut info as *mut MODULEINFO,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(ModuleSpan {
        base: info.lpBaseOfDll as *const u8,
        size: info.SizeOfImage as usize,
    })
}

/// Get the span of the *main* module (the EXE itself).
pub fn main_module_span() -> Option<ModuleSpan> {
    let h: HMODULE = unsafe { GetModuleHandleW(ptr::null()) };
    if h.is_null() {
        return None;
    }
    let mut info: MODULEINFO = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        GetModuleInformation(
            GetCurrentProcess(),
            h,
            &mut info as *mut MODULEINFO,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(ModuleSpan {
        base: info.lpBaseOfDll as *const u8,
        size: info.SizeOfImage as usize,
    })
}

/// Look up the human-readable filename of the main module.
pub fn main_module_name() -> Option<String> {
    let h: HMODULE = unsafe { GetModuleHandleW(ptr::null()) };
    if h.is_null() {
        return None;
    }
    let mut buf = [0u16; 512];
    let n = unsafe {
        windows_sys::Win32::System::ProcessStatus::GetModuleFileNameExW(
            GetCurrentProcess(),
            h,
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    if n == 0 {
        return None;
    }
    let s = OsString::from_wide(&buf[..n as usize]);
    s.into_string().ok()
}

/// Scan a span for the first occurrence of `pat`. Returns the absolute
/// address where the match starts.
pub fn scan(span: ModuleSpan, pat: &Pattern) -> Option<*const u8> {
    if pat.bytes.is_empty() {
        return None;
    }
    let plen = pat.bytes.len();
    if span.size < plen {
        return None;
    }
    // SAFETY: We're scanning bytes within our own process's loaded
    // module. The OS guarantees these pages are readable.
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(span.base, span.size) };

    let last = span.size - plen;
    let mut i = 0;
    'outer: while i <= last {
        for j in 0..plen {
            let mb = pat.mask[j];
            if mb == b'?' {
                continue;
            }
            if bytes[i + j] != pat.bytes[j] {
                i += 1;
                continue 'outer;
            }
        }
        return Some(unsafe { span.base.add(i) });
    }
    None
}

/// Resolve a RIP-relative 4-byte displacement after a match. Returns the
/// absolute address the displacement points to.
pub fn resolve_rip(match_addr: *const u8, rip_offset: usize) -> *const u8 {
    unsafe {
        let disp_ptr = match_addr.add(rip_offset) as *const i32;
        let disp = disp_ptr.read_unaligned() as isize;
        // Next-instruction-pointer is `match + rip_offset + 4`.
        let next_ip = match_addr.add(rip_offset + 4);
        next_ip.offset(disp)
    }
}

/// Find a pattern + resolve. Returns the target address.
pub fn find(span: ModuleSpan, pat: &Pattern) -> Option<*const u8> {
    let m = scan(span, pat)?;
    Some(match pat.rip_offset {
        Some(off) => resolve_rip(m, off),
        None => m,
    })
}

/// Diagnostic — log the main module's identity + size on first call.
pub fn log_module_info() {
    let name = main_module_name().unwrap_or_else(|| "(unknown)".to_string());
    let span = main_module_span();
    match span {
        Some(s) => logging::log(&format!(
            "[sigscan] main module: {name} base={:p} size=0x{:x}",
            s.base, s.size
        )),
        None => logging::log(&format!("[sigscan] main module: {name} (size unknown)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_parse_ok() {
        let p = Pattern::parse("test", "48 8B 05 ?? ?? ?? ?? 48 85 C0", Some(3)).unwrap();
        assert_eq!(p.bytes.len(), 10);
        assert_eq!(p.mask, b"xxx????xxx".to_vec());
        assert_eq!(p.rip_offset, Some(3));
    }

    #[test]
    fn pattern_parse_bad_hex() {
        let r = Pattern::parse("test", "ZZ", None);
        assert!(r.is_err());
    }

    #[test]
    fn scan_finds_in_synthetic_buffer() {
        let buf: &[u8] = b"\x00\x00\x48\x8b\x05\xaa\xbb\xcc\xdd\x48\x85\xc0";
        let span = ModuleSpan { base: buf.as_ptr(), size: buf.len() };
        let p = Pattern::parse("t", "48 8B 05 ?? ?? ?? ?? 48 85 C0", None).unwrap();
        let m = scan(span, &p).expect("should match");
        let off = unsafe { m.offset_from(buf.as_ptr()) };
        assert_eq!(off, 2);
    }

    #[test]
    fn scan_no_match() {
        let buf: &[u8] = b"\x01\x02\x03\x04";
        let span = ModuleSpan { base: buf.as_ptr(), size: buf.len() };
        let p = Pattern::parse("t", "FF FF FF FF", None).unwrap();
        assert!(scan(span, &p).is_none());
    }

    #[test]
    fn resolve_rip_basic() {
        // Bytes: AA AA AA <disp i32 = 4> 11 22 33 44 ...
        // match_addr points at first AA; rip_offset = 3 → disp at offset 3.
        // next_ip = match + 3 + 4 = match + 7 → +disp(4) = match + 11.
        let buf: [u8; 16] = [
            0xAA, 0xAA, 0xAA, 0x04, 0x00, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0xEE, 0, 0, 0, 0,
        ];
        let target = resolve_rip(buf.as_ptr(), 3);
        let off = unsafe { target.offset_from(buf.as_ptr()) };
        assert_eq!(off, 11);
    }
}
