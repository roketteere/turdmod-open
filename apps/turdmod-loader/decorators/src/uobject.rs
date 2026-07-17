//! UE 4.27 `UObject` / `UStruct` / `UClass` binary-layout helpers.
//!
//! Source for every offset and the CDO probe heuristic:
//! [docs/scum-internals/22-rich-text-decorator-internals.md], §"Q2 — UClass
//! binary layout in UE 4.27 cooked Win64". The dossier cites the public 4.27
//! mirror header lines and Dumper-7's runtime probe (`OffsetFinder.cpp:1057`)
//! for `ClassDefaultObject`. Cooked Win64 is the only supported target; all
//! field-reader functions are `unsafe` because they raw-deref the input.

#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

// `crate::fname::FName` is the sibling FNamePool module. It's `#[repr(C)]`
// with two `pub u32` fields (`comparison_index`, `number`) — UE 4.27's
// 8-byte FName layout. We construct it field-by-field from the raw qword
// rather than `transmute`, which keeps the dependency surface explicit.
// The integration step adds `mod uobject;` + `mod fname;` to lib.rs; if
// `fname` isn't declared yet the build fails loudly with "could not find
// `fname` in the crate root" — that's intentional, the modules ship together.
use crate::fname::FName;
use crate::sigscan::ModuleSpan;

// Opaque pointer newtypes — keep callers honest about which UE4 layout they
// hold without runtime cost (`#[repr(transparent)]` over `*const u8`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct UObjectPtr(pub *const u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct UClassPtr(pub *const u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct UStructPtr(pub *const u8);

impl UObjectPtr {
    pub const fn null() -> Self { Self(std::ptr::null()) }
    pub fn is_null(self) -> bool { self.0.is_null() }
}
impl UClassPtr {
    pub const fn null() -> Self { Self(std::ptr::null()) }
    pub fn is_null(self) -> bool { self.0.is_null() }
    /// A UClass is-a UObject (and is-a UStruct); widen for base reads.
    pub fn as_uobject(self) -> UObjectPtr { UObjectPtr(self.0) }
    pub fn as_ustruct(self) -> UStructPtr { UStructPtr(self.0) }
}
impl UStructPtr {
    pub fn is_null(self) -> bool { self.0.is_null() }
}

/// UE 4.27 cooked-Win64 field offsets. Every offset is sourced from
/// docs/scum-internals/22-rich-text-decorator-internals.md §Q2 — see that
/// dossier for confidence labels (high vs medium) and per-field header line
/// citations (`Class.h` line numbers).
pub mod offsets {
    // UObjectBase — high (stable across UE4 4.20–4.27).
    pub const UOBJECT_CLASS_PRIVATE: usize = 0x10;  // §Q2 UObjectBase:238
    pub const UOBJECT_NAME_PRIVATE:  usize = 0x18;  // §Q2 UObjectBase:241
    pub const UOBJECT_OUTER_PRIVATE: usize = 0x20;  // §Q2 UObjectBase:244

    // UStruct — high (declarations #1/#2, no editor gates above).
    pub const USTRUCT_SUPER_STRUCT:  usize = 0x40;  // §Q2 UStruct:229
    pub const USTRUCT_CHILDREN:      usize = 0x48;  // §Q2 UStruct:231

    // UClass — medium (CRC sanity recommended; bitfield packing risk).
    pub const UCLASS_CLASS_CONSTRUCTOR: usize = 0x90;  // §Q2 Class.h:1356
    pub const UCLASS_CLASS_FLAGS:       usize = 0xB0;  // §Q2 Class.h:1366
    pub const UCLASS_CLASS_WITHIN:      usize = 0xC0;  // §Q2 Class.h:1372

    // ClassDefaultObject — runtime-probed (Dumper-7 OffsetFinder.cpp:1057).
    pub const CDO_PROBE_LO: usize = 0x28;
    pub const CDO_PROBE_HI: usize = 0x200;
}

/// Unaligned-safe pointer read at `base + off`.
/// # Safety: `base + off + 8` must be in live readable memory.
#[inline]
unsafe fn read_ptr_at(base: *const u8, off: usize) -> *const u8 {
    (base.add(off) as *const *const u8).read_unaligned()
}

/// Unaligned-safe u64 read at `base + off`.
/// # Safety: `base + off + 8` must be in live readable memory.
#[inline]
unsafe fn read_u64_at(base: *const u8, off: usize) -> u64 {
    (base.add(off) as *const u64).read_unaligned()
}

/// FName at offset 0x18 of any UObject. The 8-byte FName slot is
/// `ComparisonIndex (u32, low) | Number (u32, high)` on cooked Win64.
/// # Safety: `obj` must be a live UE4 `UObject` (≥0x28 readable bytes).
pub unsafe fn name_private(obj: UObjectPtr) -> FName {
    let raw = read_u64_at(obj.0, offsets::UOBJECT_NAME_PRIVATE);
    FName {
        comparison_index: raw as u32,
        number: (raw >> 32) as u32,
    }
}

