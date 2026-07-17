//! `FUObjectArray` walker — resolves `UClass*` pointers by `FName` lookup.
//!
//! Companion to [`crate::fname`] and [`crate::uobject`]. The decorator DLL
//! needs `URichTextBlockImageDecorator::StaticClass()` and
//! `URichTextBlockActionPromptDecorator::StaticClass()` (plus the
//! `URichTextBlockDecorator` parent for the [`crate::vtable`] diff probe);
//! per [docs/scum-internals/16-memory-signatures.md] §"Recommended primary
//! approach", a `GUObjectArray`-by-`FName` walk is the stable way to get
//! those without per-class byte signatures (UE codegen makes per-class
//! `StaticClass()` byte patterns ambiguous).
//!
//! Layout from UE 4.27 release branch
//! `Engine/Source/Runtime/CoreUObject/Public/UObject/UObjectArray.h`:
//!
//! ```text
//! class FUObjectArray
//! {
//!     int32 ObjFirstGCIndex;                 //  +0x00
//!     int32 ObjLastNonGCIndex;               //  +0x04
//!     int32 MaxObjectsNotConsideredByGC;     //  +0x08
//!     bool  OpenForDisregardForGC;           //  +0x0C
//!     FChunkedFixedUObjectArray ObjObjects;  //  +0x10
//!     // …locks, deletion list, etc.
//! };
//!
//! class FChunkedFixedUObjectArray
//! {
//!     FUObjectItem** Objects;     //  +0x00 within ObjObjects (=> +0x10 in FUObjectArray)
//!     FUObjectItem*  PreAllocatedObjects;  //  +0x08
//!     int32 MaxElements;          //  +0x10
//!     int32 NumElements;          //  +0x14
//!     int32 MaxChunks;            //  +0x18
//!     int32 NumChunks;            //  +0x1C
//! };
//!
//! struct FUObjectItem // 24 bytes on cooked Win64
//! {
//!     UObjectBase* Object;        //  +0x00
//!     int32 Flags;                //  +0x08
//!     int32 ClusterRootIndex;     //  +0x0C
//!     int32 SerialNumber;         //  +0x10
//!     // 4 bytes pad to 0x18
//! };
//! ```
//!
//! `NumElementsPerChunk` is a public constexpr inside
//! `FChunkedFixedUObjectArray` (=`64 * 1024 = 65536` for 4.27). Each chunk
//! is `NumElementsPerChunk * sizeof(FUObjectItem)` = 1.5 MiB.
//!
//! Status: **candidate, requires runtime probe against SCUM 23128448** —
//! the offset constants below match the public 4.27 mirror but UE has
//! shifted GC-bookkeeping fields across minor releases. Validate by walking
//! a known FName (e.g. `"None"` or `"Object"`) on first attach; if the
//! lookup misses, refuse to install detours and log loudly. Same convention
//! as [`crate::fname`] / [`crate::uobject`].

#![allow(dead_code)]

use crate::fname::FName;
use crate::uobject::{class_of, name_private, UClassPtr, UObjectPtr};

// per UE 4.27 UObjectArray.h — FUObjectArray::ObjObjects starts at 0x10.
// CANDIDATE.
const FUOBJ_ARRAY_OBJOBJECTS_OFFSET: usize = 0x10;

// Within FChunkedFixedUObjectArray (at FUObjectArray + 0x10):
const FCHUNKED_OBJECTS_PTR_OFFSET: usize = 0x00;     // FUObjectItem**
const FCHUNKED_NUM_ELEMENTS_OFFSET: usize = 0x14;    // int32
const FCHUNKED_NUM_CHUNKS_OFFSET: usize = 0x1C;      // int32

const FUOBJ_ITEM_SIZE: usize = 0x18;
const FUOBJ_ITEM_OBJECT_OFFSET: usize = 0x00;

/// Per UE 4.27 `FChunkedFixedUObjectArray::NumElementsPerChunk`.
const NUM_ELEMENTS_PER_CHUNK: usize = 64 * 1024;

/// Hard cap on objects we'll walk. SCUM's runtime UObject count is roughly
/// 700k–1.2M (per dossier 18); 4M is comfortably above that and bounds the
/// scan if the live count looks corrupted.
const MAX_OBJECTS_SAFETY_CAP: usize = 4 * 1024 * 1024;

