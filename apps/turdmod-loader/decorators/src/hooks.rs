//! Function-detour scaffolding for the decorator DLL.
//!
//! The detour install path is staged so the engine-side work (sigscan-resolved
//! UE 4.27 vtable entries → `retour::GenericDetour`) can plug in once dossier
//! sections 22 (decorator class CDOs) and 23 (UTexture2D construction)
//! return runtime-validated patterns.
//!
//! Today this module exposes:
//!
//! * `install()` — top-level entrypoint called from `init_thread`. Currently
//!   logs and returns Ok; future work resolves the three decorator
//!   `CreateDecorator()` vtable entries via `GUObjectArray`-FName lookup +
//!   vtable-offset (per docs/scum-internals/22-...) and wraps each in a
//!   `GenericDetour`.
//! * `HookRegistry` — owns the live detour handles so they survive past the
//!   stack frame that installed them. `Drop` disables them at unload.
//! * `install_smoke_hook` — a self-contained test that detours a known Rust
//!   function in this same DLL, calls it, and confirms the detour fires
//!   *and* the trampoline still reaches the original. Validates the
//!   `retour` integration end-to-end without needing UE4 internals.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::OnceCell;
use retour::GenericDetour;

use crate::engine_resolve;
use crate::guarray::{count_class_instances, count_subclass_instances, GUObjectArray};
use crate::logging;
use crate::sigscan_ext;
use crate::uobject::UClassPtr;
use crate::vtable::vtable_slot;

/// Process-wide registry of installed detours. Disabling on Drop is the
/// safe-shutdown path — we never want a detour to outlive the DLL it points
/// into, so DllMain DETACH (or test-tearDown) cleans up.
pub struct HookRegistry {
    /// Boxed so that future Phase-B detours of varying signatures can sit
    /// in the same registry. `dyn Drop` is enough — `retour::GenericDetour`
    /// disables itself in its own `Drop` impl.
    handles: Vec<Box<dyn DropHandle + Send>>,
}

/// Tiny trait-object adapter so the registry holds detours of different
/// concrete `GenericDetour<T>` types in one Vec. We don't need to *call*
/// them through this trait — only to keep them alive and let `Drop` run.
trait DropHandle {}
impl<T: ?Sized> DropHandle for T {}

impl HookRegistry {
    pub fn new() -> Self { Self { handles: Vec::new() } }

    /// Take ownership of a detour. Returns the same handle so install
    /// callers can `?` on errors without losing the boxed value.
    pub fn keep_alive<H: DropHandle + Send + 'static>(&mut self, h: H) {
        self.handles.push(Box::new(h));
    }

    pub fn count(&self) -> usize { self.handles.len() }
}

static REGISTRY: OnceCell<Mutex<HookRegistry>> = OnceCell::new();

fn registry() -> &'static Mutex<HookRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(HookRegistry::new()))
}

/// Install all decorator detours. Called once from `init_thread`.
///
/// Phase B step 1 — run the engine-resolution probe and log everything we
/// found to `%LOCALAPPDATA%/TurdMOD/decorators.log`. This step never
/// installs an actual detour: it answers "do all the layout assumptions
/// hold against this build?" purely from logs. Once a live SCUM client
/// run produces a probe report with `all_classes_resolved() == true`, the
/// next step (vtable-diff + `GenericDetour::new`) gets wired into the
/// gated branch below.
pub fn install() -> Result<(), String> {
    logging::log("[hooks] install() — phase B step 1: engine-resolve probe");

    let report = engine_resolve::probe();
    // One write per line so a tail -f tracks progress; summary() already
    // returns a multi-line string so we just hand it through.
    for line in report.summary().lines() {
        logging::log(line);
    }

    match report.child_vtable_diffs_agree() {
        Some(idx) => {
            logging::log(&format!(
                "[hooks] all checks pass — children agree on CreateDecorator@vtable[{idx}]; \
                 installing detours"
            ));
            if let Err(e) = install_create_decorator_detours(&report, idx) {
                logging::log(&format!("[hooks] detour install FAILED: {e}"));
            }
            // Spawn the instance-survey thread regardless of detour install
            // outcome — even if install failed, the survey is informative
            // diagnostic data.
            spawn_instance_survey(&report);
        }
        None if report.all_cdos_resolved() => logging::log(
            "[hooks] CDOs resolved but child vtable-diffs disagree — refusing to install \
             detours (likely vtable-pointer miss or layout drift)"
        ),
        None if report.all_uclasses_resolved() => logging::log(
            "[hooks] UClasses resolved but CDO probe failed — refusing to install detours; \
             see notes above"
        ),
        None => logging::log(
            "[hooks] one or more decorator classes did NOT resolve — refusing to install \
             detours; see notes above"
        ),
    }

    Ok(())
}

