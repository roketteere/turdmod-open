//! AI-as-player track ([[project_ai_as_player_track]]): perception + movement,
//! pure in-process memory access (NO ProcessEvent), every read VirtualQuery-
//! guarded so wrong offsets return None instead of faulting SCUM.exe.
//!
//! Build-drift-resilient: instead of trusting fixed pawn/field offsets, we
//! IDENTIFY the local character as the PlayerController-referenced UObject
//! whose RootComponent holds plausible world coordinates, and auto-detect the
//! location field offset. GEngine RVA from patternsleuth (SCUM.exe 2026-06-07).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS,
    PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};

const RVA_GENGINE: usize = 0x7b34228; // patternsleuth, current SCUM.exe (2026-06-14; was 0x7b32228 on 06-07 build, +0x2000 shift)
const OFF_GAMEINSTANCE: usize = 0xD28;
const OFF_LOCALPLAYERS: usize = 0x38;
const OFF_PLAYERCONTROLLER: usize = 0x30;
const OFF_CONTROL_INPUT: usize = 0x264; // APawn::ControlInputVector (verified at runtime if needed)
// UObject::ProcessEvent virtual-table slot. Dumper-7 (SCUM client SDK) reports
// ProcessEventIdx = 0x44 and it's stable across SCUM builds. GEngine is itself a
// UObject, so GEngine->vtable[68] == &UObject::ProcessEvent. @brk if a SCUM update
// reorders the UObject vtable (rare), re-confirm via Dumper-7 OffsetsInfo.json.
const PROCESSEVENT_VTABLE_IDX: usize = 68;

static MOVE_GEN: AtomicU64 = AtomicU64::new(0);

unsafe fn readable(addr: usize, len: usize) -> bool {
    if addr < 0x1_0000 || addr >= 0x0000_8000_0000_0000 {
        return false;
    }
    let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
    let n = VirtualQuery(addr as *const _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
    if n == 0 || mbi.State != MEM_COMMIT {
        return false;
    }
    let p = mbi.Protect;
    if p & (PAGE_GUARD | PAGE_NOACCESS) != 0 {
        return false;
    }
    let rd = PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
        | PAGE_WRITECOPY | PAGE_EXECUTE_WRITECOPY;
    if p & rd == 0 {
        return false;
    }
    addr + len <= mbi.BaseAddress as usize + mbi.RegionSize
}
pub(crate) unsafe fn safe_rd(a: usize) -> Option<usize> {
    if readable(a, 8) { Some((a as *const usize).read_unaligned()) } else { None }
}
unsafe fn safe_rd_f32(a: usize) -> Option<f32> {
    if readable(a, 4) { Some((a as *const f32).read_unaligned()) } else { None }
}
unsafe fn safe_wr_f32(a: usize, v: f32) -> bool {
    if readable(a, 4) { (a as *mut f32).write_unaligned(v); true } else { false }
}
pub(crate) fn img_base() -> usize {
    unsafe { GetModuleHandleW(std::ptr::null()) as usize }
}
pub(crate) fn in_img(p: usize) -> bool {
    let b = img_base();
    p > b && p < b + 0x1000_0000
}
pub(crate) unsafe fn looks_uobject(o: usize) -> bool {
    matches!(safe_rd(o), Some(vt) if in_img(vt))
}

// --- FName pool resolution (UE 4.27) — pure read, crash-guarded. ---
const RVA_FNAMEPOOL: usize = 0x79b5100; // patternsleuth, current SCUM.exe (2026-06-14; was 0x79b3100, +0x2000 shift)
const FN_BLOCKS_OFF: usize = 0x10;
const FN_BLOCK_BITS: u32 = 16;
const FN_BLOCK_MASK: u32 = 0xFFFF;
const FN_BLOCK_SIZE: usize = 2 * 65536;
const FN_HDR: usize = 2;
const FN_LEN_SHIFT: u32 = 6;
const FN_LEN_MASK: u16 = 0x03FF;
const FN_WIDE: u16 = 1;

unsafe fn fname_string(comparison_index: u32) -> Option<String> {
    let pool = img_base() + RVA_FNAMEPOOL;
    let block = (comparison_index >> FN_BLOCK_BITS) as usize;
    let off = (comparison_index & FN_BLOCK_MASK) as usize * 2;
    if block >= (1 << 13) {
        return None;
    }
    let block_ptr = safe_rd(pool + FN_BLOCKS_OFF + block * 8)?;
    if block_ptr == 0 || off + FN_HDR > FN_BLOCK_SIZE {
        return None;
    }
    if !readable(block_ptr + off, FN_HDR) {
        return None;
    }
    let header = ((block_ptr + off) as *const u16).read_unaligned();
    let len = ((header >> FN_LEN_SHIFT) & FN_LEN_MASK) as usize;
    if len == 0 || len > 512 {
        return None;
    }
    let body = block_ptr + off + FN_HDR;
    if header & FN_WIDE != 0 {
        if !readable(body, len * 2) {
            return None;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(body as *const u16, len)))
    } else {
        if !readable(body, len) {
            return None;
        }
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(body as *const u8, len)).into_owned())
    }
}

