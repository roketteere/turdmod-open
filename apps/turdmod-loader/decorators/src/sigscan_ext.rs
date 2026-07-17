//! Extends the shared UE 4.27 sigscan (`crate::sigscan`) with the helpers the
//! decorator DLL needs to find per-class globals at runtime.
//!
//! Three building blocks (per
//! [docs/scum-internals/16-memory-signatures.md] §"URichTextBlock decorator
//! class CDOs" + the full chain proposed in
//! [docs/scum-internals/22-rich-text-decorator-internals.md] §Q3):
//!
//!   1. `scan_wide_string` — UTF-16 LE NUL-NUL-terminated needle in `.rdata`.
//!      Class names like `"RichTextBlockImageDecorator"` live here as part of
//!      UHT's `Z_Construct_UClass_*_Statics::ClassParams` static initialisers.
//!   2. `scan_lea_to`     — locate a `lea rcx/rdx/r8/r9, [rip+disp]` whose
//!      RIP-relative target equals a given address. The first such xref to a
//!      class-name string lands inside its registration callback.
//!   3. `find_guarray`    — entry point used by the decorator DLL on attach.
//!      Tries the canonical UE 4.27 `FUObjectArray` byte pattern (also used
//!      by the kitchen-sink loader: see
//!      `apps/turdmod-loader/src/hooks.rs::resolve_engine`), then falls back
//!      to the wide-string xref chain anchored on the literal
//!      `"FUObjectArray"` if the byte pattern misses (recommended fallback
//!      per dossier 16 §"Recommended primary approach" — when the canonical
//!      pattern drifts after a SCUM patch the string-anchored path is the
//!      stable refind route).
//!
//! Why a fallback at all? The dossier 22 conversation summary captures an
//! earlier session where the canonical byte pattern matched at the wrong
//! call site against build 23128448 (the codegen-shape it targets is shared
//! by other UObject-array members). When that happens the symptom is a
//! pointer that dereferences but doesn't look like an `FUObjectArray`
//! header — the caller-side validator should catch it. Until that lands we
//! at least surface a stable string-anchored path here.
//!
//! Tests against the live binary need SCUM running in-process; those are
//! marked `#[ignore]` so `cargo test --release --package turdmod-rich-decorators`
//! runs cleanly on a CI box without the game installed.

#![allow(dead_code)]

use crate::sigscan::{find, scan, ModuleSpan, Pattern};

// ----------------------------------------------------------------------------
// PE header walk — locate `.rdata`/`.data` section spans of a loaded module.
//
// The canonical layout (Microsoft "PE Format" reference) is:
//
//   ImageBase + 0x00            : IMAGE_DOS_HEADER (`MZ`); e_lfanew @ 0x3C
//   ImageBase + e_lfanew        : IMAGE_NT_HEADERS64
//                                   +0x00 Signature   `PE\0\0`
//                                   +0x04 FileHeader (20 bytes)
//                                       +0x02 NumberOfSections (u16)
//                                       +0x10 SizeOfOptionalHeader (u16)
//                                   +0x18 OptionalHeader  (SizeOfOptionalHeader)
//   first IMAGE_SECTION_HEADER  : right after the optional header (40 B each)
//
// We deliberately do raw-pointer reads instead of pulling
// `windows-sys::Win32::System::Diagnostics::Debug::IMAGE_NT_HEADERS64`
// because that struct is feature-gated on `Win32_System_SystemInformation`
// (for `IMAGE_FILE_MACHINE`) which the decorator crate does not enable, and
// the constraint is "no Cargo.toml deps".
// ----------------------------------------------------------------------------

const DOS_E_LFANEW_OFFSET: usize = 0x3C;
const NT_FILEHDR_NUMSECTIONS_OFFSET: usize = 0x04 + 0x02; // sig + FileHeader.NumberOfSections
const NT_FILEHDR_OPTHDRSIZE_OFFSET: usize = 0x04 + 0x10;  // sig + FileHeader.SizeOfOptionalHeader
const NT_OPTHDR_OFFSET: usize = 0x04 + 0x14;              // sig + sizeof(IMAGE_FILE_HEADER)
const SECTION_HEADER_SIZE: usize = 40;