// ---------- CreateDecorator detour ----------
//
// `URichTextBlockDecorator::CreateDecorator(URichTextBlock* InOwner)` returns
// a `TSharedPtr<ITextDecorator>` (16 bytes: ObjectPtr + ReferenceController).
// MSVC x64 returns non-trivial >8-byte aggregates by hidden pointer, so the
// effective ABI is:
//
//   TSharedPtr<ITextDecorator>* __fastcall CreateDecorator(
//       URichTextBlockImageDecorator* this,    // RCX
//       TSharedPtr<ITextDecorator>* return_ptr, // RDX (hidden)
//       URichTextBlock* InOwner                 // R8
//   );
//   // returns RAX = return_ptr
//
// On x64 Windows, both `extern "C"` and `extern "system"` map to fastcall, so
// either works. We use `extern "C"` for portability with the rest of the
// codebase.
//
// Image and ActionPrompt have DIFFERENT vtables (proven by the diff probe),
// so vtable[78] points to a DIFFERENT function for each. We install one
// detour per child. Both handlers route through the same logging path with
// a class label so the log is unambiguous.

type CreateDecoratorFn = unsafe extern "C" fn(
    this: *mut u8,
    return_slot: *mut u8,
    owner: *mut u8,
) -> *mut u8;

// Box::leaked + OnceCell-of-&'static-ref pattern. Set BEFORE enable() so the
// handler always finds it; the `&GenericDetour<F>::call(...)` then uses the
// retour-managed trampoline (saved original prologue bytes) instead of the
// patched function, so we never recurse into ourselves.
static IMAGE_DETOUR: OnceCell<&'static GenericDetour<CreateDecoratorFn>> = OnceCell::new();
static AP_DETOUR: OnceCell<&'static GenericDetour<CreateDecoratorFn>> = OnceCell::new();

// Per-class fire counters — log first 5 + every power-of-two thereafter so
// we get clear evidence the detour fired without spamming the log file when
// SCUM renders a chat window with hundreds of inline icons.
static IMAGE_FIRE_COUNT: AtomicU64 = AtomicU64::new(0);
static AP_FIRE_COUNT: AtomicU64 = AtomicU64::new(0);

fn should_log_fire(n: u64) -> bool {
    n < 5 || n.is_power_of_two()
}

unsafe extern "C" fn image_create_decorator_detour(
    this: *mut u8,
    return_slot: *mut u8,
    owner: *mut u8,
) -> *mut u8 {
    let n = IMAGE_FIRE_COUNT.fetch_add(1, Ordering::Relaxed);
    if should_log_fire(n) {
        logging::log(&format!(
            "[hooks] CreateDecorator [Image] fire #{n} this={this:p} owner={owner:p}"
        ));
    }
    // Forward to original via the retour-managed trampoline. Calling the
    // detour's `.call()` skips the patched bytes, so engine-stock rendering
    // still works for every <img/> tag — we are observe-only at this stage.
    match IMAGE_DETOUR.get() {
        Some(d) => d.call(this, return_slot, owner),
        None => {
            // Should be impossible: handler can only fire after enable(),
            // and we set the OnceCell before enable(). Log loudly and
            // return the return_slot so the caller doesn't crash on a
            // null-deref of the would-be TSharedPtr.
            logging::log("[hooks] FATAL: IMAGE_DETOUR fired before set!");
            return_slot
        }
    }
}