/// Resolve a UObject's class name (obj.ClassPrivate@0x10 -> UClass.NamePrivate@0x18 -> string).
pub(crate) unsafe fn class_name(obj: usize) -> Option<String> {
    let cls = safe_rd(obj + 0x10)?;
    if !looks_uobject(cls) || !readable(cls + 0x18, 4) {
        return None;
    }
    let ci = ((cls + 0x18) as *const u32).read_unaligned();
    fname_string(ci)
}
/// A UClass's own name (cls.NamePrivate@0x18 -> string).
unsafe fn class_own_name(cls: usize) -> Option<String> {
    if !readable(cls + 0x18, 4) {
        return None;
    }
    fname_string(((cls + 0x18) as *const u32).read_unaligned())
}

// --- GUObjectArray (FUObjectArray) enumeration (UE 4.27). ---
const RVA_GUOBJECTARRAY: usize = 0x79f1640; // patternsleuth, current SCUM.exe (2026-06-14; was 0x79ef640, +0x2000 shift)
const GUA_OBJOBJECTS: usize = 0x10; // FUObjectArray::ObjObjects
const GUA_NUM_ELEMENTS: usize = 0x14; // within ObjObjects
const GUA_ITEM_SIZE: usize = 0x18; // FUObjectItem
const GUA_PER_CHUNK: usize = 64 * 1024;

/// Enumerate EVERY live BP_Prisoner_C in the world (any class whose name is
/// the player character), returning (pawn, root, root-float-band). Walks the
/// whole GUObjectArray, so it finds the in-world character regardless of which
/// PlayerController owns it. Class match is cached per UClass for speed.
unsafe fn enumerate_prisoners(max: usize) -> Vec<(usize, usize, Vec<f32>)> {
    let mut out = Vec::new();
    let base = img_base();
    let arr = base + RVA_GUOBJECTARRAY;
    let objobj = arr + GUA_OBJOBJECTS;
    let chunks = match safe_rd(objobj) {
        Some(c) if c != 0 => c,
        _ => return out,
    };
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else {
        0
    };
    if num <= 0 {
        return out;
    }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut cache: std::collections::HashMap<usize, bool> = std::collections::HashMap::new();
    let mut idx = 0usize;
    while idx < num && out.len() < max {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) {
            Some(c) if c != 0 => c,
            _ => break,
        };
        let item = chunk + in_chunk * GUA_ITEM_SIZE;
        let obj = match safe_rd(item) {
            Some(o) if o != 0 => o,
            _ => continue,
        };
        let cls = match safe_rd(obj + 0x10) {
            Some(c) if c != 0 => c,
            _ => continue,
        };
        let is_char = *cache.entry(cls).or_insert_with(|| {
            class_own_name(cls).map(|n| is_character_class(&n)).unwrap_or(false)
        });
        if !is_char {
            continue;
        }
        // find root + dump its location band for movement-diffing
        let mut root = 0usize;
        for r in (0x100..0x180).step_by(8) {
            let c = safe_rd(obj + r).unwrap_or(0);
            if c != 0 && c != obj && looks_uobject(c) {
                root = c;
                break;
            }
        }
        // WIDE band: root transform region 0x80..0x340 + pawn velocity/input
        // region 0x200..0x300. Any float that changes while walking = motion.
        let mut band: Vec<f32> = Vec::new();
        if root != 0 {
            band.extend((0x80..0x340).step_by(4).map(|o| safe_rd_f32(root + o).unwrap_or(f32::NAN)));
        }
        band.extend((0x200..0x300).step_by(4).map(|o| safe_rd_f32(obj + o).unwrap_or(f32::NAN)));
        out.push((obj, root, band));
    }
    out
}

/// Is this class name the local player character (vs vehicle/zombie/camera)?
fn is_character_class(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    (n.contains("prisoner") || n.contains("conzchar") || n.contains("character"))
        && !n.contains("controller")
        && !n.contains("anim")
        && !n.contains("camera")
        && !n.contains("zombie")
}
/// Finite, bounded, not all-zero — a candidate location triple.
fn finite_loc(v: [f32; 3]) -> bool {
    v.iter().all(|f| f.is_finite() && f.abs() < 2.0e6) && !(v[0] == 0.0 && v[1] == 0.0 && v[2] == 0.0)
}
/// Horizontal magnitude — used to pick the real world character over small
/// UI/relative objects. SCUM map coords are large (≫1000).
fn horiz(v: [f32; 3]) -> f32 {
    v[0].abs() + v[1].abs()
}
/// A real spawned-in-world character has large horizontal world coords.
const WORLD_MIN_HORIZ: f32 = 1000.0;

/// Walk to the PlayerController (the stable part of the chain).
pub(crate) unsafe fn resolve_pc() -> Option<usize> {
    let base = img_base();
    let geng = safe_rd(base + RVA_GENGINE)?;
    let gi = safe_rd(geng + OFF_GAMEINSTANCE)?;
    let lp_data = safe_rd(gi + OFF_LOCALPLAYERS)?;
    if safe_rd(gi + OFF_LOCALPLAYERS + 8).unwrap_or(0) as i32 <= 0 {
        return None;
    }
    let lp0 = safe_rd(lp_data)?;
    let pc = safe_rd(lp0 + OFF_PLAYERCONTROLLER)?;
    if pc == 0 { None } else { Some(pc) }
}