/// `UClass*` (ClassPrivate) at offset 0x10.
/// # Safety: `obj` must be a live UE4 `UObject`.
pub unsafe fn class_of(obj: UObjectPtr) -> UClassPtr {
    UClassPtr(read_ptr_at(obj.0, offsets::UOBJECT_CLASS_PRIVATE))
}

/// Outer `UObject*` (OuterPrivate) at offset 0x20.
/// # Safety: `obj` must be a live UE4 `UObject`.
pub unsafe fn outer_of(obj: UObjectPtr) -> UObjectPtr {
    UObjectPtr(read_ptr_at(obj.0, offsets::UOBJECT_OUTER_PRIVATE))
}

/// Parent `UClass*` (SuperStruct) at offset 0x40.
/// # Safety: `class` must be a live UE4 `UClass`/`UStruct`.
pub unsafe fn super_struct(class: UClassPtr) -> UClassPtr {
    UClassPtr(read_ptr_at(class.0, offsets::USTRUCT_SUPER_STRUCT))
}

/// C++ vtable pointer of any UE4 `UObject` (offset 0x0). The Itanium/MSVC
/// ABI places the vtable as the first field of any polymorphic C++ object;
/// `UObject` inherits this, so every UObject's vtable is at offset 0.
/// # Safety: `obj` must be a live UE4 `UObject`.
pub unsafe fn vtable(obj: UObjectPtr) -> *const *const u8 {
    (obj.0 as *const *const *const u8).read_unaligned()
}

/// `ClassDefaultObject` runtime probe.
///
/// Heuristic per Dumper-7 `OffsetFinder.cpp:1057`: for each candidate offset
/// in `[0x28, 0x200)` step 8, read the qword at `class + off` for every
/// known `(UClass*, expected_CDO*)` pair. Return the offset where every
/// pair's qword equals its expected CDO. With ≥2 distinct pairs a false
/// positive is essentially impossible.
///
/// Failure modes:
/// 1. *Bad seed* — a mis-paired `(class, cdo)` causes no offset to match.
/// 2. *Engine drift* — UE4 hotfix that changes bitfield packing around
///    `ClassCastFlags` shifts the CDO field; probe returns `None`. Re-pin.
/// 3. *Module-scope mismatch* — an in-module check rejects candidate
///    pointers that fall outside SCUM.exe (when a `ModuleSpan` is wired in).
pub struct CdoProbe {
    cdo_offset: AtomicUsize,
    /// Module-span of SCUM.exe. Used to reject obviously-wrong candidate
    /// pointers (those outside the module). `None` skips the in-module
    /// check (acceptable when the seed has ≥2 high-quality pairs).
    module: Option<ModuleSpan>,
}

const CDO_OFFSET_UNRESOLVED: usize = usize::MAX;

impl CdoProbe {
    /// Construct an unresolved probe. Use `probe_from_known_classes` to
    /// fill in the offset before any `class_default_object` call.
    pub fn new(module: Option<ModuleSpan>) -> Self {
        Self { cdo_offset: AtomicUsize::new(CDO_OFFSET_UNRESOLVED), module }
    }

    /// Returns the resolved offset, or `None` if probing hasn't succeeded.
    pub fn resolved_offset(&self) -> Option<usize> {
        match self.cdo_offset.load(Ordering::Acquire) {
            CDO_OFFSET_UNRESOLVED => None,
            v => Some(v),
        }
    }

    /// Run Dumper-7's pair-match probe. Each input is `(UClass*, &CDO)`
    /// — pass 2-3 well-known classes (e.g. `UObject`, `UField`) and their
    /// CDOs (`Default__Object`, `Default__Field`) resolved via FName lookup.
    /// On success the resolved offset is cached and returned.
    /// # Safety
    /// Each `class` pointer must be a live UE4 `UClass` with ≥`CDO_PROBE_HI`
    /// readable bytes — the probe reads every 8-byte slot in the range.
    pub unsafe fn probe_from_known_classes(
        &self,
        pairs: &[(UClassPtr, UObjectPtr)],
    ) -> Option<usize> {
        if pairs.is_empty() { return None; }
        let mut off = offsets::CDO_PROBE_LO;
        while off + 8 <= offsets::CDO_PROBE_HI {
            let mut all_match = true;
            for (class, expected) in pairs {
                let candidate = read_ptr_at(class.0, off);
                if candidate != expected.0 || !self.candidate_in_module(candidate) {
                    all_match = false;
                    break;
                }
            }
            if all_match {
                self.cdo_offset.store(off, Ordering::Release);
                return Some(off);
            }
            off += 8;
        }
        None
    }