unsafe extern "C" fn ap_create_decorator_detour(
    this: *mut u8,
    return_slot: *mut u8,
    owner: *mut u8,
) -> *mut u8 {
    let n = AP_FIRE_COUNT.fetch_add(1, Ordering::Relaxed);
    if should_log_fire(n) {
        logging::log(&format!(
            "[hooks] CreateDecorator [AP] fire #{n} this={this:p} owner={owner:p}"
        ));
    }
    match AP_DETOUR.get() {
        Some(d) => d.call(this, return_slot, owner),
        None => {
            logging::log("[hooks] FATAL: AP_DETOUR fired before set!");
            return_slot
        }
    }
}

/// Install retour detours on Image's and AP's `CreateDecorator` vtable[idx].
/// Both detours are observe-only at this stage — handler logs the fire and
/// forwards to the original via the trampoline. Phase B step 2b will replace
/// the forward with real markup parsing + texture marshal.
fn install_create_decorator_detours(
    report: &engine_resolve::ProbeReport,
    idx: usize,
) -> Result<(), String> {
    use engine_resolve::PARENT_INDEX;

    // Indices in DECORATOR_CLASS_NAMES: 0 parent, 1 Image, 2 ActionPrompt.
    // We installed one entry per child; map them back here.
    let mut children: Vec<(&str, &engine_resolve::ResolvedClass, &OnceCell<&'static GenericDetour<CreateDecoratorFn>>, CreateDecoratorFn)> = Vec::new();
    for (i, c) in report.classes.iter().enumerate() {
        if i == PARENT_INDEX { continue; }
        let cell: &OnceCell<&'static GenericDetour<CreateDecoratorFn>>;
        let handler: CreateDecoratorFn;
        match c.name {
            "RichTextBlockImageDecorator" => {
                cell = &IMAGE_DETOUR;
                handler = image_create_decorator_detour;
            }
            "RichTextBlockActionPromptDecorator" => {
                cell = &AP_DETOUR;
                handler = ap_create_decorator_detour;
            }
            other => {
                logging::log(&format!(
                    "[hooks] skipping unknown child class for detour: {other}"
                ));
                continue;
            }
        }
        children.push((c.name, c, cell, handler));
    }

    let mut installed = 0usize;
    for (label, c, cell, handler) in children {
        let Some(cdo) = c.cdo else {
            logging::log(&format!("[hooks] {label}: CDO missing — skipping detour"));
            continue;
        };
        let Some(target) = (unsafe { vtable_slot(cdo, idx) }) else {
            logging::log(&format!(
                "[hooks] {label}: vtable[{idx}] read returned null — skipping detour"
            ));
            continue;
        };
        let target_fn: CreateDecoratorFn = unsafe { std::mem::transmute(target) };
        logging::log(&format!(
            "[hooks] {label}: installing detour at vtable[{idx}] = {target:p}"
        ));
        let detour = unsafe {
            GenericDetour::new(target_fn, handler).map_err(|e| {
                format!("{label}: GenericDetour::new failed: {e}")
            })?
        };
        // Leak so we can hand a 'static reference to the OnceCell. The
        // process-lifetime ownership matches retour's safety story (the
        // detour must outlive every call site).
        let leaked: &'static GenericDetour<CreateDecoratorFn> = Box::leak(Box::new(detour));
        // Set the cell BEFORE enabling — once enable() returns, the next
        // CreateDecorator call lands in our handler which reads the cell.
        cell.set(leaked).map_err(|_| format!("{label}: detour cell already set"))?;
        unsafe { leaked.enable().map_err(|e| format!("{label}: enable failed: {e}"))?; }
        logging::log(&format!("[hooks] {label}: detour ENABLED (observe-only stub)"));
        installed += 1;
    }

    logging::log(&format!("[hooks] {installed} CreateDecorator detour(s) installed"));
    Ok(())
}

// ---------- Instance survey ----------
//
// Detour-fire validation: walk GUObjectArray on a 10s tick for 60s and log
// (a) how many decorator-class instances exist, and (b) the per-class fire
// counter from the detour handlers.
//
// Why: `CreateDecorator` is called at URichTextBlock construction, NOT per
// `<img/>` tag, and only ONCE per registered decorator on each block. SCUM's
// main menu may not construct any URichTextBlock with these decorators, in
// which case fires=0 is EXPECTED, not a bug. The survey distinguishes:
//
//   * decorator instance count > 0, fires == 0
//      → detour install pointed at the wrong vtable slot, OR the ABI is
//        wrong, OR these instances were constructed before our DLL attached
//        (race). Worth investigating.
//
//   * decorator instance count > 0, fires > 0
//      → detour fires as designed. Move on to the body.
//
//   * decorator instance count == 0, fires == 0
//      → SCUM's current screen doesn't use these decorators. User needs to
//        navigate to a screen that does (in-game HUD, chat panel, action-
//        prompt overlay) before fires would happen.
//
// Bonus: also count URichTextBlock subclass instances if URichTextBlock's
// UClass can be resolved from GUObjectArray. That number tells us how many
// rich-text widgets are alive — each is a potential CreateDecorator caller.
fn spawn_instance_survey(report: &engine_resolve::ProbeReport) {
    use engine_resolve::PARENT_INDEX;

    // Snapshot what we need before spawning so the closure has plain Send
    // values, not a &report borrow.
    let Some(arr_ptr) = report.guarray_addr else {
        logging::log("[hooks] survey skipped: GUObjectArray unresolved");
        return;
    };
    let arr_ptr = arr_ptr as usize; // Send

    // Pointers don't implement Send. The runtime safety is the same either
    // way (we're saying "the underlying address is stable for the survey's
    // lifetime"), so the cleanest cross-thread story is to demote each
    // pointer to a usize for the move and re-cast inside the closure.
    let mut targets: Vec<(String, usize)> = Vec::new();
    for (i, c) in report.classes.iter().enumerate() {
        if i == PARENT_INDEX { continue; }
        if let Some(uc) = c.uclass {
            targets.push((c.name.to_string(), uc.0 as usize));
        }
    }
    let parent_uclass: Option<usize> = report.classes.get(PARENT_INDEX)
        .and_then(|c| c.uclass).map(|u| u.0 as usize);

    // Also resolve URichTextBlock itself if we have the FName ctor handy —
    // every concrete rich-text-block instance is a subclass of it, and its
    // count tells us "how many text widgets exist that COULD have a
    // decorator". Best-effort: skip silently on miss.
    let urtb_uclass: Option<usize> = resolve_urichtextblock_uclass(arr_ptr as *const u8)
        .map(|u| u.0 as usize);

    std::thread::spawn(move || {
        let arr = unsafe { GUObjectArray::from_global(arr_ptr as *const u8) };
        const TICKS: u32 = 6;
        const TICK_MS: u64 = 10_000;
        for tick in 0..TICKS {
            std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
            let secs = (tick + 1) as u64 * (TICK_MS / 1000);

            let mut row = format!("[hooks] survey @ +{secs:02}s:");
            for (name, uc_addr) in &targets {
                let uc = UClassPtr(*uc_addr as *const u8);
                let n = unsafe { count_class_instances(&arr, uc) };
                let label = short_class_label(name);
                row.push_str(&format!(" {label}_inst={n}"));
            }
            // Per-handler atomic fire counts (load semantics — readers don't
            // care about ordering with the increment, only "what's the
            // latest visible value").
            let img_fires = IMAGE_FIRE_COUNT.load(Ordering::Relaxed);
            let ap_fires = AP_FIRE_COUNT.load(Ordering::Relaxed);
            row.push_str(&format!(" image_fire={img_fires} ap_fire={ap_fires}"));

            if let Some(parent_addr) = parent_uclass {
                let parent = UClassPtr(parent_addr as *const u8);
                let n_parent_subs = unsafe { count_subclass_instances(&arr, parent) };
                row.push_str(&format!(" parent_subclass_inst={n_parent_subs}"));
            }
            if let Some(urtb_addr) = urtb_uclass {
                let urtb = UClassPtr(urtb_addr as *const u8);
                let n_urtb = unsafe { count_subclass_instances(&arr, urtb) };
                row.push_str(&format!(" urichtextblock_inst={n_urtb}"));
            }
            logging::log(&row);
        }
        logging::log("[hooks] survey complete");
    });
}

/// Best-effort URichTextBlock UClass lookup. Same pattern as the probe's
/// class-resolution loop but standalone so the survey doesn't depend on it
/// having been resolved earlier.
fn resolve_urichtextblock_uclass(arr_ptr: *const u8) -> Option<UClassPtr> {
    let ctor = sigscan_ext::find_fname_ctor()?;
    let arr = unsafe { GUObjectArray::from_global(arr_ptr) };
    let fname = unsafe {
        engine_resolve::construct_fname_via_engine(ctor, "RichTextBlock")
    }?;
    let uobj = unsafe { arr.find_by_fname(fname) }?;
    Some(UClassPtr(uobj.0))
}

/// Compress the long class name into a short label for the log row so we
/// don't blow past readable line widths when both classes are reported.
fn short_class_label(class_name: &str) -> &'static str {
    if class_name.contains("Image") { "image" }
    else if class_name.contains("ActionPrompt") { "ap" }
    else { "other" }
}