/// Resolve UObject::ProcessEvent — the universal UFunction-call primitive.
/// GEngine is a UObject; ProcessEvent is virtual slot 68 (Dumper-7 confirmed,
/// build-stable), so GEngine->vtable[68] is its address. Pure read, crash-guarded.
pub unsafe fn resolve_process_event() -> Option<usize> {
    let base = img_base();
    let geng = safe_rd(base + RVA_GENGINE)?; // UGameEngine*
    if geng == 0 { return None; }
    let vtable = safe_rd(geng)?; // UGameEngine vtable (in .rdata of the image)
    if !in_img(vtable) { return None; }
    let pe = safe_rd(vtable + PROCESSEVENT_VTABLE_IDX * 8)?;
    if in_img(pe) { Some(pe) } else { None }
}

/// Diagnostic: resolve + report the native call surface (ProcessEvent, GEngine,
/// PlayerController). Confirms the un-staled RVAs work and the ProcessEvent
/// vtable approach lands before we wire CreateWidget/AddToViewport.
pub fn engine_probe() -> Json {
    unsafe {
        let base = img_base();
        let geng = safe_rd(base + RVA_GENGINE).unwrap_or(0);
        let pe = resolve_process_event();
        let pc = resolve_pc();
        json!({
            "ok": pe.is_some(),
            "imgBase": format!("{:#x}", base),
            "gEngine": format!("{:#x}", geng),
            "processEvent": pe.map(|p| format!("{:#x} (rva {:#x})", p, p - base)),
            "playerController": pc.map(|p| format!("{:#x}", p)),
        })
    }
}

/// A UObject's own FName string (NamePrivate at +0x18). Crash-guarded.
pub(crate) unsafe fn object_name(obj: usize) -> Option<String> {
    if !readable(obj + 0x18, 4) { return None; }
    let idx = ((obj + 0x18) as *const u32).read_unaligned();
    fname_string(idx)
}

/// Read a material's parent-material name. SCUM's vehicle slots hold a
/// UMaterialInstanceDynamic whose Parent (UMaterialInstance::Parent) is the real
/// typed material (e.g. Mi_Dummy_Glass / _Metal / _Rubber) — that name is how we
/// tell a glass slot from a rim slot from a tire slot. Returns the first
/// MaterialInterface-derived member (the Parent sits near the top of the instance).
pub(crate) unsafe fn material_parent_name(mat: usize) -> Option<String> {
    if mat == 0 || !looks_uobject(mat) { return None; }
    // skip class(+0x10)/outer(+0x20); scan the rest of the instance for the first
    // member pointing at another MaterialInterface (the Parent).
    for off in (0x28..0x600).step_by(8) {
        if off == 0x10 || off == 0x20 { continue; }
        let p = match safe_rd(mat + off) { Some(p) if p != 0 && p != mat && looks_uobject(p) => p, _ => continue };
        let cls = match safe_rd(p + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, "MaterialInterface") {
            if let Some(n) = object_name(p) { return Some(n); }
        }
    }
    None
}

/// Find the first live UObject whose name == `name`, optionally requiring its
/// Outer (+0x20) to be named `outer_name`. Walks GUObjectArray. Slow/one-shot —
/// used to resolve the WBP class, the BlueprintLibrary CDO, and UFunctions by
/// name for the native-UI ProcessEvent calls. Every read is VirtualQuery-guarded.
pub(crate) unsafe fn find_object(name: &str, outer_name: Option<&str>) -> Option<usize> {
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|c| *c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        match object_name(obj) { Some(n) if n == name => {}, _ => continue }
        if let Some(on) = outer_name {
            let outer = match safe_rd(obj + 0x20) { Some(o) if o != 0 => o, _ => continue };
            match object_name(outer) { Some(n) if n == on => {}, _ => continue }
        }
        return Some(obj);
    }
    None
}

/// Walk a UClass's SuperStruct chain (UStruct::SuperStruct @ 0x40, Dumper-confirmed)
/// looking for a class whose own name == `target`. Crash-guarded; caps depth.
unsafe fn class_is_a(cls: usize, target: &str) -> bool {
    let mut c = cls;
    for _ in 0..20 {
        if c == 0 || !looks_uobject(c) {
            break;
        }
        if matches!(class_own_name(c), Some(n) if n == target) {
            return true;
        }
        c = safe_rd(c + 0x40).unwrap_or(0);
    }
    false
}