/// Opaque wrapper around the resolved `FUObjectArray*`. Construct via
/// [`crate::sigscan_ext::find_guarray`].
#[derive(Clone, Copy)]
pub struct GUObjectArray(pub *const u8);

unsafe impl Send for GUObjectArray {}
unsafe impl Sync for GUObjectArray {}

impl GUObjectArray {
    /// # Safety
    /// `ptr` must point at a fully-constructed UE 4.27 `FUObjectArray` for
    /// the wrapper's lifetime.
    pub unsafe fn from_global(ptr: *const u8) -> Self { Self(ptr) }

    /// Currently-live object count — read from `ObjObjects.NumElements`,
    /// clamped to `MAX_OBJECTS_SAFETY_CAP` to defend against a corrupted
    /// header (or wrong sigscan match).
    pub unsafe fn num_elements(&self) -> usize {
        let chunked = self.0.add(FUOBJ_ARRAY_OBJOBJECTS_OFFSET);
        let n = (chunked.add(FCHUNKED_NUM_ELEMENTS_OFFSET) as *const i32).read_unaligned();
        if n <= 0 { return 0; }
        (n as usize).min(MAX_OBJECTS_SAFETY_CAP)
    }

    /// Iterate every live `UObject*` in `[0, num_elements())`, calling
    /// `visit` until it returns `Some(_)` (which is then returned). Returns
    /// `None` if the walk completes without a hit.
    ///
    /// # Safety
    /// `self` must wrap a valid `FUObjectArray`. The visit closure must not
    /// retain raw pointers past the GC tick that runs after the walk.
    pub unsafe fn walk<T, F>(&self, mut visit: F) -> Option<T>
    where F: FnMut(UObjectPtr) -> Option<T>,
    {
        let chunked = self.0.add(FUOBJ_ARRAY_OBJOBJECTS_OFFSET);
        let chunks_ptr = (chunked.add(FCHUNKED_OBJECTS_PTR_OFFSET) as *const *const *const u8)
            .read_unaligned();
        if chunks_ptr.is_null() { return None; }

        let total = self.num_elements();
        if total == 0 { return None; }

        let mut idx: usize = 0;
        while idx < total {
            let chunk_idx = idx / NUM_ELEMENTS_PER_CHUNK;
            let in_chunk = idx % NUM_ELEMENTS_PER_CHUNK;
            let chunk_base = (chunks_ptr.add(chunk_idx)).read_unaligned();
            if chunk_base.is_null() { break; }
            let item = chunk_base.add(in_chunk * FUOBJ_ITEM_SIZE);
            let obj = (item.add(FUOBJ_ITEM_OBJECT_OFFSET) as *const *const u8).read_unaligned();
            idx += 1;
            if obj.is_null() { continue; }   // GC tombstone — keep going.
            if let Some(t) = visit(UObjectPtr(obj)) { return Some(t); }
        }
        None
    }

    /// Find the first object whose `NamePrivate` equals `target`. Useful for
    /// resolving a single class CDO / class-object by name.
    /// # Safety: see [`Self::walk`].
    pub unsafe fn find_by_fname(&self, target: FName) -> Option<UObjectPtr> {
        self.walk(|obj| if name_private(obj) == target { Some(obj) } else { None })
    }

    /// Find the `UClass*` for a class whose `FName` matches `target`. Filters
    /// candidates via `class_filter` so callers can reject false positives
    /// (e.g. require `class_of(obj) == UClass::StaticClass()` once that is
    /// resolved). The first object passing both predicates is returned as a
    /// `UClassPtr`. Identical bytewise to [`Self::find_by_fname`] for
    /// straightforward use; the filter exists so the next-step caller can
    /// disambiguate when two unrelated UObjects share an FName.
    /// # Safety: see [`Self::walk`].
    pub unsafe fn find_class_by_fname<F>(&self, target: FName, mut class_filter: F) -> Option<UClassPtr>
    where F: FnMut(UObjectPtr) -> bool,
    {
        self.walk(|obj| {
            if name_private(obj) != target { return None; }
            if !class_filter(obj) { return None; }
            // The matched UObject IS the UClass instance — `UClass*` is just a
            // typed alias for the same address.
            Some(UClassPtr(obj.0))
        })
    }
}

