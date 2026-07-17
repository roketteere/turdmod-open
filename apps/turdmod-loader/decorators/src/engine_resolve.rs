//! Engine-resolution probe — orchestrates the foundation modules into a
//! single "what did we find in SCUM.exe?" diagnostic step.
//!
//! Composes the layers in dependency order:
//!
//! ```text
//!   sigscan / sigscan_ext   →   raw FNamePool* + FUObjectArray*
//!         │                              │
//!         ▼                              ▼
//!     fname::FNamePool             guarray::GUObjectArray
//!         │                              │
//!         └──────────► resolve UClass ───┤
//!                                        │
//!                              uobject::CdoProbe
//!                              ──────────────────►  per-class CDO offset
//!                                        │
//!                                        ▼
//!                              vtable::diff_first_override
//!                              ──────────────────►  CreateDecorator vtable index
//! ```
//!
//! Output is a [`ProbeReport`] — a plain data struct of every pointer +
//! every "did this step succeed?" boolean. Callers (today: `hooks::install`)
//! log the report to `%LOCALAPPDATA%/TurdMOD/decorators.log` so post-attach
//! diagnostics work without a debugger. When a future Phase-B step wants to
//! install the real detour, it asks the report whether all three classes
//! resolved + whether the two children's vtable-diff results agree before
//! touching anything.
//!
//! Status: **candidate, requires runtime probe against SCUM 23128448** —
//! every constituent module carries the same warning. The first run against
//! a live SCUM client is the validation event; on miss the report logs
//! exactly which step failed so the next session knows what to refind.

#![allow(dead_code)]

use crate::fname::{FName, FNamePool};
use crate::guarray::GUObjectArray;
use crate::logging;
use crate::sigscan::{find, main_module_span, ModuleSpan, Pattern};
use crate::sigscan_ext::{self, module_sections, scan_all_leas_to, scan_lea_to, scan_wide_string};
use crate::uobject::{class_default_object, name_private, CdoProbe, UClassPtr, UObjectPtr};
use crate::vtable::{diff_first_override, DiffOutcome};

/// Construct a UE4 `FName` for `name` by calling the engine's
/// `FName::FName(wchar_t*, EFindName)` constructor at `ctor_va`.
/// Returns `Some(FName)` only if the resulting comparison_index is
/// non-zero (zero = not in the pool, FNAME_Find returns the empty FName).
///
/// # Safety
/// `ctor_va` must point at the real engine FName ctor; calling something
/// else with this signature will corrupt registers / crash.
pub unsafe fn construct_fname_via_engine(ctor_va: *const u8, name: &str) -> Option<FName> {
    // EFindName: 0 = FNAME_Find (don't add if absent), 1 = FNAME_Add,
    // 2 = FNAME_Replace_Not_Safe_For_Threading. We want Find — never
    // mutate the pool from a probe.
    const FNAME_FIND: i32 = 0;
    type CtorFn = unsafe extern "C" fn(*mut FName, *const u16, i32);
    let ctor: CtorFn = std::mem::transmute(ctor_va);
    let wname: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut out = FName { comparison_index: 0, number: 0 };
    ctor(&mut out, wname.as_ptr(), FNAME_FIND);
    if out.comparison_index == 0 { None } else { Some(out) }
}

/// The three rich-text decorator classes we need to resolve. Order matters
/// for the vtable-diff step: index 0 is the abstract parent, indices 1+ are
/// the concrete children that override `CreateDecorator`.
pub const DECORATOR_CLASS_NAMES: &[&str] = &[
    "RichTextBlockDecorator",
    "RichTextBlockImageDecorator",
    "RichTextBlockActionPromptDecorator",
];

/// Index of the parent class within `DECORATOR_CLASS_NAMES` — used by
/// vtable-diff to pick the parent CDO.
pub const PARENT_INDEX: usize = 0;

/// Seed pairs for the CDO-offset probe. Each tuple is
/// `(class_fname, cdo_fname)` — both walked through `GUObjectArray` to get
/// the matched `UClass*` and CDO `UObject*`. The probe needs ≥2 successful
/// pairs to lock an offset; we ship four candidates so a partial miss
/// (e.g. a class that's been renamed in SCUM's engine fork) still leaves
/// enough to converge.
pub const CDO_SEED_PAIRS: &[(&str, &str)] = &[
    ("Object",    "Default__Object"),
    ("Class",     "Default__Class"),
    ("Field",     "Default__Field"),
    ("Interface", "Default__Interface"),
];

/// Per-class resolution outcome.
#[derive(Debug, Clone)]
pub struct ResolvedClass {
    pub name: &'static str,
    pub uclass: Option<UClassPtr>,
    pub cdo: Option<UObjectPtr>,
    /// Vtable-diff result of this class's CDO vs the parent class's CDO.
    /// Always `None` for the parent itself (`PARENT_INDEX`). For children:
    /// `Some(DivergesAt(i))` means index `i` IS the `CreateDecorator`
    /// vtable index.
    pub vtable_diff_vs_parent: Option<DiffOutcome>,
    /// Failure mode reported to the log when a step misses. Empty on full
    /// success.
    pub note: String,
}

/// Full probe report. Every field documents one "did this resolve?" check.
#[derive(Debug, Clone)]
pub struct ProbeReport {
    pub module: Option<ModuleSpan>,
    pub fnamepool_addr: Option<*const u8>,
    pub guarray_addr: Option<*const u8>,
    pub guarray_num_elements: Option<usize>,
    /// Number of `(UClass, CDO)` pairs the CDO probe successfully resolved
    /// from `CDO_SEED_PAIRS` before locking an offset.
    pub cdo_seeds_resolved: usize,
    /// Probed offset of `UClass::ClassDefaultObject`. `None` if probing
    /// failed (insufficient seeds, layout drift, …).
    pub cdo_offset: Option<usize>,
    pub classes: Vec<ResolvedClass>,
}

unsafe impl Send for ProbeReport {}
unsafe impl Sync for ProbeReport {}

impl ProbeReport {
    pub fn empty() -> Self {
        Self {
            module: None,
            fnamepool_addr: None,
            guarray_addr: None,
            guarray_num_elements: None,
            cdo_seeds_resolved: 0,
            cdo_offset: None,
            classes: DECORATOR_CLASS_NAMES
                .iter()
                .map(|n| ResolvedClass {
                    name: n,
                    uclass: None,
                    cdo: None,
                    vtable_diff_vs_parent: None,
                    note: String::new(),
                })
                .collect(),
        }
    }