/// Find a pawn to possess for the exo-suit mech. Walks GUObjectArray for a live
/// instance derived from `AConZCharacter` (SCUM's character base — players, puppets,
/// sentries all share it, so they all WALK with CharacterMovement). Skips the local
/// player + CDOs. Prefers a real Sentry/Mech/Drone by class name. Returns (pawn, class).
pub(crate) unsafe fn find_possess_target() -> Option<(usize, String)> {
    let my_pawn = find_character().map(|c| c.pawn).unwrap_or(0);
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|c| *c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    // Live Sentry/Drone hijack CRASHES SCUM (~7s in — their AI/combat tick faults with no
    // AI controller). Confirmed 2026-06-14. So AVOID them for the live-possess walk proof;
    // prefer simple mobile bodies (Puppet = SCUM's zombie). The mech LOOK comes from a custom
    // cooked pawn (no combat AI), not from hijacking a live Sentry.
    const PREFER: &[&str] = &["Puppet", "Soldier", "Enemy"];
    const AVOID: &[&str] = &["Sentry", "Mech", "Drone", "Robot", "Banker", "Trader", "Mechanic",
        "Vendor", "Shop", "Caretaker", "Doctor", "Barber"];
    let mut cache: std::collections::HashMap<usize, bool> = std::collections::HashMap::new();
    let mut best: Option<(usize, String)> = None; // non-vendor character
    let mut any: Option<(usize, String)> = None; // anything as a last resort
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        if obj == my_pawn { continue; }
        if matches!(object_name(obj), Some(n) if n.starts_with("Default__")) { continue; }
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        let cn = match class_own_name(cls) { Some(n) => n, None => continue };
        let is_char = *cache.entry(cls).or_insert_with(|| class_is_a(cls, "ConZCharacter"));
        if !is_char { continue; }
        if PREFER.iter().any(|p| cn.contains(p)) {
            return Some((obj, cn)); // ideal mobile/combatant body
        }
        let avoided = AVOID.iter().any(|a| cn.contains(a));
        if !avoided && best.is_none() { best = Some((obj, cn.clone())); }
        if any.is_none() { any = Some((obj, cn)); }
    }
    best.or(any)
}

/// Find the first live non-CDO CHARACTER instance whose class's own name contains
/// `substr` (e.g. "Sentry" → the BP_Sentry_C pawn, NOT its AIController/spawner/volume
/// which also contain "Sentry"). Requires AConZCharacter so it has a body mesh to borrow.
/// Read-only; crash-guarded.
pub(crate) unsafe fn find_instance_named(substr: &str) -> Option<usize> {
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|c| *c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        if matches!(object_name(obj), Some(n) if n.starts_with("Default__")) { continue; }
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if matches!(class_own_name(cls), Some(n) if n.contains(substr)) && class_is_a(cls, "ConZCharacter") {
            return Some(obj);
        }
    }
    None
}

/// Read a usize at addr (crash-guarded). Exposed for native_ui's mesh-swap chain.
pub(crate) unsafe fn rd(a: usize) -> Option<usize> {
    safe_rd(a)
}

/// World location of an actor: scan its members for the RootComponent (a SceneComponent)
/// and read a plausible world-coordinate triple from it. Fuzzy (same heuristic as the
/// player locator) but good enough to compare distances. None if not locatable.
pub(crate) unsafe fn actor_location(obj: usize) -> Option<[f32; 3]> {
    for r in (0x100..0x220).step_by(8) {
        let comp = match safe_rd(obj + r) {
            Some(c) if c != 0 && c != obj && looks_uobject(c) => c,
            _ => continue,
        };
        for lo in (0x100..0x180).step_by(4) {
            let v = [
                safe_rd_f32(comp + lo).unwrap_or(f32::NAN),
                safe_rd_f32(comp + lo + 4).unwrap_or(f32::NAN),
                safe_rd_f32(comp + lo + 8).unwrap_or(f32::NAN),
            ];
            if finite_loc(v) && horiz(v) > WORLD_MIN_HORIZ {
                return Some(v);
            }
        }
    }
    None
}

/// Find the vehicle (class name contains any of `subs`) NEAREST to `from`. This is how
/// we target the car the player is standing next to, not the first one in memory.
pub(crate) unsafe fn find_nearest_vehicle(subs: &[&str], from: [f32; 3]) -> Option<usize> {
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|c| *c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut best: Option<(usize, f32)> = None;
    let mut first_any: Option<usize> = None;
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        if matches!(object_name(obj), Some(n) if n.starts_with("Default__")) { continue; }
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        let cn = match class_own_name(cls) { Some(n) => n, None => continue };
        if !subs.iter().any(|s| cn.contains(s)) { continue; }
        if first_any.is_none() { first_any = Some(obj); }
        if let Some(loc) = actor_location(obj) {
            let dx = loc[0] - from[0]; let dy = loc[1] - from[1]; let dz = loc[2] - from[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            if best.map_or(true, |(_, bd)| d2 < bd) { best = Some((obj, d2)); }
        }
    }
    best.map(|(o, _)| o).or(first_any)
}

/// Find the first live non-CDO UObject whose class's OWN name contains any of `subs`
/// (e.g. ["Laika","Cruiser"] → the actual vehicle actor). Read-only; crash-guarded.
pub(crate) unsafe fn find_instance_class_contains(subs: &[&str]) -> Option<usize> {
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|c| *c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        if matches!(object_name(obj), Some(n) if n.starts_with("Default__")) { continue; }
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if let Some(cn) = class_own_name(cls) {
            if subs.iter().any(|s| cn.contains(s)) {
                return Some(obj);
            }
        }
    }
    None
}