/// Count UObjects whose `ClassPrivate` (offset 0x10) equals `target_class`.
/// Direct equality check — does NOT walk subclasses. For "is X or any
/// subclass of X" use [`count_subclass_instances`].
///
/// Useful for detecting whether a leaf decorator class (e.g. URichText-
/// BlockImageDecorator) has live instances in the current game state, which
/// indirectly confirms whether the corresponding `CreateDecorator` detour
/// should be firing.
///
/// # Safety: see [`Self::walk`].
pub unsafe fn count_class_instances(arr: &GUObjectArray, target_class: UClassPtr) -> usize {
    let mut n = 0usize;
    let _: Option<()> = arr.walk(|obj| {
        if class_of(obj) == target_class { n += 1; }
        None // never short-circuit — we want to count every match.
    });
    n
}

/// Count UObjects whose class is `ancestor` OR any subclass of it. Walks
/// the `UStruct::SuperStruct` chain (cap at 32 hops to bound corruption-
/// induced infinite loops).
///
/// Useful for "how many URichTextBlock instances exist" — SCUM may subclass
/// the parent in BP/native code, and a direct equality check would miss
/// those.
///
/// # Safety: see [`Self::walk`].
pub unsafe fn count_subclass_instances(arr: &GUObjectArray, ancestor: UClassPtr) -> usize {
    use crate::uobject::super_struct;
    const MAX_HOPS: usize = 32;
    let mut n = 0usize;
    let _: Option<()> = arr.walk(|obj| {
        let mut cur = class_of(obj);
        let mut hops = 0;
        while !cur.is_null() && hops < MAX_HOPS {
            if cur == ancestor { n += 1; break; }
            cur = super_struct(cur);
            hops += 1;
        }
        None
    });
    n
}

/// Convenience: when the caller already has the FName matching a class name,
/// trust it and skip the per-object class filter. Use this for quick
/// resolution of well-known UE engine classes; for SCUM-game classes that
/// might collide with non-class FNames, prefer [`GUObjectArray::find_class_by_fname`]
/// with a filter that asserts the object's `class_of()` is itself a UClass.
/// # Safety
/// `arr` must wrap a valid `FUObjectArray`.
pub unsafe fn resolve_class_unchecked(arr: &GUObjectArray, target: FName) -> Option<UClassPtr> {
    arr.find_by_fname(target).map(|o| UClassPtr(o.0))
}