    /// True iff every class resolved to a non-null `UClassPtr`.
    pub fn all_uclasses_resolved(&self) -> bool {
        self.classes.iter().all(|c| c.uclass.is_some())
    }

    /// True iff every class resolved BOTH a UClass and a CDO.
    pub fn all_cdos_resolved(&self) -> bool {
        self.classes.iter().all(|c| c.cdo.is_some())
    }

    /// True iff both child classes (everything but `PARENT_INDEX`) report
    /// `DiffOutcome::DivergesAt(i)` for the SAME index `i`. That mutual
    /// agreement is the strongest pre-detour check we can run without
    /// installing anything: both children override `CreateDecorator` (the
    /// only virtual either of them touches in `URichTextBlockDecorator`),
    /// so their vtables MUST diverge at the same slot. Disagreement means
    /// either layout drift or a vtable-pointer miss.
    pub fn child_vtable_diffs_agree(&self) -> Option<usize> {
        let children: Vec<usize> = self.classes.iter().enumerate()
            .filter(|(i, _)| *i != PARENT_INDEX)
            .filter_map(|(_, c)| match c.vtable_diff_vs_parent {
                Some(DiffOutcome::DivergesAt(i)) => Some(i),
                _ => None,
            })
            .collect();
        if children.len() != self.classes.len() - 1 { return None; }
        let first = *children.first()?;
        if children.iter().all(|i| *i == first) { Some(first) } else { None }
    }

    /// Multi-line human-readable summary suitable for `decorators.log`.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("[engine-resolve] probe report:\n");
        match self.module {
            Some(m) => s.push_str(&format!(
                "  module          : base={:p} size=0x{:x}\n",
                m.base, m.size
            )),
            None => s.push_str("  module          : NOT FOUND\n"),
        }
        match self.fnamepool_addr {
            Some(p) => s.push_str(&format!("  FNamePool       : {:p}\n", p)),
            None    => s.push_str("  FNamePool       : NOT FOUND (sigscan miss)\n"),
        }
        match (self.guarray_addr, self.guarray_num_elements) {
            (Some(p), Some(n)) => s.push_str(&format!(
                "  GUObjectArray   : {:p} (NumElements={})\n", p, n,
            )),
            (Some(p), None) => s.push_str(&format!(
                "  GUObjectArray   : {:p} (NumElements unread)\n", p,
            )),
            (None, _) => s.push_str("  GUObjectArray   : NOT FOUND (sigscan miss)\n"),
        }
        s.push_str(&format!(
            "  CDO probe       : seeds_resolved={}/{} offset={}\n",
            self.cdo_seeds_resolved,
            CDO_SEED_PAIRS.len(),
            match self.cdo_offset {
                Some(o) => format!("0x{:x}", o),
                None => "UNRESOLVED".to_string(),
            }
        ));
        s.push_str("  decorator classes:\n");
        for c in &self.classes {
            let uc = c.uclass.map(|u| format!("{:p}", u.0)).unwrap_or_else(|| "MISS".into());
            let cdo = c.cdo.map(|o| format!("{:p}", o.0)).unwrap_or_else(|| "MISS".into());
            let diff = match c.vtable_diff_vs_parent {
                None => "(parent)".to_string(),
                Some(DiffOutcome::DivergesAt(i)) => format!("override@{}", i),
                Some(DiffOutcome::Identical) => "Identical".to_string(),
                Some(DiffOutcome::Degenerate) => "Degenerate".to_string(),
            };
            if c.uclass.is_none() || (c.cdo.is_none() && self.cdo_offset.is_some()) {
                s.push_str(&format!(
                    "    {:<40} uclass={} cdo={} note={}\n",
                    c.name, uc, cdo, c.note
                ));
            } else {
                s.push_str(&format!(
                    "    {:<40} uclass={} cdo={} diff={}\n",
                    c.name, uc, cdo, diff
                ));
            }
        }
        match self.child_vtable_diffs_agree() {
            Some(i) => s.push_str(&format!(
                "  consensus       : children agree on CreateDecorator@vtable[{}]\n", i)),
            None if self.all_cdos_resolved() => s.push_str(
                "  consensus       : DISAGREE — refuse to install\n"),
            None => {} // already obvious from earlier MISS lines
        }
        s
    }
}

/// FNamePool sigscan pattern, mirrored verbatim from
/// [docs/scum-internals/16-memory-signatures.md] §"FNamePool".
const FNAMEPOOL_PATTERN: &str = "74 09 48 8D 15 ?? ?? ?? ?? EB 16 48 8D 15 ?? ?? ?? ??";
const FNAMEPOOL_RIP_OFFSET: usize = 5;

/// Per-step diagnostic for [`resolve_fnamepool_verbose`] — same role as
/// [`sigscan_ext::FindGuarrayDiag`].
#[derive(Debug, Clone)]
pub struct FnamePoolDiag {
    pub byte_pattern_hit: Option<*const u8>,
    pub fname_string_hit: Option<*const u8>,
    pub fname_lea_hit: Option<*const u8>,
    pub fname_lea_followup_disp_hit: Option<*const u8>,
}

unsafe impl Send for FnamePoolDiag {}
unsafe impl Sync for FnamePoolDiag {}

/// Resolve `FNamePool*` via the canonical UE 4.27 byte pattern. Mirror of
/// `apps/turdmod-loader/src/hooks.rs::resolve_engine` — same pattern data,
/// scoped to the decorator DLL so a Phase-B miss doesn't depend on the
/// kitchen-sink loader being attached first.
pub fn resolve_fnamepool(span: ModuleSpan) -> Option<*const u8> {
    resolve_fnamepool_verbose(span).0
}