/// One section's runtime span (already RVA-relocated, i.e. absolute pointers).
#[derive(Debug, Clone, Copy)]
pub struct SectionSpan {
    /// Section name, NUL-trimmed (`.text`, `.rdata`, `.data`, …).
    pub name: [u8; 8],
    pub base: *const u8,
    pub size: usize,
}

unsafe impl Send for SectionSpan {}
unsafe impl Sync for SectionSpan {}

impl SectionSpan {
    pub fn name_str(&self) -> &str {
        let n = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        std::str::from_utf8(&self.name[..n]).unwrap_or("?")
    }
}

/// Walk `span`'s PE headers. Returns each section's loaded span. Returns an
/// empty `Vec` if the headers don't look like a PE32+ image.
pub fn module_sections(span: ModuleSpan) -> Vec<SectionSpan> {
    let mut out = Vec::new();
    if span.size < 0x40 {
        return out;
    }
    // SAFETY: `span.base..span.base+span.size` is a contiguous loaded module.
    // Each read below stays inside that range — bounds-checked before use.
    unsafe {
        // DOS header sniff.
        if *span.base != b'M' || *span.base.add(1) != b'Z' {
            return out;
        }
        let e_lfanew = (span.base.add(DOS_E_LFANEW_OFFSET) as *const i32).read_unaligned();
        if e_lfanew < 0 {
            return out;
        }
        let nt = e_lfanew as usize;
        if nt + NT_OPTHDR_OFFSET >= span.size {
            return out;
        }
        let nt_base = span.base.add(nt);
        // PE\0\0 sniff.
        if *nt_base != b'P' || *nt_base.add(1) != b'E' || *nt_base.add(2) != 0 || *nt_base.add(3) != 0 {
            return out;
        }
        let num_sections = (nt_base.add(NT_FILEHDR_NUMSECTIONS_OFFSET) as *const u16)
            .read_unaligned() as usize;
        let opt_hdr_size = (nt_base.add(NT_FILEHDR_OPTHDRSIZE_OFFSET) as *const u16)
            .read_unaligned() as usize;
        let sec_table_off = nt + NT_OPTHDR_OFFSET + opt_hdr_size;
        if sec_table_off + num_sections * SECTION_HEADER_SIZE > span.size {
            return out;
        }

        for i in 0..num_sections {
            let sh = span.base.add(sec_table_off + i * SECTION_HEADER_SIZE);
            let mut name = [0u8; 8];
            for j in 0..8 {
                name[j] = *sh.add(j);
            }
            // IMAGE_SECTION_HEADER layout: VirtualSize @ +8, VirtualAddress @ +12.
            let virt_size = (sh.add(8) as *const u32).read_unaligned() as usize;
            let virt_addr = (sh.add(12) as *const u32).read_unaligned() as usize;
            if virt_addr.saturating_add(virt_size) > span.size {
                continue;
            }
            out.push(SectionSpan {
                name,
                base: span.base.add(virt_addr),
                size: virt_size,
            });
        }
    }
    out
}

/// Convenience: span covering `.rdata` (+ `.data` if present, contiguous or
/// not — we just return `.rdata` since UTF-16 string literals always live
/// there in MSVC-produced PE32+ images). Falls back to the full module span
/// if the section isn't found, so callers still get a sensible default.
pub fn module_data_section_span(span: ModuleSpan) -> ModuleSpan {
    for s in module_sections(span) {
        if &s.name[..6] == b".rdata" {
            return ModuleSpan { base: s.base, size: s.size };
        }
    }
    span
}

// ----------------------------------------------------------------------------
// UTF-16 wide-string scan
// ----------------------------------------------------------------------------

