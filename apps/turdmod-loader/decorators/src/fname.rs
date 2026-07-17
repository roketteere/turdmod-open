//! UE 4.27 `FName` + `FNamePool` lookup. Used by `hooks::install` to walk
//! `GUObjectArray` and find the `UClass*` for `RichTextBlockImageDecorator`
//! etc. without per-class byte signatures.
//!
//! Layout from the UE 4.27 release branch under
//! `Engine/Source/Runtime/Core/{Public,Private}/UObject/`:
//!   * `NameTypes.h:~498`: `FName { ComparisonIndex: u32, Number: u32 }`.
//!   * `NameTypes.h:~80`:  `FNameEntryHandle` packs `(Block, Offset)`;
//!     `MaxBlockBits=13`, `BlockOffsetBits=16`, `EntryAlignment=2`.
//!   * `UnrealNames.cpp:~1110`: `FNamePool` = lock+cursor prelude then
//!     `void* Blocks[FNameMaxBlocks]`; each block is 64 KiB.
//!   * `NameTypes.cpp:~250`: `FNameEntryHeader { bIsWide:1; ProbeHash:5;
//!     Len:10 }` little-endian u16; ASCII or UTF-16 body follows; padded
//!     up to 2-byte alignment before the next entry.
//!
//! Status: **candidate, requires runtime probe against SCUM 23128448** —
//! the FNamePool prelude (`FRWLock`+cursor) has shifted across UE minor
//! releases. Promote to "verified" once a sigscan-resolved pool round-trips
//! a known FName (e.g. `"None"` → `comparison_index=0`). Same convention
//! as 16-memory-signatures.md.

#![allow(dead_code)]

use std::hash::{Hash, Hasher};

// per UE 4.27 NameTypes.h:~75-85 — FNameBlockOffsetBits=16, MaxBlockBits=13,
// FNameEntryAlignment=2 → block size = (1 << 16) * 2 = 64 KiB.
const FNAME_BLOCK_OFFSET_BITS: u32 = 16;
const FNAME_BLOCK_OFFSET_MASK: u32 = (1 << FNAME_BLOCK_OFFSET_BITS) - 1;
const FNAME_ENTRY_STRIDE: u32 = 2;
const FNAME_BLOCK_SIZE_BYTES: usize =
    (1usize << FNAME_BLOCK_OFFSET_BITS) * FNAME_ENTRY_STRIDE as usize;
const FNAME_MAX_BLOCKS: usize = 1 << 13;
// per UE 4.27 UnrealNames.cpp:~1110 — pool prelude then Blocks[].
// CANDIDATE — exact size depends on Win64 SRWLOCK/cursor packing.
const FNAMEPOOL_BLOCKS_OFFSET: usize = 0x10;
// per UE 4.27 NameTypes.cpp:~250 — packed u16 header { bIsWide:1; Hash:5; Len:10 }.
const FNAME_ENTRY_LEN_MASK: u16 = 0x03FF;
const FNAME_ENTRY_LEN_SHIFT: u32 = 6;
const FNAME_ENTRY_WIDE_BIT: u16 = 1 << 0;
const FNAME_ENTRY_HEADER_BYTES: usize = 2;

/// UE 4.27 `FName` — 8 bytes. Two FNames are equal iff
/// `(comparison_index, number)` match; never compare strings. Hash and Eq
/// use the same fields so FNames work as `HashMap` keys without a pool
/// round-trip.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FName {
    /// `FNameEntryHandle` packing `(block, offset_in_strides)` — see `decode_handle`.
    pub comparison_index: u32,
    /// Suffix. Displayed as `"Foo"` when `Number==0`, else `"Foo_{Number-1}"`.
    pub number: u32,
}

impl PartialEq for FName {
    fn eq(&self, other: &Self) -> bool {
        self.comparison_index == other.comparison_index && self.number == other.number
    }
}
impl Eq for FName {}

impl Hash for FName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the fields used for equality — never the resolved string.
        self.comparison_index.hash(state);
        self.number.hash(state);
    }
}

/// Opaque wrapper around a real UE `FNamePool` global.
pub struct FNamePool(*const u8);

// FNamePool is a process-wide global; UE locks it internally.
unsafe impl Send for FNamePool {}
unsafe impl Sync for FNamePool {}

impl FNamePool {
    /// # Safety
    /// `ptr` must point at a fully-constructed UE 4.27 `FNamePool` for the
    /// wrapper's lifetime.
    pub unsafe fn from_global(ptr: *const u8) -> Self { Self(ptr) }