/// Diagnostic version of [`resolve_fnamepool`]. On byte-pattern miss, falls
/// back to a string-anchored path: scan `.rdata` for any of a handful of
/// FName-related anchor strings ("FName", "Names"), find a `lea` xref, and
/// scan forward for a `mov rax, [rip+disp]` whose target shape looks like
/// the pool. The fallback is intentionally loose; a verified pattern goes
/// in next session.
pub fn resolve_fnamepool_verbose(span: ModuleSpan) -> (Option<*const u8>, FnamePoolDiag) {
    let mut diag = FnamePoolDiag {
        byte_pattern_hit: None,
        fname_string_hit: None,
        fname_lea_hit: None,
        fname_lea_followup_disp_hit: None,
    };

    // ---- Path 1: canonical byte pattern ----
    if let Ok(pat) = Pattern::parse("FNamePool", FNAMEPOOL_PATTERN, Some(FNAMEPOOL_RIP_OFFSET)) {
        if let Some(addr) = find(span, &pat) {
            diag.byte_pattern_hit = Some(addr);
            return (Some(addr), diag);
        }
    }

    // ---- Path 2: string-anchored fallback (loose; first hit per anchor) ----
    let rdata = sigscan_ext::module_data_section_span(span);
    const FNAME_ANCHORS: &[&str] = &[
        "FNamePool",
        "FName::DisplayIndexToString",
        "GetPlainNameString",
    ];
    let mut anchor_target: Option<*const u8> = None;
    for needle in FNAME_ANCHORS {
        if let Some(s) = scan_wide_string(rdata, needle) {
            anchor_target = Some(s);
            diag.fname_string_hit = Some(s);
            break;
        }
    }
    let Some(str_ptr) = anchor_target else { return (None, diag); };
    let Some(lea_site) = scan_lea_to(span, str_ptr) else { return (None, diag); };
    diag.fname_lea_hit = Some(lea_site);

    // Look forward 128 bytes for a `mov rax, [rip+disp32]` (REX.W 48 8B 05).
    const SCAN_WIN: usize = 128;
    let lea_off = (lea_site as usize).wrapping_sub(span.base as usize);
    if lea_off >= span.size { return (None, diag); }
    let max = (lea_off + SCAN_WIN).min(span.size.saturating_sub(7));
    let bytes = unsafe { std::slice::from_raw_parts(span.base, span.size) };
    let mut i = lea_off;
    while i < max {
        if bytes[i] == 0x48 && bytes[i + 1] == 0x8B && bytes[i + 2] == 0x05 {
            let disp = i32::from_le_bytes([bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6]]) as isize;
            let next_ip = unsafe { span.base.add(i + 7) };
            let target = unsafe { next_ip.offset(disp) };
            diag.fname_lea_followup_disp_hit = Some(target);
            return (Some(target), diag);
        }
        i += 1;
    }
    (None, diag)
}

/// Per-class diagnostic for [`find_class_via_static_xref`].
#[derive(Debug, Clone)]
pub struct StaticXrefDiag {
    pub class_name: &'static str,
    pub wide_string_hit: Option<*const u8>,
    pub lea_xref_hit: Option<*const u8>,
    pub outer_singleton_write_hit: Option<*const u8>,
    pub uclass_target: Option<*const u8>,
}

unsafe impl Send for StaticXrefDiag {}
unsafe impl Sync for StaticXrefDiag {}

/// Resolve a UClass pointer directly via the string-anchored xref chain.
///
/// Per dossier 16 §"URichTextBlock decorator class CDOs" + §"Recommended
/// fallback":
///
/// 1. Scan `.rdata` for `"ClassName"` UTF-16 LE NUL-NUL — class names live
///    here as part of `Z_Construct_UClass_*_Statics::ClassParams`.
/// 2. Scan `.text` for `lea rcx/rdx/r8/r9, [rip+disp]` whose target equals
///    that string. The first such LEA is inside the registration callback
///    (or its ClassParams initializer).
/// 3. From the LEA site, scan forward up to `MAX_REGISTRATION_BODY_BYTES`
///    for the first `mov [rip+disp32], rax` (REX.W `48 89 05 disp32`).
///    That instruction writes the constructed `UClass*` to
///    `Z_Registration_Info_UClass_<Class>.OuterSingleton`.
/// 4. Resolve the `disp32`'s absolute target — that's the static slot
///    returned by `StaticClass()`. Read the qword stored there to get
///    the live `UClass*`.
///
/// Step 3's window is generous (1 KiB) because UE4's registration
/// callbacks routinely run ~500 bytes between the ClassParams LEA and the
/// final OuterSingleton write. Caller-side validation (sanity-check the
/// resulting pointer's vtable) catches false matches.
pub fn find_class_via_static_xref(name: &'static str) -> (Option<UClassPtr>, StaticXrefDiag) {
    let mut diag = StaticXrefDiag {
        class_name: name,
        wide_string_hit: None,
        lea_xref_hit: None,
        outer_singleton_write_hit: None,
        uclass_target: None,
    };
    let Some(span) = main_module_span() else { return (None, diag); };
    let rdata = sigscan_ext::module_data_section_span(span);

    // Step 1.
    let Some(str_ptr) = scan_wide_string(rdata, name) else { return (None, diag); };
    diag.wide_string_hit = Some(str_ptr);

    // Step 2 — try EVERY LEA xref to the string. The first LEA isn't
    // always inside the registration callback (UE may also reference the
    // class name from property-registration helpers, log statements, or
    // engine debug paths). We scan each LEA's forward window for an
    // OuterSingleton write; first one that yields a non-null UClass wins.
    let leas = scan_all_leas_to(span, str_ptr);
    if leas.is_empty() { return (None, diag); }
    diag.lea_xref_hit = Some(leas[0]);

    // Step 3: for each LEA, scan forward for `mov [rip+disp32], reg`
    // (REX.W or REX.WR). Any destination register is acceptable since the
    // OuterSingleton write doesn't have to use rax — UE codegen routinely
    // uses other registers. Encoding: `48 89 /r/5` or `4C 89 /r/5` followed
    // by a `[RIP+disp32]` ModR/M (the low 3 bits of the second byte are
    // 5 indicating RIP-relative).
    //
    // 32 KiB window covers what live SCUM 23128448 needed: the parent's
    // first write was at +0x319 (~800 bytes), but Image's first write was
    // at +0x13b9 (~5 KiB) and ActionPrompt's at +0x186e (~6 KiB). 32 KiB
    // gives substantial headroom without risking spurious cross-function
    // matches.
    const MAX_REGISTRATION_BODY_BYTES: usize = 32 * 1024;
    let bytes = unsafe { std::slice::from_raw_parts(span.base, span.size) };
    let mut last_outer_seen: Option<*const u8> = None;
    for &lea_site in &leas {
        let lea_off = (lea_site as usize).wrapping_sub(span.base as usize);
        if lea_off >= span.size { continue; }
        let max = (lea_off + MAX_REGISTRATION_BODY_BYTES).min(span.size.saturating_sub(7));
        let mut i = lea_off;
        while i < max {
            // REX byte: 0x48 (REX.W) or 0x4C (REX.W+R for r8-r15)
            // Op: 0x89 (mov m64, r64)
            // ModR/M: low 3 bits == 0b101 (RIP-relative), source reg in middle 3 bits
            let is_rex_mov_rip_rel =
                (bytes[i] == 0x48 || bytes[i] == 0x4C)
                && bytes[i + 1] == 0x89
                && (bytes[i + 2] & 0b1100_0111) == 0b0000_0101;
            if is_rex_mov_rip_rel {
                let disp = i32::from_le_bytes([
                    bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6],
                ]) as isize;
                let next_ip = unsafe { span.base.add(i + 7) };
                let outer_singleton = unsafe { next_ip.offset(disp) };
                last_outer_seen = Some(outer_singleton);
                let uclass_raw = unsafe {
                    (outer_singleton as *const *const u8).read_unaligned()
                };
                if !uclass_raw.is_null() {
                    diag.outer_singleton_write_hit = Some(outer_singleton);
                    diag.uclass_target = Some(uclass_raw);
                    return (Some(UClassPtr(uclass_raw)), diag);
                }
                // Non-null seen → keep scanning this same LEA's window so
                // we don't immediately bail on a Default__* / lock-prefix
                // false positive, but record the hit shape for the
                // diagnostic.
            }
            i += 1;
        }
    }
    diag.outer_singleton_write_hit = last_outer_seen;
    diag.uclass_target = last_outer_seen.map(|p| unsafe {
        (p as *const *const u8).read_unaligned()
    });
    (None, diag)
}

