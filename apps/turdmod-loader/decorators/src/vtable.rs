//! Vtable-diff probe — locate the index of the first virtual method a child
//! UClass overrides relative to its parent's vtable.
//!
//! Used by [`crate::hooks::install`] to find the vtable index of
//! `URichTextBlockImageDecorator::CreateDecorator()` (and of
//! `URichTextBlockActionPromptDecorator::CreateDecorator()`) without writing a
//! per-class byte signature. The dossier
//! ([docs/scum-internals/16-memory-signatures.md] §"URichTextBlock decorator
//! class CDOs") rejects per-class sigscan as ambiguous and prescribes
//! `GUObjectArray`-by-`FName` lookup + this vtable-diff as the stable path.
//!
//! Algorithm:
//! 1. Caller resolves parent + child `UClass` CDOs via FName/GUObjectArray.
//! 2. We read each CDO's vtable pointer at offset 0 (Itanium/MSVC ABI:
//!    every polymorphic C++ object stores its vtable as the first field;
//!    `UObject`-derived classes follow this).
//! 3. Compare slot-by-slot until they diverge. The first differing slot is
//!    the index of the first virtual method the child overrides — and for
//!    `RichTextBlockDecorator` → `*ImageDecorator` / `*ActionPromptDecorator`
//!    that override is `CreateDecorator`.
//!
//! Status: **candidate, requires runtime probe against SCUM 23128448**. The
//! approach is mechanical; the only drift risk is a silent UE patch that
//! reorders virtual declarations in `URichTextBlockDecorator` between the
//! parent header revision we cited and SCUM's locked engine fork. A
//! sanity-check the next session should add: when both decorator children
//! are resolvable, assert `diff_first_override(parent, image_child)` ==
//! `diff_first_override(parent, actionprompt_child)` (both override the same
//! virtual). If they disagree, refuse to install detours and log loudly.

#![allow(dead_code)]

use crate::uobject::{vtable, UObjectPtr};

/// Cap on how many vtable slots we'll compare. `URichTextBlockDecorator`'s
/// vtable has roughly 30–60 entries; 256 is generous without risking a
/// read past the end of `.rdata` into an adjacent section.
pub const MAX_SLOTS: usize = 256;

/// Outcome of a parent/child vtable comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOutcome {
    /// Vtables match on every slot in `[0, MAX_SLOTS)`. Either the child
    /// overrides nothing in range or the slot count exceeds the cap.
    Identical,
    /// Both pointers were null or equal — degenerate input. Caller should
    /// treat the same as `Identical` for control-flow purposes but the
    /// distinction helps diagnose miswired CDO seeds.
    Degenerate,
    /// First slot at which the child's pointer differs from the parent's.
    /// For `RichTextBlockDecorator → *ImageDecorator` / `*ActionPromptDecorator`
    /// this slot IS the `CreateDecorator` vtable index.
    DivergesAt(usize),
}

impl DiffOutcome {
    /// Convenience: return the override index if one was found.
    pub fn index(self) -> Option<usize> {
        match self {
            DiffOutcome::DivergesAt(i) => Some(i),
            _ => None,
        }
    }
}

/// Read the function pointer at `vtable[index]` for the given CDO.
///
/// # Safety
/// `cdo` must be a live UE4 `UObject` whose vtable (read at offset 0) holds
/// at least `index + 1` valid function-pointer slots. For UE4 decorator
/// CDOs the vtable is in `.rdata`; reading slot 78 is well within bounds.
pub unsafe fn vtable_slot(cdo: UObjectPtr, index: usize) -> Option<*const u8> {
    if cdo.is_null() { return None; }
    let vt = vtable(cdo);
    if vt.is_null() { return None; }
    let slot = vt.add(index).read_unaligned();
    if slot.is_null() { None } else { Some(slot) }
}