/// Encode `needle` as UTF-16 LE bytes with a 16-bit NUL terminator (so it
/// matches a C `wchar_t*` literal as compiled into MSVC `.rdata`) and locate
/// the first occurrence within `span`. Returns the pointer to the first byte
/// of the string body (the same address a `lea rcx, [rip+disp]` would
/// resolve to).
pub fn scan_wide_string(span: ModuleSpan, needle: &str) -> Option<*const u8> {
    if needle.is_empty() {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(needle.len() * 2 + 2);
    for u in needle.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    scan_raw_bytes(span, &bytes)
}

/// Plain memmem over the module span. Kept private — `Pattern`/`scan` is the
/// public API for byte patterns, but here we have a fixed needle (no
/// wildcards), so a simpler search is faster and reads cleanly.
fn scan_raw_bytes(span: ModuleSpan, needle: &[u8]) -> Option<*const u8> {
    if needle.is_empty() || span.size < needle.len() {
        return None;
    }
    // SAFETY: span is a loaded module; the OS maps it readable.
    let hay: &[u8] = unsafe { std::slice::from_raw_parts(span.base, span.size) };
    let last = hay.len() - needle.len();
    let first = needle[0];
    let mut i = 0;
    while i <= last {
        if hay[i] == first && &hay[i..i + needle.len()] == needle {
            // SAFETY: i ≤ last < span.size.
            return Some(unsafe { span.base.add(i) });
        }
        i += 1;
    }
    None
}

// ----------------------------------------------------------------------------
// LEA-to-target xref scan
// ----------------------------------------------------------------------------

/// Scan `span` for a `lea r{cx,dx,8,9}, [rip+disp32]` instruction whose
/// resolved displacement equals `target`. Returns the address of the LEA
/// instruction's first byte.
///
/// REX.W LEA encodings we accept (per dossier 16 §"Per-class candidate
/// patterns" — these are the registers UE registration callbacks use to
/// pass a class-name string to UHT helpers):
///
///   `48 8D 0D <disp32>` — lea rcx, [rip+disp]
///   `48 8D 15 <disp32>` — lea rdx, [rip+disp]
///   `4C 8D 05 <disp32>` — lea r8,  [rip+disp]
///   `4C 8D 0D <disp32>` — lea r9,  [rip+disp]
/// Iterate every `lea reg, [rip+disp]` instruction in `span` whose
/// resolved disp equals `target`. Returns the absolute addresses of every
/// LEA opcode start. Used by callers that need to try multiple xref
/// candidates because the first hit may not be in the function they want.
pub fn scan_all_leas_to(span: ModuleSpan, target: *const u8) -> Vec<*const u8> {
    const LEA_LEN: usize = 7;
    let mut out = Vec::new();
    if span.size < LEA_LEN { return out; }
    let hay: &[u8] = unsafe { std::slice::from_raw_parts(span.base, span.size) };
    let last = hay.len() - LEA_LEN;
    let mut i = 0usize;
    while i <= last {
        let b0 = hay[i];
        let b1 = hay[i + 1];
        let b2 = hay[i + 2];
        let is_lea = b1 == 0x8D
            && (
                (b0 == 0x48 && (b2 == 0x0D || b2 == 0x15))
                || (b0 == 0x4C && (b2 == 0x05 || b2 == 0x0D))
            );
        if is_lea {
            let disp = i32::from_le_bytes([hay[i + 3], hay[i + 4], hay[i + 5], hay[i + 6]]) as isize;
            let resolved = unsafe { span.base.add(i + LEA_LEN).offset(disp) };
            if resolved == target {
                out.push(unsafe { span.base.add(i) });
            }
        }
        i += 1;
    }
    out
}

pub fn scan_lea_to(span: ModuleSpan, target: *const u8) -> Option<*const u8> {
    const LEA_LEN: usize = 7;
    if span.size < LEA_LEN {
        return None;
    }
    // SAFETY: span is loaded module memory.
    let hay: &[u8] = unsafe { std::slice::from_raw_parts(span.base, span.size) };
    let last = hay.len() - LEA_LEN;
    let mut i = 0;
    while i <= last {
        let b0 = hay[i];
        let b1 = hay[i + 1];
        let b2 = hay[i + 2];
        let is_lea = b1 == 0x8D
            && (
                (b0 == 0x48 && (b2 == 0x0D || b2 == 0x15))
                || (b0 == 0x4C && (b2 == 0x05 || b2 == 0x0D))
            );
        if is_lea {
            // disp32 at offset +3, RIP = match + 7
            let disp = i32::from_le_bytes([hay[i + 3], hay[i + 4], hay[i + 5], hay[i + 6]]) as isize;
            // SAFETY: i + LEA_LEN ≤ span.size.
            let resolved = unsafe { span.base.add(i + LEA_LEN).offset(disp) };
            if resolved == target {
                return Some(unsafe { span.base.add(i) });
            }
        }
        i += 1;
    }
    None
}

// ----------------------------------------------------------------------------
// FUObjectArray entry point
// ----------------------------------------------------------------------------

/// Per-step outcome from [`find_guarray_verbose`]. The caller uses this to
/// log exactly where the resolution chain broke when both paths miss —
/// without it, "GUObjectArray: NOT FOUND" gives no clue whether to refind
/// the byte pattern, the string anchor, or the LEA xref shape.
#[derive(Debug, Clone)]
pub struct FindGuarrayDiag {
    pub byte_pattern_hit: Option<*const u8>,
    pub rdata_section_size: usize,
    pub wide_string_hit: Option<*const u8>,
    pub lea_xref_hit: Option<*const u8>,
    pub mov_rax_rip_hit: Option<*const u8>,
}

unsafe impl Send for FindGuarrayDiag {}
unsafe impl Sync for FindGuarrayDiag {}

/// Locate the `FUObjectArray` global in SCUM.exe.
///
/// Strategy: try every known UE 4.27 GUObjectArray pattern in order, and
/// return the first hit. Patterns ported from
/// [trumank/patternsleuth](https://github.com/trumank/patternsleuth)'s
/// `resolvers::unreal::guobject_array` — they cross-validate (multiple
/// patterns resolve to the same target on real binaries) and have been
/// proven against many UE 4.20–5.x games. SCUM 23128448 hits patterns
/// `[0]` and `[1]` (offline-confirmed 2026-05-09; both resolve to RVA
/// 0x7775440).
///
/// Returns the absolute address of the `FUObjectArray` struct in SCUM.exe's
/// address space (caller dereferences to walk `ObjObjects`).
pub fn find_guarray() -> Option<*const u8> {
    find_guarray_verbose().0
}

/// Patterns + RIP-disp-offset for the GUObjectArray resolver. Each tuple
/// is (pattern, rip_offset) where `rip_offset` is the byte index inside
/// the matched pattern at which the 4-byte RIP-relative displacement
/// starts. After matching, resolve as `match + rip_offset + 4 + disp32`.
///
/// Source: patternsleuth `resolvers/unreal/guobject_array.rs` 7-pattern
/// list. We keep only the PE-image patterns (Linux variants don't apply
/// to SCUM cooked Win64).
const GUARRAY_PATTERNS: &[(&str, usize)] = &[
    // Pattern 0 — original `|` mark at 24, hits SCUM 23128448 5×
    ("8B 05 ?? ?? ?? ?? 3B 05 ?? ?? ?? ?? 75 ?? 48 8D 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8D 05", 24),
    // Pattern 1 — `|` at 5, also hits SCUM 23128448 1×
    ("74 ?? 48 8D 0D ?? ?? ?? ?? C6 05 ?? ?? ?? ?? 01 E8 ?? ?? ?? ?? C6 05 ?? ?? ?? ?? 01", 5),
    // Pattern 2 — `|` at 8
    ("75 ?? 48 ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 45 33 C9 4C 89 74 24", 8),
    // Pattern 3 — `|` at 19, longer / more conservative
    ("45 84 C0 48 C7 41 10 00 00 00 00 B8 FF FF FF FF 4C 8D 1D ?? ?? ?? ?? 89 41 08 4C 8B D1 4C 89 19 0F 45 05 ?? ?? ?? ?? FF C0 89 41 08 3B 05", 19),
    // Pattern 4 — `|` at 15
    ("81 CE 00 00 00 02 83 E0 FB 89 47 08 48 8D 0D ?? ?? ?? ?? 48 89 FA 45 31 C0 E8 ?? ?? ?? ??", 15),
    // Pattern 5 — `|` at 14, very short / low specificity
    ("8B 05 ?? ?? ?? ?? 2B 05 ?? ?? ?? ?? 2B 05 ?? ?? ?? ??", 14),
    // Pattern 6 — `|` at 19
    ("E8 ?? ?? ?? ?? 8B 05 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 03 0D ?? ?? ?? ?? 29 C8 87 05", 19),
];

/// Same as [`find_guarray`], but also returns a per-step diagnostic so the
/// caller can log which path succeeded or where the chain broke. Useful
/// when both paths miss — names the exact step that needs refinding.
pub fn find_guarray_verbose() -> (Option<*const u8>, FindGuarrayDiag) {
    let mut diag = FindGuarrayDiag {
        byte_pattern_hit: None,
        rdata_section_size: 0,
        wide_string_hit: None,
        lea_xref_hit: None,
        mov_rax_rip_hit: None,
    };
    let Some(span) = crate::sigscan::main_module_span() else {
        return (None, diag);
    };

    // ---- Path 1: try every patternsleuth GUObjectArray pattern in order. ----
    // First hit returns. Cross-validation across multiple patterns isn't done
    // here (it would need the caller to compare results); the patterns are
    // disjoint enough in shape that a false positive across two patterns
    // landing on the same disp would be astronomically unlikely.
    for (pat_str, rip_off) in GUARRAY_PATTERNS {
        if let Ok(p) = Pattern::parse("FUObjectArray", pat_str, Some(*rip_off)) {
            if let Some(addr) = find(span, &p) {
                diag.byte_pattern_hit = Some(addr);
                return (Some(addr), diag);
            }
        }
    }

    // ---- Path 2: string-xref fallback ----
    let rdata = module_data_section_span(span);
    diag.rdata_section_size = rdata.size;
    let Some(str_ptr) = scan_wide_string(rdata, "FUObjectArray") else {
        return (None, diag);
    };
    diag.wide_string_hit = Some(str_ptr);

    let Some(lea_site) = scan_lea_to(span, str_ptr) else {
        return (None, diag);
    };
    diag.lea_xref_hit = Some(lea_site);

    // Look for `48 8B 05 <disp32>` in the next 64 bytes after `lea_site`.
    const SCAN_WIN: usize = 64;
    let lea_off = (lea_site as usize).wrapping_sub(span.base as usize);
    if lea_off >= span.size {
        return (None, diag);
    }
    let max = (lea_off + SCAN_WIN).min(span.size.saturating_sub(7));
    let bytes = unsafe { std::slice::from_raw_parts(span.base, span.size) };
    let mut i = lea_off;
    while i < max {
        if bytes[i] == 0x48 && bytes[i + 1] == 0x8B && bytes[i + 2] == 0x05 {
            let disp = i32::from_le_bytes([bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6]]) as isize;
            let next_ip = unsafe { span.base.add(i + 7) };
            let target = unsafe { next_ip.offset(disp) };
            diag.mov_rax_rip_hit = Some(target);
            return (Some(target), diag);
        }
        i += 1;
    }
    (None, diag)
}