    /// Find the FName whose stored body matches `name` (case-sensitive,
    /// ASCII or UTF-16) and tag it with `number`. O(N) over the pool —
    /// callers should cache results across the process lifetime. UE's
    /// FName lookup is case-insensitive but stored bodies preserve
    /// first-seen case, which is what we need for matching UClass names.
    pub fn find_string(&self, name: &str, number: u32) -> Option<FName> {
        // SAFETY: wrapper invariant — self.0 is a real pool.
        unsafe { walk_blocks(self.0, |h, b| (b.into_string() == name).then_some(h)) }
            .map(|comparison_index| FName { comparison_index, number })
    }

    /// Reverse-lookup: decode the entry at `name.comparison_index` to a
    /// String. The `Number` suffix is NOT applied.
    pub fn resolve(&self, name: FName) -> Option<String> {
        // SAFETY: same as find_string.
        unsafe { read_entry(self.0, name.comparison_index) }
    }
}

/// Decode `FNameEntryId` into `(block_index, byte_offset_in_block)`.
/// The handle's offset half is in 2-byte strides.
fn decode_handle(handle: u32) -> (usize, usize) {
    let block = (handle >> FNAME_BLOCK_OFFSET_BITS) as usize;
    let offset_strides = (handle & FNAME_BLOCK_OFFSET_MASK) as usize;
    (block, offset_strides * FNAME_ENTRY_STRIDE as usize)
}

fn encode_handle(block: usize, byte_offset: usize) -> u32 {
    debug_assert!(byte_offset % FNAME_ENTRY_STRIDE as usize == 0);
    debug_assert!(block < FNAME_MAX_BLOCKS);
    ((block as u32) << FNAME_BLOCK_OFFSET_BITS)
        | ((byte_offset / FNAME_ENTRY_STRIDE as usize) as u32 & FNAME_BLOCK_OFFSET_MASK)
}