/// Disable all installed detours. Called from DLL_PROCESS_DETACH.
pub fn uninstall() {
    let Ok(mut reg) = registry().lock() else { return; };
    let n = reg.handles.len();
    reg.handles.clear(); // Drop runs on each, disabling the detours.
    logging::log(&format!("[hooks] uninstall — disabled {n} detour(s)"));
}

// ---------- smoke test ----------

/// Detour-install validation that runs without any UE4 dependency.
///
/// Detours an in-DLL function whose original behaviour is known, calls it
/// through the detour-public API, and asserts both that the detour body
/// runs *and* that the trampoline can reach the original. If this passes
/// the `retour` integration is healthy; remaining failure modes are
/// engine-specific (sigscan miss, vtable-layout drift).
// `#[inline(never)]` + `#[no_mangle]` keep the compiler from inlining or
// merging-by-value, so the function has a stable address retour can patch.
// Calling through the explicit function pointer in the test forces dispatch
// through that address rather than a possibly-inlined copy.
#[inline(never)]
#[no_mangle]
extern "C" fn smoke_target(x: i32) -> i32 {
    // Black-box prevents the optimizer from constant-folding through us.
    std::hint::black_box(x) + 1
}

extern "C" fn smoke_replacement(x: i32) -> i32 {
    x * 100
}