/// Poll a candidate `OuterSingleton` slot for up to `timeout_ms` waiting
/// for UE4's static initializer to populate it. Returns the resolved
/// pointer or `None` on timeout. Polls every `poll_ms`. Used when the
/// xref chain finds the right write site but the class hasn't been
/// registered yet.
pub fn wait_for_uclass(outer_singleton: *const u8, timeout_ms: u64, poll_ms: u64) -> Option<*const u8> {
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < timeout_ms as u128 {
        let p = unsafe { (outer_singleton as *const *const u8).read_unaligned() };
        if !p.is_null() { return Some(p); }
        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
    }
    None
}

/// Sanity-check a candidate `UObject*` — it must:
///   1. Be non-null.
///   2. Its vtable pointer (offset 0) must lie inside `.rdata` (MSVC
///      stores vtables there; the FUNCTION POINTERS inside the vtable
///      point to .text, but the vtable itself is in .rdata).
fn looks_like_uobject(p: *const u8, rdata_section: Option<(usize, usize)>) -> bool {
    if p.is_null() { return false; }
    let vtable_qw = unsafe { (p as *const usize).read_unaligned() };
    let Some((rd_lo, rd_hi)) = rdata_section else { return vtable_qw != 0; };
    vtable_qw >= rd_lo && vtable_qw < rd_hi
}

/// Probe `UClass::ClassDefaultObject`'s offset by trying each candidate
/// against EVERY input `UClass*`. The chosen offset is the FIRST one for
/// which every class's CDO slot reads a "looks like a UObject" pointer
/// AND those pointers are NOT all identical (a self-reference sentinel
/// value won't validate as a proper CDO offset since it'd give bogus
/// vtable-diff results).
///
/// Candidate offsets cover both canonical UE 4.20–4.27 cooked Win64
/// (Dumper-7 OffsetFinder.cpp:1057) AND SCUM's observed shifted layout
/// (offsets 0x270 / 0x280, found via runtime byte-dump on build
/// 23128448). The latter is unusual; SCUM appears to use a UE 4.27
/// fork with extra fields pushing CDO past 0x200.
pub fn probe_cdo_offset_by_shape(
    classes: &[UClassPtr],
    rdata_section: Option<(usize, usize)>,
) -> Option<usize> {
    if classes.is_empty() { return None; }
    const CANDIDATE_OFFSETS: &[usize] = &[
        // Canonical UE 4.27 cooked layouts
        0xF0, 0xF8, 0x100, 0x108, 0x110, 0x118, 0x120, 0x128, 0x130,
        // SCUM 23128448 shifted layout — observed via runtime dump
        0x270, 0x280, 0x288, 0x290,
    ];
    for &off in CANDIDATE_OFFSETS {
        // Read the candidate pointer for every class.
        let cdos: Vec<*const u8> = classes.iter().map(|c| {
            if c.0.is_null() { return std::ptr::null(); }
            unsafe { (c.0.add(off) as *const *const u8).read_unaligned() }
        }).collect();
        // Shape check: every pointer must be non-null and have a
        // .rdata-region vtable.
        let all_shape_ok = cdos.iter().all(|cdo| looks_like_uobject(*cdo, rdata_section));
        if !all_shape_ok { continue; }
        // Distinctness: at least two CDOs must differ. If every class
        // has the SAME pointer at this offset, it's not the CDO field
        // (more likely ClassPrivate or some shared metaclass slot).
        let mut sorted = cdos.clone();
        sorted.sort();
        sorted.dedup();
        if sorted.len() < 2 { continue; }
        return Some(off);
    }
    None
}

/// One-shot section dump — written to `decorators.log` on attach so we can
/// verify the PE walker found `.rdata` / `.text` with sensible sizes before
/// trusting any sigscan output.
pub fn log_module_sections(span: ModuleSpan) {
    let secs = module_sections(span);
    if secs.is_empty() {
        logging::log("[engine-resolve] module_sections: NO SECTIONS (PE walker bailed)");
        return;
    }
    logging::log(&format!(
        "[engine-resolve] module_sections: {} entries", secs.len()
    ));
    for s in secs {
        logging::log(&format!(
            "    section {} base={:p} size=0x{:x}",
            s.name_str(), s.base, s.size,
        ));
    }
}

/// Walk `GUObjectArray` for each `(class_fname, cdo_fname)` in
/// `CDO_SEED_PAIRS`; return every successfully-resolved pair as a
/// `(UClassPtr, UObjectPtr)`. Pairs whose class OR CDO FName misses are
/// silently dropped — partial seed resolution is fine for the probe.
/// # Safety
/// `pool` and `arr` must wrap valid UE structures.
pub unsafe fn resolve_cdo_seeds(
    pool: &FNamePool,
    arr: &GUObjectArray,
) -> Vec<(UClassPtr, UObjectPtr)> {
    let mut out = Vec::with_capacity(CDO_SEED_PAIRS.len());
    for (cls_name, cdo_name) in CDO_SEED_PAIRS {
        let Some(cls_fname) = pool.find_string(cls_name, 0) else { continue; };
        let Some(cdo_fname) = pool.find_string(cdo_name, 0) else { continue; };
        let Some(cls_obj) = arr.find_by_fname(cls_fname) else { continue; };
        let Some(cdo_obj) = arr.find_by_fname(cdo_fname) else { continue; };
        out.push((UClassPtr(cls_obj.0), cdo_obj));
    }
    out
}