/// Build a class filter that accepts only objects whose own class is the
/// metaclass. Pass `metaclass_uclass` = the resolved `UClass*` for `"Class"`
/// (i.e. `UClass::StaticClass()`). Returned closure can be handed to
/// [`GUObjectArray::find_class_by_fname`].
pub fn is_class_metaclass_filter(metaclass_uclass: UClassPtr) -> impl FnMut(UObjectPtr) -> bool {
    move |obj| {
        // SAFETY: caller-side wrapping flow already guarantees obj is a live
        // UObject — `class_of` reads one pointer-sized field at offset 0x10.
        let cls = unsafe { class_of(obj) };
        cls == metaclass_uclass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic `FUObjectArray` + one chunk + N items pointing at N
    /// synthetic UObjects whose offset-0x18 FName is the i-th of `names`.
    /// Returns `(arr_ptr, _keep_alive)`. The `_keep_alive` Vec must outlive
    /// any walks of the returned pointer — drop ends the test.
    struct Fixture {
        _items: Vec<u8>,
        _chunk_ptrs: Vec<*const u8>,
        _objects: Vec<Box<[u8; 0x28]>>,
        _arr: Vec<u8>,
        arr_ptr: *const u8,
    }

    fn build_fixture(names: &[FName]) -> Fixture {
        // Synthetic UObject = 0x28 bytes; we only populate offsets 0x10
        // (ClassPrivate, leave null) and 0x18 (NamePrivate u64).
        let mut objects: Vec<Box<[u8; 0x28]>> = Vec::with_capacity(names.len());
        let mut object_ptrs: Vec<*const u8> = Vec::with_capacity(names.len());
        for n in names {
            let mut buf: Box<[u8; 0x28]> = Box::new([0u8; 0x28]);
            let raw = ((n.number as u64) << 32) | (n.comparison_index as u64);
            unsafe {
                (buf.as_mut_ptr().add(0x18) as *mut u64).write_unaligned(raw);
            }
            object_ptrs.push(buf.as_ptr());
            objects.push(buf);
        }

        // One chunk: a `[FUObjectItem; names.len()]` flat byte buffer.
        let mut items_bytes = vec![0u8; FUOBJ_ITEM_SIZE * names.len()];
        for (i, op) in object_ptrs.iter().enumerate() {
            unsafe {
                (items_bytes.as_mut_ptr().add(i * FUOBJ_ITEM_SIZE) as *mut *const u8)
                    .write_unaligned(*op);
            }
        }
        let chunk_base: *const u8 = items_bytes.as_ptr();

        // Chunk-pointer table holding one chunk.
        let chunk_ptrs: Vec<*const u8> = vec![chunk_base];

        // FUObjectArray bytes — only offsets we read are 0x10..0x10+0x20.
        let mut arr = vec![0u8; 0x40];
        unsafe {
            // ObjObjects.Objects = chunk_ptrs.as_ptr()
            (arr.as_mut_ptr().add(FUOBJ_ARRAY_OBJOBJECTS_OFFSET + FCHUNKED_OBJECTS_PTR_OFFSET)
                as *mut *const *const u8)
                .write_unaligned(chunk_ptrs.as_ptr());
            // ObjObjects.NumElements = names.len()
            (arr.as_mut_ptr().add(FUOBJ_ARRAY_OBJOBJECTS_OFFSET + FCHUNKED_NUM_ELEMENTS_OFFSET)
                as *mut i32)
                .write_unaligned(names.len() as i32);
        }
        let arr_ptr = arr.as_ptr();

        Fixture { _items: items_bytes, _chunk_ptrs: chunk_ptrs, _objects: objects, _arr: arr, arr_ptr }
    }

    fn fname(c: u32, n: u32) -> FName { FName { comparison_index: c, number: n } }

    #[test]
    fn num_elements_reads_count_field() {
        let fx = build_fixture(&[fname(1, 0), fname(2, 0), fname(3, 0)]);
        unsafe {
            let arr = GUObjectArray::from_global(fx.arr_ptr);
            assert_eq!(arr.num_elements(), 3);
        }
    }

    #[test]
    fn walk_visits_every_object_in_order() {
        let fx = build_fixture(&[fname(10, 0), fname(20, 0), fname(30, 0)]);
        let mut seen = Vec::new();
        unsafe {
            let arr = GUObjectArray::from_global(fx.arr_ptr);
            let _: Option<()> = arr.walk(|obj| {
                seen.push(name_private(obj));
                None
            });
        }
        assert_eq!(seen, vec![fname(10, 0), fname(20, 0), fname(30, 0)]);
    }

    #[test]
    fn find_by_fname_returns_first_match() {
        let target = fname(42, 0);
        let fx = build_fixture(&[fname(1, 0), target, fname(2, 0), target]);
        unsafe {
            let arr = GUObjectArray::from_global(fx.arr_ptr);
            let found = arr.find_by_fname(target).expect("should hit");
            assert_eq!(name_private(found), target);
        }
    }

    #[test]
    fn find_by_fname_returns_none_when_absent() {
        let fx = build_fixture(&[fname(1, 0), fname(2, 0)]);
        unsafe {
            let arr = GUObjectArray::from_global(fx.arr_ptr);
            assert!(arr.find_by_fname(fname(999, 0)).is_none());
        }
    }

    #[test]
    fn find_class_by_fname_respects_filter() {
        let target = fname(42, 0);
        let fx = build_fixture(&[target, target, target]);
        // Reject the first match, accept the second — pure-logic test of the
        // filter wiring (the live-binary version filters on metaclass).
        let mut seen_count = 0u32;
        unsafe {
            let arr = GUObjectArray::from_global(fx.arr_ptr);
            let cls = arr.find_class_by_fname(target, |_| {
                seen_count += 1;
                seen_count == 2
            });
            assert!(cls.is_some());
        }
    }

    #[test]
    fn empty_array_walks_to_none() {
        let fx = build_fixture(&[]);
        unsafe {
            let arr = GUObjectArray::from_global(fx.arr_ptr);
            assert_eq!(arr.num_elements(), 0);
            let r: Option<UObjectPtr> = arr.walk(|_| Some(UObjectPtr::null()));
            assert!(r.is_none());
        }
    }
}