    /// Bounds-check a candidate pointer against the configured module span.
    /// When no span is configured we accept any non-null pointer.
    fn candidate_in_module(&self, p: *const u8) -> bool {
        if p.is_null() { return false; }
        let Some(span) = self.module else { return true; };
        let base = span.base as usize;
        let pa = p as usize;
        pa >= base && pa < base.saturating_add(span.size)
    }
}

/// Fetch `UClass::ClassDefaultObject` using a previously-probed offset.
/// Returns `None` if the probe hasn't resolved or the slot is null.
/// # Safety
/// `class` must be a live UE4 `UClass` with ≥`CDO_PROBE_HI` readable bytes.
pub unsafe fn class_default_object(class: UClassPtr, probe: &CdoProbe) -> Option<UObjectPtr> {
    let off = probe.resolved_offset()?;
    let p = read_ptr_at(class.0, off);
    if p.is_null() { None } else { Some(UObjectPtr(p)) }
}

// Tests use mock buffers only; no UE4 dependency.
#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic UObject buffer with controlled bytes at the high-confidence
    /// offsets. Heap-allocated so pointer reads are well-defined.
    fn mock_uobject() -> Box<[u8; 0x40]> {
        let mut buf = Box::new([0u8; 0x40]);
        // ClassPrivate @ 0x10 — a synthetic non-null pointer pattern
        buf[0x10..0x18].copy_from_slice(&0xDEAD_BEEF_CAFE_BABEu64.to_le_bytes());
        // NamePrivate @ 0x18 — index = 0x42, number = 0x0
        buf[0x18..0x20].copy_from_slice(&0x0000_0000_0000_0042u64.to_le_bytes());
        // OuterPrivate @ 0x20 — synthetic pointer
        buf[0x20..0x28].copy_from_slice(&0x1111_2222_3333_4444u64.to_le_bytes());
        buf
    }

    #[test]
    fn name_private_reads_offset_0x18() {
        // mock_uobject lays comparison_index = 0x42, number = 0x0 at 0x18.
        let buf = mock_uobject();
        let n = unsafe { name_private(UObjectPtr(buf.as_ptr())) };
        assert_eq!(n.comparison_index, 0x42);
        assert_eq!(n.number, 0);
    }

    #[test]
    fn class_of_reads_offset_0x10() {
        let buf = mock_uobject();
        let c = unsafe { class_of(UObjectPtr(buf.as_ptr())) };
        assert_eq!(c.0 as u64, 0xDEAD_BEEF_CAFE_BABE);
    }

    #[test]
    fn vtable_returns_first_qword() {
        // Place a "vtable pointer" at offset 0 that points to a fake vtable.
        let fake_vt: [usize; 4] = [0xAAAA, 0xBBBB, 0xCCCC, 0xDDDD];
        let mut buf = vec![0u8; 0x20];
        let vt_addr = fake_vt.as_ptr() as u64;
        buf[0..8].copy_from_slice(&vt_addr.to_le_bytes());
        let vt = unsafe { vtable(UObjectPtr(buf.as_ptr())) };
        assert_eq!(vt as u64, vt_addr);
        assert_eq!(unsafe { *vt } as u64, 0xAAAA);
    }

    #[test]
    fn cdo_probe_finds_offset_when_pair_matches() {
        // Lay an "expected CDO pointer" at offset 0x40 of two synthetic
        // class buffers and confirm the probe returns 0x40.
        let mut class_a = vec![0u8; 0x200];
        let mut class_b = vec![0u8; 0x200];
        let cdo_a: u64 = 0xAAAA_BBBB_CCCC_DDDD;
        let cdo_b: u64 = 0x1111_2222_3333_4444;
        class_a[0x40..0x48].copy_from_slice(&cdo_a.to_le_bytes());
        class_b[0x40..0x48].copy_from_slice(&cdo_b.to_le_bytes());

        let probe = CdoProbe::new(None); // skip in-module check
        let pairs = [
            (UClassPtr(class_a.as_ptr()), UObjectPtr(cdo_a as *const u8)),
            (UClassPtr(class_b.as_ptr()), UObjectPtr(cdo_b as *const u8)),
        ];
        assert_eq!(unsafe { probe.probe_from_known_classes(&pairs) }, Some(0x40));
        assert_eq!(probe.resolved_offset(), Some(0x40));
    }

    #[test]
    fn cdo_probe_returns_none_when_no_offset_matches() {
        let class_a = vec![0u8; 0x200];
        let class_b = vec![0u8; 0x200];
        let probe = CdoProbe::new(None);
        let pairs = [
            (UClassPtr(class_a.as_ptr()), UObjectPtr(0x1234 as *const u8)),
            (UClassPtr(class_b.as_ptr()), UObjectPtr(0x5678 as *const u8)),
        ];
        assert_eq!(unsafe { probe.probe_from_known_classes(&pairs) }, None);
        assert_eq!(probe.resolved_offset(), None);
    }
}