// ----------------------------------------------------------------------------
// FName::FName(wchar_t const*, EFindName) constructor entry point
// ----------------------------------------------------------------------------

/// Locate `FName::FName(wchar_t const*, EFindName)` in SCUM.exe via the
/// generic UE 4.27 pattern published by patternsleuth (`fname.rs`
/// `FNameCtorWchar` PE resolver). Pattern offline-confirmed against SCUM
/// 23128448: 2 hits, both resolving to the same VA (RVA 0x2934880).
///
/// The returned address is the entry point of the FName constructor — a
/// member function with the canonical Win64 thiscall ABI:
///   * RCX = `FName*` (out, this)
///   * RDX = `wchar_t const*` (Name)
///   * R8  = `int` (EFindName: 0=Find, 1=Add, 2=Replace)
/// The constructor writes to *RCX and returns RCX in RAX.
///
/// Caller wraps as `extern "C" fn(*mut FName, *const u16, i32)`.
pub fn find_fname_ctor() -> Option<*const u8> {
    let span = crate::sigscan::main_module_span()?;
    let pat = Pattern::parse(
        "FName::FName",
        "EB 07 48 8D 15 ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? 41 B8 01 00 00 00 E8 ?? ?? ?? ??",
        Some(24),
    ).ok()?;
    find(span, &pat)
}