/// Compare `parent` and `child` CDO vtables and return the first divergence.
///
/// # Safety
/// Both arguments must be live UE4 `UObject`s (CDOs work; any instance does).
/// Each one's vtable pointer (read at offset 0) must address at least
/// `MAX_SLOTS * 8` readable bytes — true of any vtable emitted by MSVC into
/// `.rdata`. The caller-side flow walks `GUObjectArray` for both pointers,
/// which guarantees both conditions.
pub unsafe fn diff_first_override(parent: UObjectPtr, child: UObjectPtr) -> DiffOutcome {
    if parent.is_null() || child.is_null() {
        return DiffOutcome::Degenerate;
    }
    let pv = vtable(parent);
    let cv = vtable(child);
    if pv.is_null() || cv.is_null() {
        return DiffOutcome::Degenerate;
    }
    if std::ptr::eq(pv, cv) {
        // Shared vtable — child uses parent's verbatim, no overrides at all.
        return DiffOutcome::Degenerate;
    }
    for i in 0..MAX_SLOTS {
        let p = pv.add(i).read_unaligned();
        let c = cv.add(i).read_unaligned();
        if p != c {
            return DiffOutcome::DivergesAt(i);
        }
    }
    DiffOutcome::Identical
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic UObject: 8 bytes whose first qword stores a pointer
    /// to the supplied vtable slice. The vtable is leaked so it outlives the
    /// returned pointer (test scope only).
    fn synth_uobject(vtable_slots: &[*const u8]) -> *const u8 {
        let vtbl: Box<[*const u8]> = vtable_slots.to_vec().into_boxed_slice();
        let vtbl_ptr = Box::leak(vtbl).as_ptr();

        let obj_storage: Box<[u8; 8]> = Box::new([0u8; 8]);
        let obj_ptr = Box::leak(obj_storage).as_mut_ptr();
        unsafe {
            (obj_ptr as *mut *const *const u8).write_unaligned(vtbl_ptr);
        }
        obj_ptr as *const u8
    }

    fn fake_fn(addr: usize) -> *const u8 { addr as *const u8 }

    #[test]
    fn divergence_at_first_slot() {
        let parent_vt = [fake_fn(0x1000), fake_fn(0x1100), fake_fn(0x1200)];
        let child_vt  = [fake_fn(0x9000), fake_fn(0x1100), fake_fn(0x1200)];
        let p = UObjectPtr(synth_uobject(&parent_vt));
        let c = UObjectPtr(synth_uobject(&child_vt));
        unsafe {
            assert_eq!(diff_first_override(p, c), DiffOutcome::DivergesAt(0));
        }
    }

    #[test]
    fn divergence_at_inner_slot() {
        let parent_vt = [fake_fn(0x1000), fake_fn(0x1100), fake_fn(0x1200), fake_fn(0x1300)];
        let child_vt  = [fake_fn(0x1000), fake_fn(0x1100), fake_fn(0x9200), fake_fn(0x1300)];
        let p = UObjectPtr(synth_uobject(&parent_vt));
        let c = UObjectPtr(synth_uobject(&child_vt));
        unsafe {
            assert_eq!(diff_first_override(p, c), DiffOutcome::DivergesAt(2));
        }
    }

    #[test]
    fn identical_vtables_returns_identical() {
        // Different vtable storage but byte-identical contents — should walk
        // the full MAX_SLOTS-bounded range and return Identical.
        let mut slots = Vec::with_capacity(MAX_SLOTS);
        for i in 0..MAX_SLOTS { slots.push(fake_fn(0x1000 + i * 0x10)); }
        let parent_vt = slots.clone();
        let child_vt  = slots.clone();
        let p = UObjectPtr(synth_uobject(&parent_vt));
        let c = UObjectPtr(synth_uobject(&child_vt));
        unsafe {
            assert_eq!(diff_first_override(p, c), DiffOutcome::Identical);
        }
    }

    #[test]
    fn shared_vtable_pointer_returns_degenerate() {
        // Both objects share the same backing vtable storage — UE codegen
        // shape when a child *does not* override anything in range. We treat
        // this as Degenerate so the caller logs and refuses to install.
        let shared_vt = [fake_fn(0x1000), fake_fn(0x1100)];
        let leaked: Box<[*const u8]> = shared_vt.to_vec().into_boxed_slice();
        let vtbl_ptr = Box::leak(leaked).as_ptr();

        let mut obj_a = Box::new([0u8; 8]);
        let mut obj_b = Box::new([0u8; 8]);
        unsafe {
            (obj_a.as_mut_ptr() as *mut *const *const u8).write_unaligned(vtbl_ptr);
            (obj_b.as_mut_ptr() as *mut *const *const u8).write_unaligned(vtbl_ptr);
        }
        let p = UObjectPtr(Box::leak(obj_a).as_ptr());
        let c = UObjectPtr(Box::leak(obj_b).as_ptr());
        unsafe {
            assert_eq!(diff_first_override(p, c), DiffOutcome::Degenerate);
        }
    }

    #[test]
    fn null_inputs_return_degenerate() {
        unsafe {
            assert_eq!(
                diff_first_override(UObjectPtr::null(), UObjectPtr::null()),
                DiffOutcome::Degenerate
            );
        }
    }

    #[test]
    fn outcome_index_helper() {
        assert_eq!(DiffOutcome::DivergesAt(7).index(), Some(7));
        assert_eq!(DiffOutcome::Identical.index(), None);
        assert_eq!(DiffOutcome::Degenerate.index(), None);
    }
}