#[allow(dead_code)]
pub fn install_smoke_hook() -> Result<(), String> {
    let target_fp: extern "C" fn(i32) -> i32 = smoke_target;
    unsafe {
        let detour = GenericDetour::new(target_fp, smoke_replacement)
            .map_err(|e| format!("smoke detour create: {e}"))?;
        detour.enable().map_err(|e| format!("smoke detour enable: {e}"))?;

        // While the detour is enabled, calling through the function pointer
        // lands in the replacement.
        let observed = target_fp(7);
        if observed != 700 {
            return Err(format!("smoke detour did not redirect: got {observed}, expected 700"));
        }

        // The trampoline (= the original function bytes) must still be
        // callable. retour's `call()` is the safe way to reach it.
        let via_trampoline = detour.call(7);
        if via_trampoline != 8 {
            return Err(format!("smoke trampoline broken: got {via_trampoline}, expected 8"));
        }

        // Disable validates teardown.
        detour.disable().map_err(|e| format!("smoke detour disable: {e}"))?;
        let after = target_fp(7);
        if after != 8 {
            return Err(format!("smoke detour did not lift on disable: got {after}, expected 8"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_hook_install_and_lift() {
        // Skip on non-x86_64; retour's inline detour requires it.
        #[cfg(target_arch = "x86_64")]
        install_smoke_hook().expect("smoke hook should pass");
    }

    #[test]
    fn registry_is_empty_until_kept() {
        let reg = HookRegistry::new();
        assert_eq!(reg.count(), 0);
    }
}