/// Find the first live non-CDO UObject whose class CHAIN includes `base` (e.g.
/// "DcxVehicle" → a loaded vehicle, "VehiclePaintJobItemComponent" → a vehicle's
/// paint component). Read-only; crash-guarded.
pub(crate) unsafe fn find_instance_deriving(base: &str) -> Option<usize> {
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|c| *c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        if matches!(object_name(obj), Some(n) if n.starts_with("Default__")) { continue; }
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, base) {
            return Some(obj);
        }
    }
    None
}

/// Scan an actor's members for its primary USkeletalMeshComponent (the body mesh).
/// More robust than a hardcoded ACharacter::Mesh offset (SCUM may relayout). Returns
/// (component_ptr, offset, class_name). Skips obvious non-body comps by taking the
/// FIRST SkeletalMeshComponent-derived pointer in the member range.
pub(crate) unsafe fn find_skeletal_mesh_comp(obj: usize) -> Option<(usize, usize, String)> {
    for off in (0x100..0x600).step_by(8) {
        let p = match safe_rd(obj + off) { Some(p) if p != 0 && p != obj => p, _ => continue };
        if !looks_uobject(p) { continue; }
        let cls = match safe_rd(p + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, "SkeletalMeshComponent") {
            let cn = class_own_name(cls).unwrap_or_default();
            return Some((p, off, cn));
        }
    }
    None
}

/// Collect ALL of an actor's USkeletalMeshComponent pointers (body + every gear/
/// clothing layer). Used to hide the human gear layers when "mounting" the mech so
/// the clean robot body shows, and re-show them on dismount.
pub(crate) unsafe fn find_all_skeletal_mesh_comps(obj: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for off in (0x100..0x600).step_by(8) {
        let p = match safe_rd(obj + off) { Some(p) if p != 0 && p != obj => p, _ => continue };
        if !looks_uobject(p) { continue; }
        let cls = match safe_rd(p + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, "SkeletalMeshComponent") && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Collect EVERY live component deriving `base` whose Outer (+0x20) is `owner` — i.e.
/// all of an actor's mesh components, found by OWNERSHIP via GUObjectArray (reliable;
/// no member-offset guessing). Read-only; crash-guarded.
pub(crate) unsafe fn find_owned_comps(owner: usize, base: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = match safe_rd(objobj) { Some(c) if c != 0 => c, _ => return out };
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return out; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        if safe_rd(obj + 0x20) != Some(owner) { continue; } // Outer == owner
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, base) {
            out.push(obj);
        }
    }
    out
}

/// Collect EVERY live MeshComponent whose OWNER (Outer @ +0x20)'s class name contains
/// `owner_sub` — i.e. every mesh on the Laika AND its Laika-named sub-objects, wherever
/// SCUM nests the visible body. Returns (comp, owner_class, comp_class) for logging.
pub(crate) unsafe fn find_mesh_comps_by_owner_class(owner_sub: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = match safe_rd(objobj) { Some(c) if c != 0 => c, _ => return out };
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return out; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if !class_is_a(cls, "MeshComponent") { continue; }
        let owner = match safe_rd(obj + 0x20) { Some(o) if o != 0 && looks_uobject(o) => o, _ => continue };
        let ocls = match safe_rd(owner + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        let ocn = match class_own_name(ocls) { Some(n) => n, None => continue };
        if ocn.contains(owner_sub) {
            let ccn = class_own_name(cls).unwrap_or_default();
            out.push((obj, ocn, ccn));
        }
    }
    out
}

/// Collect every live component whose CLASS derives `comp_base` and whose OWNER
/// (Outer @ +0x20) class name contains `owner_sub`. Generalizes the mesh finder —
/// used to grab the headlights' ULightComponent(s) ("LightComponent", "Headlight").
pub(crate) unsafe fn find_comps_by_class_owner(comp_base: &str, owner_sub: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = match safe_rd(objobj) { Some(c) if c != 0 => c, _ => return out };
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return out; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if !class_is_a(cls, comp_base) { continue; }
        let owner = match safe_rd(obj + 0x20) { Some(o) if o != 0 && looks_uobject(o) => o, _ => continue };
        let ocls = match safe_rd(owner + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        let ocn = match class_own_name(ocls) { Some(n) => n, None => continue };
        if ocn.contains(owner_sub) {
            out.push(obj);
        }
    }
    out
}

/// First component deriving `comp_base` whose OWNER (Outer @ +0x20) is exactly
/// `owner_ptr` — used to grab a freshly-spawned actor's own component (e.g. a
/// StaticMeshActor's StaticMeshComponent) so we can set its mesh.
pub(crate) unsafe fn find_comp_of_owner(owner_ptr: usize, comp_base: &str) -> Option<usize> {
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = safe_rd(objobj).filter(|&c| c != 0)?;
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return None; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        let owner = match safe_rd(obj + 0x20) { Some(o) => o, _ => continue };
        if owner != owner_ptr { continue; }
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, comp_base) { return Some(obj); }
    }
    None
}