// ----------------------------------------------------------------------------
// Tests — synthetic buffers only. Live-binary tests are #[ignore]d; they need
// SCUM.exe loaded into the test process, which only happens via the launcher.
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn span_of(buf: &[u8]) -> ModuleSpan {
        ModuleSpan { base: buf.as_ptr(), size: buf.len() }
    }

    #[test]
    fn wide_string_finds_known_needle() {
        // Encode "FUObjectArray\0" as UTF-16 LE inside a noisy buffer.
        let mut buf: Vec<u8> = vec![0xAA; 16];
        for u in "FUObjectArray".encode_utf16() {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        buf.push(0);
        buf.push(0);
        buf.extend_from_slice(&[0xBB; 8]);
        let span = span_of(&buf);
        let hit = scan_wide_string(span, "FUObjectArray").expect("needle should match");
        let off = unsafe { hit.offset_from(buf.as_ptr()) };
        assert_eq!(off, 16);
    }

    #[test]
    fn wide_string_misses_when_absent() {
        let buf: Vec<u8> = vec![0; 64];
        let span = span_of(&buf);
        assert!(scan_wide_string(span, "FUObjectArray").is_none());
    }

    #[test]
    fn wide_string_requires_double_nul_terminator() {
        // Contains "FOO" UTF-16 but NO double-NUL afterwards.
        let mut buf: Vec<u8> = Vec::new();
        for u in "FOO".encode_utf16() {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        buf.extend_from_slice(&[0xCC, 0xCC]);
        let span = span_of(&buf);
        assert!(scan_wide_string(span, "FOO").is_none());
    }

    #[test]
    fn lea_to_resolves_rcx_disp() {
        // Layout: 0..16 padding, then `48 8D 0D <disp> 90 90 90 90 <target byte>`.
        //
        // We want `lea rcx, [rip+disp]` at offset 16 to resolve to a target at
        // offset 27. RIP at the LEA = 16 + 7 = 23. disp = 27 - 23 = 4.
        let mut buf: Vec<u8> = vec![0; 28];
        buf[16] = 0x48;
        buf[17] = 0x8D;
        buf[18] = 0x0D;
        buf[19..23].copy_from_slice(&4i32.to_le_bytes());
        buf[23] = 0x90;
        buf[24] = 0x90;
        buf[25] = 0x90;
        buf[26] = 0x90;
        buf[27] = 0xEF;
        let span = span_of(&buf);
        let target = unsafe { buf.as_ptr().add(27) };
        let lea = scan_lea_to(span, target).expect("LEA should match");
        let off = unsafe { lea.offset_from(buf.as_ptr()) };
        assert_eq!(off, 16);
    }

    #[test]
    fn lea_to_resolves_r8_disp() {
        // Same shape but `4C 8D 05 <disp>` (lea r8, [rip+disp]).
        let mut buf: Vec<u8> = vec![0; 24];
        buf[8] = 0x4C;
        buf[9] = 0x8D;
        buf[10] = 0x05;
        // RIP = 8 + 7 = 15. Want target = offset 20. disp = 5.
        buf[11..15].copy_from_slice(&5i32.to_le_bytes());
        buf[20] = 0xCD;
        let span = span_of(&buf);
        let target = unsafe { buf.as_ptr().add(20) };
        let lea = scan_lea_to(span, target).expect("LEA r8 should match");
        let off = unsafe { lea.offset_from(buf.as_ptr()) };
        assert_eq!(off, 8);
    }

    #[test]
    fn lea_to_misses_when_disp_doesnt_match() {
        let mut buf: Vec<u8> = vec![0; 24];
        buf[0] = 0x48;
        buf[1] = 0x8D;
        buf[2] = 0x0D;
        buf[3..7].copy_from_slice(&100i32.to_le_bytes());
        let span = span_of(&buf);
        // Target is somewhere outside what disp resolves to.
        let target = unsafe { buf.as_ptr().add(20) };
        assert!(scan_lea_to(span, target).is_none());
    }

    /// Synthetic minimal PE32+ blob just sufficient to exercise the section
    /// walker — exercises the offsets without requiring a full linker. We
    /// keep this small (one section, fake `.rdata`) so the test stays
    /// readable; broader PE coverage (multiple sections, varying optional
    /// header sizes) is implicitly covered by the live-binary `#[ignore]`d
    /// runtime test below.
    #[test]
    fn module_sections_walks_minimal_pe() {
        let opt_hdr_size: u16 = 240; // typical PE32+ value
        let nt_off: usize = 0x80;
        let sec_off = nt_off + NT_OPTHDR_OFFSET + opt_hdr_size as usize;
        let total_size = sec_off + SECTION_HEADER_SIZE + 0x100;

        let mut buf: Vec<u8> = vec![0; total_size];
        // DOS magic + e_lfanew
        buf[0] = b'M';
        buf[1] = b'Z';
        buf[DOS_E_LFANEW_OFFSET..DOS_E_LFANEW_OFFSET + 4]
            .copy_from_slice(&(nt_off as i32).to_le_bytes());
        // PE signature
        buf[nt_off] = b'P';
        buf[nt_off + 1] = b'E';
        // FileHeader.NumberOfSections = 1, SizeOfOptionalHeader = opt_hdr_size
        buf[nt_off + NT_FILEHDR_NUMSECTIONS_OFFSET..nt_off + NT_FILEHDR_NUMSECTIONS_OFFSET + 2]
            .copy_from_slice(&1u16.to_le_bytes());
        buf[nt_off + NT_FILEHDR_OPTHDRSIZE_OFFSET..nt_off + NT_FILEHDR_OPTHDRSIZE_OFFSET + 2]
            .copy_from_slice(&opt_hdr_size.to_le_bytes());
        // One section: name=".rdata\0\0", VirtualSize=0x100, VirtualAddress=sec_off+SECTION_HEADER_SIZE
        let sec_va = sec_off + SECTION_HEADER_SIZE;
        let sec_size = 0x100u32;
        buf[sec_off..sec_off + 6].copy_from_slice(b".rdata");
        buf[sec_off + 8..sec_off + 12].copy_from_slice(&sec_size.to_le_bytes());
        buf[sec_off + 12..sec_off + 16].copy_from_slice(&(sec_va as u32).to_le_bytes());

        let span = span_of(&buf);
        let secs = module_sections(span);
        assert_eq!(secs.len(), 1, "expected exactly one section");
        assert_eq!(secs[0].name_str(), ".rdata");
        assert_eq!(secs[0].size, sec_size as usize);
        assert_eq!(secs[0].base, unsafe { buf.as_ptr().add(sec_va) });

        // module_data_section_span should pick the same .rdata
        let rdata = module_data_section_span(span);
        assert_eq!(rdata.size, sec_size as usize);
    }

    #[test]
    fn module_sections_rejects_non_pe() {
        let buf: Vec<u8> = vec![0xAA; 256]; // no MZ header
        let span = span_of(&buf);
        assert!(module_sections(span).is_empty());
        // Fallback: data-section-span returns the whole module.
        let ds = module_data_section_span(span);
        assert_eq!(ds.size, span.size);
    }

    /// Live-binary test — only meaningful when SCUM.exe is the host process.
    /// Cargo test runs in `turdmod_rich_decorators-<hash>.exe`, which is NOT
    /// SCUM, so the byte-pattern path will miss and the string-xref fallback
    /// will not find a `"FUObjectArray"` string in our test binary. Marked
    /// `#[ignore]` so unit-test CI passes; the integration suite under
    /// `apps/turdmod-loader/launcher/` exercises the live path.
    #[test]
    #[ignore = "requires SCUM.exe as the host process; covered by launcher integration tests"]
    fn find_guarray_against_live_scum() {
        let p = find_guarray().expect("FUObjectArray should resolve in live SCUM");
        assert!(!p.is_null());
    }
}