enum EntryBody<'a> { Ascii(&'a [u8]), Wide(&'a [u16]) }

impl<'a> EntryBody<'a> {
    fn into_string(self) -> String {
        match self {
            EntryBody::Ascii(b) => String::from_utf8_lossy(b).into_owned(),
            EntryBody::Wide(u)  => String::from_utf16_lossy(u),
        }
    }
}

/// Walk every block of the pool, calling `visit` until it returns `Some`.
/// # Safety
/// `pool` must point at a valid FNamePool.
unsafe fn walk_blocks<T, F>(pool: *const u8, mut visit: F) -> Option<T>
where F: FnMut(u32, EntryBody<'_>) -> Option<T>,
{
    let blocks_base = pool.add(FNAMEPOOL_BLOCKS_OFFSET) as *const *const u8;
    let stride = FNAME_ENTRY_STRIDE as usize;
    for block_idx in 0..FNAME_MAX_BLOCKS {
        let block_ptr = *blocks_base.add(block_idx);
        if block_ptr.is_null() { break; }  // append-only — first NULL ends the pool.
        let mut cursor: usize = 0;
        while cursor + FNAME_ENTRY_HEADER_BYTES <= FNAME_BLOCK_SIZE_BYTES {
            let header = (block_ptr.add(cursor) as *const u16).read_unaligned();
            let len = ((header >> FNAME_ENTRY_LEN_SHIFT) & FNAME_ENTRY_LEN_MASK) as usize;
            if len == 0 { break; }  // zero-length = unallocated tail.
            let is_wide = (header & FNAME_ENTRY_WIDE_BIT) != 0;
            let body_bytes = if is_wide { len * 2 } else { len };
            let body_start = block_ptr.add(cursor + FNAME_ENTRY_HEADER_BYTES);
            let body = if is_wide {
                EntryBody::Wide(std::slice::from_raw_parts(body_start as *const u16, len))
            } else {
                EntryBody::Ascii(std::slice::from_raw_parts(body_start, len))
            };
            if let Some(t) = visit(encode_handle(block_idx, cursor), body) { return Some(t); }
            cursor += (FNAME_ENTRY_HEADER_BYTES + body_bytes + stride - 1) & !(stride - 1);
        }
    }
    None
}

/// # Safety
/// `pool` must be a valid FNamePool and `handle` must come from a walk
/// of the same pool (or be the zero handle).
unsafe fn read_entry(pool: *const u8, handle: u32) -> Option<String> {
    let (block_idx, byte_offset) = decode_handle(handle);
    if block_idx >= FNAME_MAX_BLOCKS { return None; }
    let blocks_base = pool.add(FNAMEPOOL_BLOCKS_OFFSET) as *const *const u8;
    let block_ptr = *blocks_base.add(block_idx);
    if block_ptr.is_null() { return None; }
    if byte_offset + FNAME_ENTRY_HEADER_BYTES > FNAME_BLOCK_SIZE_BYTES { return None; }
    let header = (block_ptr.add(byte_offset) as *const u16).read_unaligned();
    let len = ((header >> FNAME_ENTRY_LEN_SHIFT) & FNAME_ENTRY_LEN_MASK) as usize;
    if len == 0 { return None; }
    let is_wide = (header & FNAME_ENTRY_WIDE_BIT) != 0;
    let body_start = block_ptr.add(byte_offset + FNAME_ENTRY_HEADER_BYTES);
    Some(if is_wide {
        let units = std::slice::from_raw_parts(body_start as *const u16, len);
        String::from_utf16_lossy(units)
    } else {
        let bytes = std::slice::from_raw_parts(body_start, len);
        String::from_utf8_lossy(bytes).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-block FNamePool: 16-byte prelude + Blocks[] (only [0]
    /// non-null) + 64 KiB block with the entries packed front-to-back.
    fn build_mock_pool(entries: &[(&str, bool)]) -> (Box<[u8]>, Box<[u8]>, Vec<u32>) {
        let mut block = vec![0u8; FNAME_BLOCK_SIZE_BYTES].into_boxed_slice();
        let mut handles = Vec::with_capacity(entries.len());
        let mut cursor: usize = 0;
        for (s, wide) in entries {
            let len = if *wide { s.encode_utf16().count() } else { s.len() };
            assert!(len <= FNAME_ENTRY_LEN_MASK as usize);
            let body_bytes = if *wide { len * 2 } else { len };
            let stride = FNAME_ENTRY_STRIDE as usize;
            let padded = (FNAME_ENTRY_HEADER_BYTES + body_bytes + stride - 1) & !(stride - 1);
            assert!(cursor + padded <= FNAME_BLOCK_SIZE_BYTES);
            let header: u16 = ((len as u16) << FNAME_ENTRY_LEN_SHIFT)
                | if *wide { FNAME_ENTRY_WIDE_BIT } else { 0 };
            block[cursor..cursor + 2].copy_from_slice(&header.to_le_bytes());
            let body_off = cursor + FNAME_ENTRY_HEADER_BYTES;
            if *wide {
                for (i, u) in s.encode_utf16().enumerate() {
                    block[body_off + i*2 .. body_off + i*2 + 2].copy_from_slice(&u.to_le_bytes());
                }
            } else {
                block[body_off..body_off + len].copy_from_slice(s.as_bytes());
            }
            handles.push(encode_handle(0, cursor));
            cursor += padded;
        }
        let pool_size = FNAMEPOOL_BLOCKS_OFFSET
            + FNAME_MAX_BLOCKS * std::mem::size_of::<*const u8>();
        let mut pool = vec![0u8; pool_size].into_boxed_slice();
        let bp = (block.as_ptr() as usize).to_le_bytes();
        pool[FNAMEPOOL_BLOCKS_OFFSET..FNAMEPOOL_BLOCKS_OFFSET + bp.len()].copy_from_slice(&bp);
        (pool, block, handles)
    }

    #[test]
    fn find_and_resolve_ascii_and_wide() {
        let (pool, _block, handles) = build_mock_pool(&[("Foo", false), ("Bar", true)]);
        // SAFETY: mock pool's backing storage outlives the wrapper.
        let np = unsafe { FNamePool::from_global(pool.as_ptr()) };
        let foo = np.find_string("Foo", 0).expect("Foo present");
        let bar = np.find_string("Bar", 7).expect("Bar present");
        assert_eq!((foo.comparison_index, foo.number), (handles[0], 0));
        assert_eq!((bar.comparison_index, bar.number), (handles[1], 7));
        assert_eq!(np.resolve(foo).as_deref(), Some("Foo"));
        assert_eq!(np.resolve(bar).as_deref(), Some("Bar"));
    }

    #[test]
    fn find_missing_returns_none() {
        let (pool, _block, _h) = build_mock_pool(&[("Foo", false)]);
        let np = unsafe { FNamePool::from_global(pool.as_ptr()) };
        assert!(np.find_string("missing", 0).is_none());
    }

    #[test]
    fn fname_eq_and_hash_use_only_indices() {
        use std::collections::hash_map::DefaultHasher;
        let a = FName { comparison_index: 0x1234, number: 0 };
        let b = FName { comparison_index: 0x1234, number: 0 };
        let c = FName { comparison_index: 0x1234, number: 1 };
        assert_eq!(a, b);
        assert_ne!(a, c);
        let (mut ha, mut hb) = (DefaultHasher::new(), DefaultHasher::new());
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn handle_codec_roundtrips() {
        for &(blk, off) in &[(0usize, 0usize), (0, 64), (3, 1024), (12, 0xFFFE)] {
            assert_eq!(decode_handle(encode_handle(blk, off)), (blk, off));
        }
    }

    #[test]
    fn case_sensitive_match() {
        let (pool, _b, _h) = build_mock_pool(&[("Foo", false)]);
        let np = unsafe { FNamePool::from_global(pool.as_ptr()) };
        assert!(np.find_string("Foo", 0).is_some() && np.find_string("foo", 0).is_none());
    }
}