/// Every live component whose CLASS derives `comp_base`, returned with its OWNER
/// class name + its OWN object name (no owner filter). Lets the caller match
/// broadly (the headlight's owner isn't literally "Headlight") and lets a probe
/// dump exactly what's there. @dep paint_vehicle light targeting + probe_lights.
pub(crate) unsafe fn find_comps_by_class_named(comp_base: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    let objobj = img_base() + RVA_GUOBJECTARRAY + GUA_OBJOBJECTS;
    let chunks = match safe_rd(objobj) { Some(c) if c != 0 => c, _ => return out };
    let num = if readable(objobj + GUA_NUM_ELEMENTS, 4) {
        ((objobj + GUA_NUM_ELEMENTS) as *const i32).read_unaligned()
    } else { 0 };
    if num <= 0 { return out; }
    let num = (num as usize).min(4 * 1024 * 1024);
    let mut idx = 0usize;
    while idx < num {
        let chunk_idx = idx / GUA_PER_CHUNK;
        let in_chunk = idx % GUA_PER_CHUNK;
        idx += 1;
        let chunk = match safe_rd(chunks + chunk_idx * 8) { Some(c) if c != 0 => c, _ => break };
        let obj = match safe_rd(chunk + in_chunk * GUA_ITEM_SIZE) { Some(o) if o != 0 => o, _ => continue };
        let cls = match safe_rd(obj + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if !class_is_a(cls, comp_base) { continue; }
        let owner = safe_rd(obj + 0x20).filter(|&o| o != 0 && looks_uobject(o));
        let ocn = owner.and_then(|o| safe_rd(o + 0x10)).filter(|&c| c != 0 && looks_uobject(c))
            .and_then(|c| class_own_name(c)).unwrap_or_default();
        let own = object_name(obj).unwrap_or_default();
        out.push((obj, ocn, own));
    }
    out
}

/// Collect ALL of an actor's component pointers whose class derives `base` (e.g.
/// "MeshComponent" → every visible mesh on a vehicle). Wide scan; dedups.
pub(crate) unsafe fn find_all_comps_deriving(obj: usize, base: &str, start: usize, end: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for off in (start..end).step_by(8) {
        let p = match safe_rd(obj + off) { Some(p) if p != 0 && p != obj => p, _ => continue };
        if !looks_uobject(p) { continue; }
        let cls = match safe_rd(p + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, base) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Scan `obj`'s members for a pointer to an object whose CLASS derives `target`
/// (e.g. "SkeletalMesh" asset on a mesh component). Returns (ptr, offset).
pub(crate) unsafe fn find_member_obj(obj: usize, target: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    for off in (start..end).step_by(8) {
        let p = match safe_rd(obj + off) { Some(p) if p != 0 && p != obj => p, _ => continue };
        if !looks_uobject(p) { continue; }
        let cls = match safe_rd(p + 0x10) { Some(c) if c != 0 && looks_uobject(c) => c, _ => continue };
        if class_is_a(cls, target) {
            return Some((p, off));
        }
    }
    None
}

/// Scan `obj`'s members for a pointer that IS a UClass deriving `base` (e.g. an
/// AnimClass = a UClass deriving UAnimInstance). Returns (class_ptr, offset).
pub(crate) unsafe fn find_member_class(obj: usize, base: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    for off in (start..end).step_by(8) {
        let p = match safe_rd(obj + off) { Some(p) if p != 0 && p != obj => p, _ => continue };
        if !looks_uobject(p) { continue; }
        if class_is_a(p, base) {
            return Some((p, off));
        }
    }
    None
}

/// Found character: pawn ptr, its RootComponent, the location-field offset
/// within the root, and the current world location.
struct Char {
    pawn: usize,
    root: usize,
    loc_off: usize,
    loc: [f32; 3],
}

/// Identify the local character: the PC-referenced UObject whose RootComponent
/// holds plausible world coordinates. Auto-detects the location offset. This
/// only resolves once the player is genuinely spawned in-world.
unsafe fn find_character() -> Option<Char> {
    let pc = resolve_pc()?;
    for off in (0x200..0x380).step_by(8) {
        let pawn = safe_rd(pc + off).unwrap_or(0);
        if pawn == 0 || in_img(pawn) || !looks_uobject(pawn) {
            continue;
        }
        // PRIMARY identifier: the pawn whose class is the player character.
        match class_name(pawn) {
            Some(n) if is_character_class(&n) => {}
            _ => continue,
        }
        // Found the character pawn. Locate its RootComponent + location field.
        let mut root = 0usize;
        for r in (0x100..0x180).step_by(8) {
            let c = safe_rd(pawn + r).unwrap_or(0);
            if c != 0 && c != pawn && looks_uobject(c) {
                root = c;
                break;
            }
        }
        if root == 0 {
            continue;
        }
        // Find the location triple (RelativeLocation/ComponentToWorld translation).
        for lo in (0x100..0x160).step_by(4) {
            let v = [
                safe_rd_f32(root + lo).unwrap_or(f32::NAN),
                safe_rd_f32(root + lo + 4).unwrap_or(f32::NAN),
                safe_rd_f32(root + lo + 8).unwrap_or(f32::NAN),
            ];
            if finite_loc(v) && horiz(v) > WORLD_MIN_HORIZ {
                return Some(Char { pawn, root, loc_off: lo, loc: v });
            }
        }
        // Character found but no clear location field — still return it for
        // movement (movement uses ControlInputVector, not the location).
        return Some(Char { pawn, root, loc_off: 0, loc: [0.0, 0.0, 0.0] });
    }
    None
}

pub fn resolve_pawn() -> Result<usize, String> {
    unsafe { find_character().map(|c| c.pawn).ok_or("character not found (in-world?)".into()) }
}

/// (player pawn, world location) — pawn doubles as a WorldContextObject for spawns,
/// location seeds the spawn transform. None until the player is in-world.
pub(crate) unsafe fn pawn_and_loc() -> Option<(usize, [f32; 3])> {
    find_character().map(|c| (c.pawn, c.loc))
}

/// Write a pawn's ControlInputVector (APawn member @ OFF_CONTROL_INPUT). The pawn's
/// CharacterMovement consumes this each tick → the body walks. Used to drive a
/// possessed mech from the player's WASD (AI bodies have no player input bindings,
/// so we feed movement directly). World-space direction. Crash-guarded.
pub(crate) unsafe fn set_control_input(pawn: usize, v: [f32; 3]) -> bool {
    if pawn == 0 { return false; }
    let a = pawn + OFF_CONTROL_INPUT;
    safe_wr_f32(a, v[0]) && safe_wr_f32(a + 4, v[1]) && safe_wr_f32(a + 8, v[2])
}

pub fn pull_player_state() -> Result<Json, String> {
    unsafe {
        let c = find_character().ok_or("character not found (not spawned in-world yet)")?;
        Ok(json!({
            "ok": true,
            "pawn": format!("0x{:x}", c.pawn),
            "root": format!("0x{:x}", c.root),
            "locOff": format!("0x{:x}", c.loc_off),
            "x": c.loc[0], "y": c.loc[1], "z": c.loc[2],
        }))
    }
}

pub fn move_input(dx: f32, dy: f32, dz: f32, scale: f32, duration_ms: u64) -> Json {
    let mag = (dx * dx + dy * dy + dz * dz).sqrt();
    if mag < 1e-4 {
        return json!({ "ok": false, "error": "zero direction" });
    }
    let s = scale.clamp(0.0, 1.0);
    let (nx, ny, nz) = (dx / mag * s, dy / mag * s, dz / mag * s);
    let gen = MOVE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let dur = Duration::from_millis(duration_ms.min(60_000));
    std::thread::Builder::new()
        .name("turdmod-mover".into())
        .spawn(move || {
            let start = Instant::now();
            while start.elapsed() < dur {
                if MOVE_GEN.load(Ordering::SeqCst) != gen {
                    break;
                }
                unsafe {
                    if let Some(c) = find_character() {
                        let a = c.pawn + OFF_CONTROL_INPUT;
                        safe_wr_f32(a, nx);
                        safe_wr_f32(a + 4, ny);
                        safe_wr_f32(a + 8, nz);
                    } else {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(8));
            }
        })
        .ok();
    json!({ "ok": true, "started": true, "dir": [nx, ny, nz], "durationMs": duration_ms, "gen": gen })
}

pub fn move_forward(scale: f32, duration_ms: u64) -> Json {
    // Without a verified yaw offset, just drive +X; caller can use moveInput
    // for a specific world direction once we confirm motion.
    move_input(1.0, 0.0, 0.0, scale, duration_ms)
}

pub fn move_stop() -> Json {
    MOVE_GEN.fetch_add(1, Ordering::SeqCst);
    json!({ "ok": true, "stopped": true })
}

/// SELF-MEASURING DRIVE TEST: write a forward input vector to candidate
/// ControlInputVector offsets on the character and check whether its
/// RootComponent transform band actually CHANGES (= the body moved). Tries
/// only currently-zero float triples (the input vector is 0 when idle), so it
/// won't clobber live data. Returns the offset that produced motion, or proof
/// that nothing moved (character not controllable / not in a playable state).
pub fn drive_probe() -> Json {
    unsafe {
        let c = match find_character() {
            Some(c) => c,
            None => return json!({ "ok": false, "error": "character not found" }),
        };
        let band = |root: usize| -> Vec<f32> {
            (0x80..0x340).step_by(4).map(|o| safe_rd_f32(root + o).unwrap_or(f32::NAN)).collect()
        };
        let changed = |a: &[f32], b: &[f32]| -> usize {
            a.iter().zip(b.iter())
                .filter(|(x, y)| x.is_finite() && y.is_finite() && (**x - **y).abs() > 0.5)
                .count()
        };
        let mut tried = Vec::new();
        for civ in (0x240..0x2A0).step_by(4) {
            // candidate must be a currently-~0 triple (idle input vector shape)
            let v = [
                safe_rd_f32(c.pawn + civ).unwrap_or(f32::NAN),
                safe_rd_f32(c.pawn + civ + 4).unwrap_or(f32::NAN),
                safe_rd_f32(c.pawn + civ + 8).unwrap_or(f32::NAN),
            ];
            if !v.iter().all(|f| f.is_finite() && f.abs() < 0.01) {
                continue;
            }
            tried.push(format!("0x{civ:x}"));
            let baseline = band(c.root);
            // drive forward ~1.5s at 125 Hz
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(1500) {
                if let Some(cc) = find_character() {
                    let a = cc.pawn + civ;
                    safe_wr_f32(a, 1.0);
                    safe_wr_f32(a + 4, 0.0);
                    safe_wr_f32(a + 8, 0.0);
                }
                std::thread::sleep(Duration::from_millis(8));
            }
            std::thread::sleep(Duration::from_millis(200));
            let after = band(c.root);
            let n = changed(&baseline, &after);
            if n > 0 {
                return json!({
                    "ok": true, "MOVED": true, "civOff": format!("0x{civ:x}"),
                    "changedFloats": n, "pawn": format!("0x{:x}", c.pawn),
                });
            }
        }
        json!({
            "ok": true, "MOVED": false, "triedOffsets": tried,
            "note": "no input offset moved the character — not controllable / not in a playable in-world state",
        })
    }
}

/// List every BP_Prisoner_C in the world with its location-float band. Poll
/// twice while the player walks: the entry whose band CHANGES is the live
/// moving character (independent of the PlayerController path).
pub fn list_prisoners() -> Json {
    unsafe {
        let ps = enumerate_prisoners(40);
        let arr: Vec<Json> = ps.iter().map(|(pawn, root, band)| {
            json!({ "pawn": format!("0x{pawn:x}"), "root": format!("0x{root:x}"), "band_0x100": band })
        }).collect();
        json!({ "ok": true, "count": arr.len(), "prisoners": arr })
    }
}

/// DIAGNOSTIC: for each PC-referenced UObject pawn, dump a wide band of floats
/// from its first SceneComponent (location region) AND from the pawn itself
/// (input-vector region). Poll while the player walks WASD: the float triple
/// that CHANGES is the world location; the floats on the pawn that go NON-ZERO
/// while moving are ControlInputVector. Identifies pawn + both offsets exactly.
/// Crash-proof. `idx` selects which candidate to detail (0-based); summary
/// always lists all candidate (pcOff,pawn,root) tuples.
pub fn probe_dump() -> Json {
    unsafe {
        let pc = match resolve_pc() {
            Some(p) => p,
            None => return json!({ "ok": false, "error": "no PlayerController (not in session)" }),
        };
        let mut cands = Vec::new();
        for off in (0x200..0x380).step_by(8) {
            let pawn = safe_rd(pc + off).unwrap_or(0);
            if pawn == 0 || in_img(pawn) || !looks_uobject(pawn) {
                continue;
            }
            // first SceneComponent-looking child = root
            let mut root = 0usize;
            for r in (0x100..0x180).step_by(8) {
                let c = safe_rd(pawn + r).unwrap_or(0);
                if c != 0 && c != pawn && looks_uobject(c) {
                    root = c;
                    break;
                }
            }
            // dedup by pawn
            if cands.iter().any(|(p, _, _): &(usize, usize, usize)| *p == pawn) {
                continue;
            }
            cands.push((off, pawn, root));
        }
        // Detail every candidate: location-region floats (root 0x110..0x140)
        // + input-region floats (pawn 0x258..0x278).
        // Detail ONLY the character (BP_Prisoner_C) with WIDE float bands so
        // a walk-while-polling capture reveals the location (root, changes)
        // and ControlInputVector (pawn, goes non-zero while moving) offsets.
        let detail: Vec<Json> = cands.iter().filter_map(|(off, pawn, root)| {
            let cn = class_name(*pawn);
            if !cn.as_deref().map(is_character_class).unwrap_or(false) {
                return None;
            }
            // root float band 0x100..0x1E0 (label by offset), pawn band 0x240..0x2D0.
            let root_band: Vec<Json> = (0x100..0x1E0).step_by(4)
                .filter_map(|o| {
                    let v = safe_rd_f32(*root + o)?;
                    if v.is_finite() && v.abs() > 1.0 && v.abs() < 2.0e6 {
                        Some(json!([format!("0x{o:x}"), v]))
                    } else { None }
                }).collect();
            let pawn_band: Vec<f32> = (0x240..0x2D0).step_by(4)
                .map(|o| safe_rd_f32(*pawn + o).unwrap_or(f32::NAN)).collect();
            Some(json!({
                "pcOff": format!("0x{off:x}"), "pawn": format!("0x{pawn:x}"), "root": format!("0x{root:x}"),
                "class": cn,
                "rootRealFloats": root_band,
                "pawn_0x240": pawn_band,
            }))
        }).collect();
        json!({ "ok": true, "pc": format!("0x{pc:x}"), "n": cands.len(), "cands": detail })
    }
}