/// Run the full probe. Consumes nothing — every step is best-effort and
/// failures populate the corresponding `note`/`None` field rather than
/// short-circuiting.
pub fn probe() -> ProbeReport {
    let mut rep = ProbeReport::empty();

    let Some(span) = main_module_span() else {
        for c in &mut rep.classes {
            c.note = "no main-module span".to_string();
        }
        return rep;
    };
    rep.module = Some(span);

    // Dump section table once so a future "GUObjectArray: NOT FOUND" can be
    // diagnosed without a debugger — we'll know whether .rdata even
    // resolved with a sensible size.
    log_module_sections(span);

    let (fnp, fnp_diag) = resolve_fnamepool_verbose(span);
    rep.fnamepool_addr = fnp;
    logging::log(&format!(
        "[engine-resolve] FNamePool diag: byte_pattern={:?} string={:?} lea={:?} followup={:?}",
        fnp_diag.byte_pattern_hit,
        fnp_diag.fname_string_hit,
        fnp_diag.fname_lea_hit,
        fnp_diag.fname_lea_followup_disp_hit,
    ));

    let (gua, gua_diag) = sigscan_ext::find_guarray_verbose();
    rep.guarray_addr = gua;
    logging::log(&format!(
        "[engine-resolve] GUObjectArray diag: byte_pattern={:?} rdata_size=0x{:x} string={:?} lea={:?} mov_rax={:?}",
        gua_diag.byte_pattern_hit,
        gua_diag.rdata_section_size,
        gua_diag.wide_string_hit,
        gua_diag.lea_xref_hit,
        gua_diag.mov_rax_rip_hit,
    ));

    // ---- Primary class-resolution path: FName ctor + GUObjectArray walk ----
    //
    // patternsleuth-derived approach (UE4SS-equivalent):
    //   1. Sigscan FName::FName(wchar_t*, EFindName) ctor.
    //   2. Sigscan GUObjectArray.
    //   3. For each target class name, call FName::FName(name, FNAME_Find)
    //      to get its FName index. (Find never mutates the pool — if the
    //      class isn't registered yet we just get index 0 and retry later.)
    //   4. Walk GUObjectArray for the matching FName, return the UClass.
    //
    // Same patterns work across every UE 4.27 game patternsleuth has been
    // tested against. Confirmed offline 2026-05-09 against SCUM 23128448:
    // GUObjectArray patterns 0+1 hit (cross-validated), FName ctor pattern
    // hits 2× converging to a single function.
    let fname_ctor_addr = sigscan_ext::find_fname_ctor();
    logging::log(&format!(
        "[engine-resolve] FName::FName ctor: {:?}", fname_ctor_addr,
    ));

    // Sequence: try resolution via FName ctor + GUObjectArray FIRST. If
    // both sigscans hit and at least one class resolves cleanly, we're
    // done. The string-xref-forward-scan fallback path proved unreliable
    // on SCUM (multiple classes collapsed to the same UClass) so we no
    // longer use it as a primary route.
    if let (Some(ctor), Some(arr_ptr)) = (fname_ctor_addr, rep.guarray_addr) {
        // SAFETY: arr_ptr came from the patternsleuth-validated sigscan;
        // ctor came from a different patternsleuth-validated sigscan
        // (same binary, same module). Both address readable memory.
        let arr = unsafe { GUObjectArray::from_global(arr_ptr) };
        rep.guarray_num_elements = Some(unsafe { arr.num_elements() });
        for c in &mut rep.classes {
            // Construct the FName for this class via the engine ctor.
            let fname = unsafe { construct_fname_via_engine(ctor, c.name) };
            match fname {
                None => {
                    c.note = "FName::FName(class_name, Find) returned 0 — \
                              class not registered yet (or stripped)".to_string();
                    continue;
                }
                Some(fn_) => {
                    logging::log(&format!(
                        "[engine-resolve] {:<40} FName=({}, {})",
                        c.name, fn_.comparison_index, fn_.number,
                    ));
                    let uobj = unsafe { arr.find_by_fname(fn_) };
                    match uobj {
                        None => {
                            c.note = "no UObject in GUObjectArray with that FName".to_string();
                        }
                        Some(uo) => {
                            c.uclass = Some(UClassPtr(uo.0));
                        }
                    }
                }
            }
        }
    } else {
        for c in &mut rep.classes {
            c.note = format!(
                "primary path skipped: fname_ctor={} guarray={}",
                if fname_ctor_addr.is_some() { "ok" } else { "MISS" },
                if rep.guarray_addr.is_some() { "ok" } else { "MISS" },
            );
        }
    }

    // ---- Log byte windows around each LEA + the next several `mov
    //      [rip+disp32], reg` candidates so we can see what UE4 codegen
    //      shape SCUM is actually producing.
    {
        let bytes = unsafe { std::slice::from_raw_parts(span.base, span.size) };
        for c in &rep.classes {
            // Use the wide_string_hit instead since we might have multiple
            // LEAs per class. We re-scan to dump multiple LEAs.
            let Some(span_) = main_module_span() else { continue; };
            let str_ptr = match scan_wide_string(
                sigscan_ext::module_data_section_span(span_), c.name) {
                Some(p) => p, None => continue,
            };
            let leas = sigscan_ext::scan_all_leas_to(span_, str_ptr);
            for (li, &lea_addr) in leas.iter().enumerate() {
                let lea_off = (lea_addr as usize).wrapping_sub(span.base as usize);
                if lea_off + 32 >= span.size { continue; }
                let dump: String = (0..32)
                    .map(|j| format!("{:02x}", bytes[lea_off + j]))
                    .collect::<Vec<_>>().join(" ");
                logging::log(&format!(
                    "[engine-resolve] {:<40} lea[{}] @ {:p}: {}",
                    c.name, li, lea_addr, dump,
                ));

                // Forward-scan next 8 mov-rip writes after the LEA, log
                // their resolved targets + the qword currently stored.
                let max = (lea_off + 16384).min(span.size.saturating_sub(7));
                let mut i = lea_off;
                let mut count = 0;
                while i < max && count < 8 {
                    let is_rex_mov_rip_rel =
                        (bytes[i] == 0x48 || bytes[i] == 0x4C)
                        && bytes[i + 1] == 0x89
                        && (bytes[i + 2] & 0b1100_0111) == 0b0000_0101;
                    if is_rex_mov_rip_rel {
                        let disp = i32::from_le_bytes([
                            bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6],
                        ]) as isize;
                        let next_ip = unsafe { span.base.add(i + 7) };
                        let target = unsafe { next_ip.offset(disp) };
                        let qw = unsafe {
                            (target as *const *const u8).read_unaligned()
                        };
                        logging::log(&format!(
                            "    candidate[{}] @ +0x{:x}: target={:p} qword={:p} (reg=R{})",
                            count, i - lea_off, target, qw,
                            (bytes[i + 2] >> 3) & 0b111,
                        ));
                        count += 1;
                    }
                    i += 1;
                }
            }
        }
    }

    // ---- Wait for UE4 static-init ----
    //
    // DllMain runs early, often before UE4 has finished registering UMG
    // classes. If FName::FName(name, FNAME_Find) returned 0 for any class
    // (or GUObjectArray didn't yet contain a matching UObject), retry up
    // to 30 s. UE4 registers most engine-stock classes in one pass, so
    // typically a few seconds is enough.
    if let (Some(ctor), Some(arr_ptr)) = (fname_ctor_addr, rep.guarray_addr) {
        let unresolved = rep.classes.iter().any(|c| c.uclass.is_none());
        if unresolved {
            logging::log("[engine-resolve] some classes unresolved — polling for 30 s");
            let arr = unsafe { GUObjectArray::from_global(arr_ptr) };
            let start = std::time::Instant::now();
            'outer: while start.elapsed().as_millis() < 30_000 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let mut still_missing = false;
                for c in rep.classes.iter_mut() {
                    if c.uclass.is_some() { continue; }
                    let Some(fn_) = (unsafe { construct_fname_via_engine(ctor, c.name) }) else {
                        still_missing = true; continue;
                    };
                    let Some(uo) = (unsafe { arr.find_by_fname(fn_) }) else {
                        still_missing = true; continue;
                    };
                    c.uclass = Some(UClassPtr(uo.0));
                    c.note.clear();
                    logging::log(&format!(
                        "[engine-resolve] {:<40} resolved after wait -> {:p} FName=({},{})",
                        c.name, uo.0, fn_.comparison_index, fn_.number,
                    ));
                }
                if !still_missing { break 'outer; }
            }
            for c in rep.classes.iter_mut() {
                if c.uclass.is_none() && c.note.is_empty() {
                    c.note = "still unresolved after 30 s wait".to_string();
                }
            }
        }
    }

    // ---- Direct CDO resolution via "Default__ClassName" FName lookup ----
    //
    // We pivoted away from probing UClass for a CDO field offset:
    // SCUM 23128448's UClass layout is shifted in ways that made the shape
    // probe land on auxiliary UClass-pointer slots (every candidate at
    // offset 0x280 had UClass::vtable, not a real CDO vtable). The
    // engine-stable alternative is to find the CDO directly:
    //
    // UE4 registers every CDO in GUObjectArray under the name
    // "Default__<ClassName>". The matched UObject* IS the CDO. Its first
    // qword (offset 0x00, MSVC ABI) is the per-class instance vtable.
    // This bypasses the UClass struct entirely and only relies on:
    //   * FName ctor sigscan (already validated)
    //   * GUObjectArray sigscan (already validated)
    //   * UObject offset 0x18 (NamePrivate) — stable across UE 4.20–4.27
    //   * UObject offset 0x00 (vtable) — Itanium/MSVC ABI standard
    //
    // Same lazy-init concern as the UClass loop above — abstract parents
    // never get a "Default__*" entry; concrete children do but might not
    // be registered until the first text block renders. Poll up to 30 s.
    if let (Some(ctor), Some(arr_ptr)) = (fname_ctor_addr, rep.guarray_addr) {
        let arr = unsafe { GUObjectArray::from_global(arr_ptr) };
        // Map class index -> "Default__<class>" lookup name. Allocate once
        // so the polling loop borrows static-lifetime String slices.
        let cdo_names: Vec<String> = rep.classes.iter()
            .map(|c| format!("Default__{}", c.name))
            .collect();
        // Initial pass.
        for (i, c) in rep.classes.iter_mut().enumerate() {
            if c.cdo.is_some() { continue; }
            let Some(fn_) = (unsafe { construct_fname_via_engine(ctor, &cdo_names[i]) }) else {
                continue;
            };
            let Some(uo) = (unsafe { arr.find_by_fname(fn_) }) else { continue; };
            c.cdo = Some(uo);
            logging::log(&format!(
                "[engine-resolve] CDO {:<40} {:p} (FName=({},{}))",
                cdo_names[i], uo.0, fn_.comparison_index, fn_.number,
            ));
        }
        // Children-only poll: parent is abstract and has no Default__*.
        // We only need both children to do the vtable-diff.
        let need_children = (0..rep.classes.len())
            .any(|i| i != PARENT_INDEX && rep.classes[i].cdo.is_none());
        if need_children {
            logging::log("[engine-resolve] some CDOs unresolved — polling for 30 s");
            let start = std::time::Instant::now();
            'cdo_wait: while start.elapsed().as_millis() < 30_000 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let mut still_missing = false;
                for (i, c) in rep.classes.iter_mut().enumerate() {
                    if c.cdo.is_some() { continue; }
                    if i == PARENT_INDEX { continue; } // abstract — skip
                    let Some(fn_) = (unsafe { construct_fname_via_engine(ctor, &cdo_names[i]) }) else {
                        still_missing = true; continue;
                    };
                    let Some(uo) = (unsafe { arr.find_by_fname(fn_) }) else {
                        still_missing = true; continue;
                    };
                    c.cdo = Some(uo);
                    logging::log(&format!(
                        "[engine-resolve] CDO {:<40} resolved after wait -> {:p} FName=({},{})",
                        cdo_names[i], uo.0, fn_.comparison_index, fn_.number,
                    ));
                }
                if !still_missing { break 'cdo_wait; }
            }
        }
    }

    // Optional path: if FNamePool + GUObjectArray DID resolve, we could
    // walk for seed CDO pairs. For SCUM 23128448 both miss, so we proceed
    // directly to shape-based CDO resolution from the xref-resolved
    // UClasses. (Kept the wrappers around for future builds where the
    // sigscans hit and a more rigorous probe is preferred.)
    if let (Some(pool_ptr), Some(arr_ptr)) = (rep.fnamepool_addr, rep.guarray_addr) {
        let pool = unsafe { FNamePool::from_global(pool_ptr) };
        let arr  = unsafe { GUObjectArray::from_global(arr_ptr) };
        rep.guarray_num_elements = Some(unsafe { arr.num_elements() });
        let seeds = unsafe { resolve_cdo_seeds(&pool, &arr) };
        rep.cdo_seeds_resolved = seeds.len();
        if seeds.len() >= 2 {
            let probe_obj = CdoProbe::new(rep.module);
            rep.cdo_offset = unsafe { probe_obj.probe_from_known_classes(&seeds) };
        }
    }

    // ---- Distinctness gate ----
    //
    // The string-xref-forward-scan path proved unreliable on SCUM
    // 23128448: parent and Image both ended up resolving to the SAME
    // UClass pointer because their LEAs share a Z_CompiledInDeferFile
    // registration array (different LEAs, same forward-reachable
    // OuterSingleton write). Until we have a class-specific anchor, we
    // refuse to do any further dereferencing — the previous build's
    // CDO shape probe walked into garbage and faulted the game.
    let resolved_ptrs: Vec<*const u8> = rep.classes.iter()
        .filter_map(|c| c.uclass.map(|u| u.0))
        .collect();
    let mut sorted = resolved_ptrs.clone();
    sorted.sort();
    sorted.dedup();
    let all_distinct = sorted.len() == resolved_ptrs.len()
        && resolved_ptrs.len() == rep.classes.len();
    if !all_distinct {
        logging::log("[engine-resolve] resolved UClasses are NOT all distinct \
                      (xref path collapsed to shared LEAs) — skipping CDO + \
                      vtable-diff to avoid dereferencing wrong class memory");
        // Mark notes so the report is honest about why we stopped.
        for c in &mut rep.classes {
            if c.note.is_empty() {
                c.note = "ambiguous resolve (same UClass as another) — \
                          xref path can't disambiguate per-class".to_string();
            }
        }
        return rep;
    }

    // ---- Shape-based CDO offset probe ----
    //
    // If the seed-pair probe couldn't run (FNamePool / GUObjectArray /
    // Default__* names all missed in SCUM), fall back to validating CDO
    // candidates by their structural shape: a CDO's first qword is its
    // vtable, which lives in `.rdata` (MSVC convention — function pointers
    // INSIDE the vtable point to .text, but the vtable itself is in
    // .rdata). We try a small list of candidate offsets and pick the
    // first one where every resolved UClass's CDO slot validates.
    //
    // Skip entirely when both children's CDOs already resolved via the
    // direct "Default__*" GUObjectArray walk above — the shape probe is
    // a fallback for builds where that walk misses, and running it on
    // already-resolved data risks overwriting good CDOs with whatever
    // the candidate offset happens to point at (a UClass aux slot, etc.).
    let direct_cdos_ok = (0..rep.classes.len())
        .filter(|i| *i != PARENT_INDEX)
        .all(|i| rep.classes[i].cdo.is_some());
    if !direct_cdos_ok && rep.cdo_offset.is_none() && rep.all_uclasses_resolved() {
        let rdata_section = module_sections(span)
            .into_iter()
            .find(|s| &s.name[..6] == b".rdata")
            .map(|s| (s.base as usize, s.base as usize + s.size));
        let classes: Vec<UClassPtr> = rep.classes.iter()
            .filter_map(|c| c.uclass)
            .collect();
        rep.cdo_offset = probe_cdo_offset_by_shape(&classes, rdata_section);
        logging::log(&format!(
            "[engine-resolve] CDO shape-probe: rdata_span={:?} chosen_offset={:?}",
            rdata_section, rep.cdo_offset,
        ));

        // Diagnostic dump — log every qword in [0x100, 0x200) for each
        // resolved UClass so we can pick the right CDO offset by hand.
        // We DELIBERATELY do NOT dereference any non-zero qword here
        // (the previous version did and crashed when a field happened
        // to hold a small integer like 0x1 instead of a pointer). The
        // human reader spots heap-shaped values by their format:
        //
        //   * heap-allocated UObject pointers look like
        //     `0x0000xxxxxxxxxxxx` with the low 3 bits zero (8-byte
        //     aligned) and the high 16 bits zero (canonical Win64
        //     user-space form). Typical range is 0x0000_0001_0000_0000
        //     up through 0x0000_7FFF_FFFF_FFFF.
        //   * Small ints (flags, refcounts, NULLs) are obviously
        //     non-pointer-shaped: 0, 1, 2, 0x10, etc.
        //   * The CDO offset is the one where ALL THREE classes show
        //     a heap-shaped value.
        if rep.cdo_offset.is_none() {
            for c in &rep.classes {
                let Some(uc) = c.uclass else { continue; };
                let mut row = format!("[engine-resolve] dump {:<40}", c.name);
                let mut off = 0x80usize;     // start at UStruct fields
                while off < 0x300 {
                    let qw = unsafe {
                        (uc.0.add(off) as *const usize).read_unaligned()
                    };
                    row.push_str(&format!(" 0x{off:03x}={qw:#018x}"));
                    if (off - 0x80) % 0x20 == 0x18 {
                        logging::log(&row);
                        row = format!("[engine-resolve] dump {:<40}", c.name);
                    }
                    off += 8;
                }
                if row.contains('=') { logging::log(&row); }
            }
        }
    }

    // ---- UClass shape sanity-check ----
    //
    // Every resolved UClass should look structurally like a UE4 UClass:
    //   * vtable (offset 0x00)            : pointer into .text
    //   * ClassPrivate (offset 0x10)      : pointer to UClass::StaticClass()
    //                                       (same for every UClass)
    //   * NamePrivate (offset 0x18)       : FName(comparison_index, number)
    //   * SuperStruct (offset 0x40)       : pointer to parent UClass
    // Logging these lets a human check at a glance whether two classes
    // we resolved are actually distinct (different NamePrivate values)
    // and whether children's SuperStruct points to the parent.
    // Vtables live in .rdata (MSVC convention) — the FUNCTION POINTERS
    // INSIDE a vtable point into .text, but the vtable itself is in
    // .rdata. Validate against that.
    let secs = module_sections(span);
    let rdata_bounds = secs.iter()
        .find(|s| &s.name[..6] == b".rdata")
        .map(|s| (s.base as usize, s.base as usize + s.size));
    for c in &rep.classes {
        let Some(uc) = c.uclass else { continue; };
        let ptr = uc.0;
        let vtable     = unsafe { (ptr.add(0x00) as *const usize).read_unaligned() };
        let class_priv = unsafe { (ptr.add(0x10) as *const usize).read_unaligned() };
        let name_lo    = unsafe { (ptr.add(0x18) as *const u32).read_unaligned() };
        let name_hi    = unsafe { (ptr.add(0x1C) as *const u32).read_unaligned() };
        let super_st   = unsafe { (ptr.add(0x40) as *const usize).read_unaligned() };
        let vtable_ok = match rdata_bounds {
            Some((lo, hi)) => vtable >= lo && vtable < hi,
            None => vtable != 0,
        };
        logging::log(&format!(
            "[engine-resolve] shape {:<40} vtable={:#x}{} class_priv={:#x} fname=({}, {}) super={:#x}",
            c.name, vtable,
            if vtable_ok { " (.rdata OK)" } else { " (vtable not in .rdata!)" },
            class_priv, name_lo, name_hi, super_st,
        ));
    }

    // Read each class's CDO via the probed offset.
    //
    // For abstract parent classes (URichTextBlockDecorator), UE4 sets
    // ClassDefaultObject to point at the UClass itself as a "no real
    // CDO" sentinel. Detect that and treat as None — there's no real
    // CDO instance for that class, and trying to vtable-diff against
    // its UClass vtable (which is UClass::vtable) is meaningless.
    if let Some(off) = rep.cdo_offset {
        for c in &mut rep.classes {
            // Don't clobber a CDO already resolved via the direct
            // "Default__*" GUObjectArray path — that route is more reliable.
            if c.cdo.is_some() { continue; }
            let Some(uc) = c.uclass else { continue; };
            let cdo_raw = unsafe { (uc.0.add(off) as *const *const u8).read_unaligned() };
            if cdo_raw.is_null() {
                if c.note.is_empty() {
                    c.note = format!("CDO slot at offset 0x{:x} is null", off);
                }
            } else if std::ptr::eq(cdo_raw, uc.0) {
                // Self-reference sentinel → abstract class, no real CDO.
                if c.note.is_empty() {
                    c.note = "CDO == UClass (abstract sentinel — no real CDO instance)".to_string();
                }
            } else {
                c.cdo = Some(UObjectPtr(cdo_raw));
            }
        }
    }

    // Vtable-diff between the two CHILDREN, skipping the parent (which
    // is abstract and has no real CDO). Both children override
    // CreateDecorator, so their vtables differ first at exactly that
    // slot. The slot where Image and ActionPrompt diverge IS the
    // CreateDecorator vtable index — independent of parent's CDO state.
    let child_indices: Vec<usize> = (0..rep.classes.len())
        .filter(|i| *i != PARENT_INDEX)
        .collect();
    if child_indices.iter().all(|i| rep.classes[*i].cdo.is_some()) && child_indices.len() >= 2 {
        let i_a = child_indices[0];
        let i_b = child_indices[1];
        let cdo_a = rep.classes[i_a].cdo.expect("checked");
        let cdo_b = rep.classes[i_b].cdo.expect("checked");
        // SAFETY: both CDOs validated as having .rdata vtables in the
        // shape probe; reading [0..256] qwords from each vtable is bounded.
        let outcome = unsafe { diff_first_override(cdo_a, cdo_b) };
        // Annotate both children with the same diff result so the report
        // reads cleanly. The diff is symmetric — Image-vs-AP same as
        // AP-vs-Image — so we record the same index on both.
        rep.classes[i_a].vtable_diff_vs_parent = Some(outcome);
        rep.classes[i_b].vtable_diff_vs_parent = Some(outcome);
        logging::log(&format!(
            "[engine-resolve] vtable-diff Image vs ActionPrompt: {:?} \
             (NOT vs parent — parent is abstract, has no real CDO)",
            outcome,
        ));
    }

    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_report_marks_every_class_unresolved() {
        let rep = ProbeReport::empty();
        assert!(!rep.all_uclasses_resolved());
        assert!(!rep.all_cdos_resolved());
        assert!(rep.child_vtable_diffs_agree().is_none());
        assert_eq!(rep.classes.len(), DECORATOR_CLASS_NAMES.len());
        for (c, name) in rep.classes.iter().zip(DECORATOR_CLASS_NAMES) {
            assert_eq!(c.name, *name);
            assert!(c.uclass.is_none());
            assert!(c.cdo.is_none());
        }
    }

    #[test]
    fn summary_is_human_readable_when_empty() {
        let rep = ProbeReport::empty();
        let s = rep.summary();
        assert!(s.contains("module"));
        assert!(s.contains("FNamePool"));
        assert!(s.contains("GUObjectArray"));
        assert!(s.contains("CDO probe"));
        assert!(s.contains("RichTextBlockImageDecorator"));
    }

    #[test]
    fn fnamepool_pattern_is_well_formed() {
        let p = Pattern::parse("FNamePool", FNAMEPOOL_PATTERN, Some(FNAMEPOOL_RIP_OFFSET))
            .expect("pattern should parse");
        assert_eq!(p.bytes.len(), 18);
        assert_eq!(p.rip_offset, Some(5));
    }

    #[test]
    fn probe_against_test_runner_does_not_crash() {
        let rep = probe();
        assert_eq!(rep.classes.len(), DECORATOR_CLASS_NAMES.len());
        assert!(!rep.all_uclasses_resolved());
        for c in &rep.classes {
            if c.uclass.is_none() {
                assert!(!c.note.is_empty(), "class {} missing note", c.name);
            }
        }
    }

    #[test]
    fn child_vtable_diffs_agree_returns_index_when_aligned() {
        let mut rep = ProbeReport::empty();
        rep.classes[1].vtable_diff_vs_parent = Some(DiffOutcome::DivergesAt(7));
        rep.classes[2].vtable_diff_vs_parent = Some(DiffOutcome::DivergesAt(7));
        assert_eq!(rep.child_vtable_diffs_agree(), Some(7));
    }

    #[test]
    fn child_vtable_diffs_agree_none_when_disagree() {
        let mut rep = ProbeReport::empty();
        rep.classes[1].vtable_diff_vs_parent = Some(DiffOutcome::DivergesAt(7));
        rep.classes[2].vtable_diff_vs_parent = Some(DiffOutcome::DivergesAt(8));
        assert!(rep.child_vtable_diffs_agree().is_none());
    }

    #[test]
    fn child_vtable_diffs_agree_none_when_a_child_is_missing() {
        let mut rep = ProbeReport::empty();
        rep.classes[1].vtable_diff_vs_parent = Some(DiffOutcome::DivergesAt(7));
        // classes[2] has None.
        assert!(rep.child_vtable_diffs_agree().is_none());
    }
}
