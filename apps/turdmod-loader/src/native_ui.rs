//! Native UMG display — puts a cooked UMG widget (WBP_TurdMODWelcome) on the
//! player's screen using the engine's own UFunctions, NOT ImGui.
//!
//! Path: hook UGameEngine::Tick (game thread) -> when a show is requested, call
//! UWidgetBlueprintLibrary::Create(world, WBPClass, PC) then UUserWidget::
//! AddToViewport(0), both via UObject::ProcessEvent. All three live on the game
//! thread, which is mandatory — UMG/Slate fault if touched off-thread.
//!
//! @dep player.rs: resolve_process_event (GEngine vtable[68]), resolve_pc,
//!      find_object (GUObjectArray name walk), img_base.
//! @inv WBP_TurdMODWelcome_C only exists once TurdMODWelcome_P.pak is mounted
//!      (client pak-bypass 6/6). @brk if Tick's signature changes across builds.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use once_cell::sync::OnceCell;
use retour::GenericDetour;

use crate::logging;
use crate::player;

// UGameEngine::Tick(this, float DeltaSeconds, bool bIdleMode). patternsleuth,
// current SCUM.exe (2026-06-14). Game-thread, once per frame.
const RVA_UGAMEENGINE_TICK: usize = 0x4499df0;

type TickFn = extern "system" fn(usize, f32, bool);

static TICK: OnceCell<GenericDetour<TickFn>> = OnceCell::new();
static SHOW_PENDING: AtomicBool = AtomicBool::new(false);
static SHOWN: AtomicBool = AtomicBool::new(false);
// Exo-suit mech (Stage 1): possess a Sentry-derived Character so the player walks
// it with their own kb/m. MOUNT/DISMOUNT serviced on the game thread (tick).
static MOUNT_PENDING: AtomicBool = AtomicBool::new(false);
static DISMOUNT_PENDING: AtomicBool = AtomicBool::new(false);
static ORIGINAL_PAWN: AtomicUsize = AtomicUsize::new(0); // the player's real body, for dismount
static MECH_PAWN: AtomicUsize = AtomicUsize::new(0); // the body we're piloting (0 = not mounted)

#[inline]
fn key_down(vk: i32) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
}

// Only treat keys as flight input when OUR game window is FOCUSED — otherwise typing in
// another app (e.g. chat) drives the drone. (GetAsyncKeyState is global; gate it.)
fn game_focused() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd as usize == 0 { return false; }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid == GetCurrentProcessId()
    }
}

// ─── Heli flight input → server physics (heliControl over HTTP) ───────────────
// A dedicated thread reads the heli flight keys (arrows + PageUp/Down + Home/End,
// chosen so they DON'T move the on-foot character) and streams heliControl to the
// bridge at ~25 Hz. The bridge's custom integrator does the physics. Toggled by the
// `heliFlight` IPC method. @dep server bridge heliControl RPC on :9090.
static HELI_FLIGHT: AtomicBool = AtomicBool::new(false);
static HELI_FLIGHT_ONCE: std::sync::Once = std::sync::Once::new();
// Client-side native-physics heli: the force-model in tick_hook drives this body.
static HELI_PHYS_BODY: AtomicUsize = AtomicUsize::new(0);     // simulating StaticMeshComponent
static HELI_PHYS_YAW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0); // heading deg (f32 bits)
// control state, set by the heliFlight keyboard thread, read by tick_hook:
static HELI_COLL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);     // collective SETTING 0..1 (f32 bits); 0.5 = hover (wheel ratchets it)
static HELI_PIT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);      // analog -1..1 cyclic (f32 bits)
static HELI_ROL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);      // analog -1..1 cyclic (f32 bits)
static HELI_YAWC: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);     // -1/0/1 (A/D)
static HELI_AH: AtomicBool = AtomicBool::new(false);
static HELI_FREELOOK: AtomicBool = AtomicBool::new(false); // Alt held → suspend cyclic, let you look around
// Live-tunable force constants (f32 bits; 0 = use default). Set via the `heliTune` IPC so we
// can dial in the feel WITHOUT a rebuild+relaunch. cm/s² (accel, bAccelChange) for lift/cyclic;
// deg/s for yaw rate. @inv hover ≈ HELI_THRUST*0.445 ≈ gravity(980).
static HELI_THRUST:  std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static HELI_FWD:     std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static HELI_STR:     std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static HELI_YAWRATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[inline] fn tunable(a: &std::sync::atomic::AtomicU32, default: f32) -> f32 {
    let v = f32::from_bits(a.load(Ordering::Relaxed)); if v > 0.0 { v } else { default }
}

// Rotor force-model on OUR simulating body — called every game-thread tick. The engine
// integrates gravity + momentum; we apply collective lift + cyclic thrust + yaw rate.
unsafe fn heli_physics_tick(dt: f32) {
    let body = HELI_PHYS_BODY.load(Ordering::SeqCst);
    if body == 0 { return; }
    // @inv body liveness — a connection drop / world teardown FREES our client-only heli
    // actor; calling AddForce on the freed body = dangling-pointer AV (the EXCEPTION_ACCESS_
    // VIOLATION @ body+0xc fatal). Gate every tick: (1) player still in world (resolve_pc =
    // None once the local player's controller is torn down), (2) the body still looks like a
    // live UObject (vtable in-image). On either failure, drop the heli refs and stop — clean
    // stop instead of a crash. @brk if resolve_pc/looks_uobject get expensive (both are cheap
    // guarded pointer-reads, safe per-tick).
    if player::resolve_pc().is_none() || !player::looks_uobject(body) {
        HELI_PHYS_BODY.store(0, Ordering::SeqCst);
        HELI_ACTOR.store(0, Ordering::SeqCst);
        return;
    }
    // Resolve once + cache — find_func is a full object-array scan; calling it every tick
    // tanks FPS. Cache ProcessEvent + the two UFunctions after first resolve.
    static PE: AtomicUsize = AtomicUsize::new(0);
    static AF: AtomicUsize = AtomicUsize::new(0);
    static AV: AtomicUsize = AtomicUsize::new(0);
    let mut pe_addr = PE.load(Ordering::Relaxed);
    if pe_addr == 0 { pe_addr = player::resolve_process_event().unwrap_or(0); PE.store(pe_addr, Ordering::Relaxed); }
    if pe_addr == 0 { return; }
    let pe: extern "system" fn(usize, usize, *mut std::os::raw::c_void) = std::mem::transmute(pe_addr);
    let mut af = AF.load(Ordering::Relaxed);
    if af == 0 { af = find_func("AddForce", "PrimitiveComponent").unwrap_or(0); AF.store(af, Ordering::Relaxed); }
    let mut av = AV.load(Ordering::Relaxed);
    if av == 0 { av = find_func("SetPhysicsAngularVelocityInDegrees", "PrimitiveComponent").unwrap_or(0); AV.store(av, Ordering::Relaxed); }
    // collective is a 0..1 SETTING the wheel ratchets (0.5 = hover). climb cmd = coll*2-1.
    let mut climb = f32::from_bits(HELI_COLL.load(Ordering::SeqCst)) * 2.0 - 1.0;
    let (mut pit, mut rol, mut yawc) = (f32::from_bits(HELI_PIT.load(Ordering::SeqCst)), f32::from_bits(HELI_ROL.load(Ordering::SeqCst)), HELI_YAWC.load(Ordering::SeqCst) as f32);
    // Cyclic self-centers: decay pitch/roll toward 0 each tick so when you stop moving the mouse
    // it levels out (no drift). The mouse hook re-adds while you move → sustained tilt while moving.
    HELI_PIT.store((pit * 0.90).to_bits(), Ordering::SeqCst);
    HELI_ROL.store((rol * 0.90).to_bits(), Ordering::SeqCst);
    if HELI_AH.load(Ordering::SeqCst) { climb = 0.0; pit = 0.0; rol = 0.0; yawc = 0.0; } // hover-lock = neutral inputs → damping parks it
    // Live-tunable (heliTune). thrust = vertical (climb) authority; fwd/strafe = cyclic; yaw = deg/s.
    let climb_auth = tunable(&HELI_THRUST, 1800.0);
    let fwd = tunable(&HELI_FWD, 4200.0);
    let strafe = tunable(&HELI_STR, 2800.0);
    let yawrate = tunable(&HELI_YAWRATE, 150.0);

    // Read OUR body's velocity → damp drift = a REAL hold: hands-off (climb=0, no cyclic) the body
    // brakes to a stop + holds altitude instead of floating away. GetPhysicsLinearVelocity(FName)→FVector.
    static GV: AtomicUsize = AtomicUsize::new(0);
    let mut gv = GV.load(Ordering::Relaxed);
    if gv == 0 { gv = find_func("GetPhysicsLinearVelocity", "PrimitiveComponent").unwrap_or(0); GV.store(gv, Ordering::Relaxed); }
    let (vx, vy, vz) = if gv != 0 {
        #[repr(C)] struct GetVel { bone: u64, rx: f32, ry: f32, rz: f32 }
        let mut g = GetVel { bone: 0, rx: 0.0, ry: 0.0, rz: 0.0 };
        pe(body, gv, &mut g as *mut _ as *mut std::os::raw::c_void);
        (g.rx, g.ry, g.rz)
    } else { (0.0, 0.0, 0.0) };
    const GRAVITY: f32 = 980.0; // cancel engine gravity so climb=0 holds altitude
    const DAMP_H: f32 = 1.3;    // horizontal velocity damping (air-drag feel + auto-brake on release)
    const DAMP_V: f32 = 3.0;    // vertical damping (tight altitude hold)

    let mut yaw = f32::from_bits(HELI_PHYS_YAW.load(Ordering::SeqCst));
    // yaw via angular velocity (upright + turn): SetPhysicsAngularVelocityInDegrees(FVector,bool,FName)
    if av != 0 {
        #[repr(C)] struct Ang { x: f32, y: f32, z: f32, badd: u8, _p: [u8; 3], bc: i32, bn: i32 }
        let mut a = Ang { x: 0.0, y: 0.0, z: yawc * yawrate, badd: 0, _p: [0; 3], bc: 0, bn: 0 };
        pe(body, av, &mut a as *mut _ as *mut std::os::raw::c_void);
    }
    yaw += yawc * yawrate * dt;
    HELI_PHYS_YAW.store(yaw.to_bits(), Ordering::SeqCst);
    let yr = yaw.to_radians(); let (cy, sy) = (yr.cos(), yr.sin());
    let fa = pit * fwd; let sa = rol * strafe;
    // Cyclic in world frame (rotated by heading) MINUS horizontal velocity damping (auto-brake).
    let fx = fa * cy - sa * sy - DAMP_H * vx;
    let fy = fa * sy + sa * cy - DAMP_H * vy;
    // Vertical: gravity-comp + climb command - vertical damping → climb=0 holds altitude.
    let up = GRAVITY + climb * climb_auth - DAMP_V * vz;
    // AddForce(FVector Force, FName BoneName, bool bAccelChange=true) — accelerations
    if af != 0 {
        #[repr(C)] struct Force { x: f32, y: f32, z: f32, bc: i32, bn: i32, accel: u8 }
        let mut fp = Force { x: fx, y: fy, z: up, bc: 0, bn: 0, accel: 1 };
        pe(body, af, &mut fp as *mut _ as *mut std::os::raw::c_void);
    }
}

fn post_heli_control(body: &str) {
    use std::io::Write;
    let token = std::env::var("TURDMOD_ENGINE_TOKEN").unwrap_or_default();
    if let Ok(mut s) = std::net::TcpStream::connect("127.0.0.1:9090") {
        let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(150)));
        let req = format!(
            "POST /engine/rpc HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            token, body.len(), body);
        let _ = s.write_all(req.as_bytes());
    }
}

// ─── Real control scheme: low-level mouse+keyboard hooks ──────────────────────
// Joel's spec: mouse WHEEL = collective (down=up), mouse MOVE = cyclic tilt, A/D = yaw,
// Alt(hold) = free-look, Shift = hover-lock. The LL hooks read the wheel reliably (raw-input
// safe) and SWALLOW handled events so SCUM's char/camera don't also react. Cyclic-via-mouse
// works while camera-follow is on (view target is our heli, so SCUM's look has nothing to steer).
// Arrow keys remain a cyclic FALLBACK via GetAsyncKeyState if the mouse fights the game.
static HOOK_ONCE: std::sync::Once = std::sync::Once::new();
static LAST_MX: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static LAST_MY: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static HAVE_LAST: AtomicBool = AtomicBool::new(false);

// Only act when a heli is live AND our game window is focused (never swallow input system-wide).
fn heli_input_active() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    if !(HELI_FLIGHT.load(Ordering::SeqCst) && HELI_PHYS_BODY.load(Ordering::SeqCst) != 0) { return false; }
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd as usize == 0 { return false; }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid == GetCurrentProcessId()
    }
}

unsafe extern "system" fn kbd_ll_proc(code: i32, w: usize, l: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN};
    if code == 0 && heli_input_active() {
        let kb = &*(l as *const KBDLLHOOKSTRUCT);
        let down = w as u32 == WM_KEYDOWN || w as u32 == WM_SYSKEYDOWN;
        match kb.vkCode {
            0x41 => { HELI_YAWC.store(if down { -1 } else { 0 }, Ordering::SeqCst); return 1; } // A = yaw left  (swallow)
            0x44 => { HELI_YAWC.store(if down {  1 } else { 0 }, Ordering::SeqCst); return 1; } // D = yaw right (swallow)
            0x12 => { HELI_FREELOOK.store(down, Ordering::SeqCst); } // Alt = free-look while held (pass through)
            _ => {}
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}

unsafe extern "system" fn mouse_ll_proc(code: i32, w: usize, l: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT, WM_MOUSEWHEEL, WM_MOUSEMOVE};
    if code == 0 && heli_input_active() {
        let ms = &*(l as *const MSLLHOOKSTRUCT);
        match w as u32 {
            WM_MOUSEWHEEL => {
                let delta = ((ms.mouseData >> 16) as i16) as i32; // +120 up notch, -120 down notch
                let mut c = f32::from_bits(HELI_COLL.load(Ordering::SeqCst));
                c += if delta < 0 { 0.07 } else { -0.07 };        // wheel DOWN = collective UP (spec)
                HELI_COLL.store(c.clamp(0.0, 1.0).to_bits(), Ordering::SeqCst);
                return 1; // swallow (no in-game zoom/scroll)
            }
            WM_MOUSEMOVE => {
                if !HELI_FREELOOK.load(Ordering::SeqCst) {
                    let (mx, my) = (ms.pt.x, ms.pt.y);
                    if HAVE_LAST.load(Ordering::SeqCst) {
                        let dx = (mx - LAST_MX.load(Ordering::SeqCst)) as f32;
                        let dy = (my - LAST_MY.load(Ordering::SeqCst)) as f32;
                        const SENS: f32 = 0.06;
                        let p = (f32::from_bits(HELI_PIT.load(Ordering::SeqCst)) + (-dy) * SENS).clamp(-1.0, 1.0); // mouse fwd → pitch fwd
                        let r = (f32::from_bits(HELI_ROL.load(Ordering::SeqCst)) + ( dx) * SENS).clamp(-1.0, 1.0); // mouse right → roll right
                        HELI_PIT.store(p.to_bits(), Ordering::SeqCst);
                        HELI_ROL.store(r.to_bits(), Ordering::SeqCst);
                    }
                    LAST_MX.store(mx, Ordering::SeqCst); LAST_MY.store(my, Ordering::SeqCst); HAVE_LAST.store(true, Ordering::SeqCst);
                    return 1; // swallow so the game camera doesn't also turn
                } else {
                    HAVE_LAST.store(false, Ordering::SeqCst); // reset on free-look → no jump when it ends
                }
            }
            _ => {}
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}

fn install_heli_input_hooks() {
    HOOK_ONCE.call_once(|| {
        std::thread::spawn(|| unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                SetWindowsHookExW, GetMessageW, TranslateMessage, DispatchMessageW,
                WH_KEYBOARD_LL, WH_MOUSE_LL, MSG,
            };
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kbd_ll_proc), std::ptr::null_mut(), 0);
            let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_ll_proc), std::ptr::null_mut(), 0);
            logging::log(&format!("[heli] LL input hooks installed kbd={:#x} mouse={:#x}", kb as usize, ms as usize));
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    });
}

/// Enable heli flight input: low-level mouse/wheel/keys hooks + a keyboard FALLBACK poll
/// (arrows = cyclic, PageUp/Dn = collective ratchet, Shift = hover-lock, V = chase cam).
pub fn request_heli_flight(enable: bool) -> Result<(), String> {
    HELI_FLIGHT.store(enable, Ordering::SeqCst);
    // @ctx NO low-level hooks / no mouse / no A-D-WASD: SCUM reads those via RAW INPUT, which an
    // LL hook canNOT suppress, so they leak to the character (50/50 player-vs-cube control). Use
    // ONLY keys SCUM does NOT bind — arrows, PageUp/Dn, Home/End, Insert/Del — so zero conflict.
    // Real mouse cockpit control needs the MOUNT path (Route-A vehicle), where SCUM suppresses the
    // character itself. Hook fns are kept (dead) as the record of why this approach fails.
    let _ = install_heli_input_hooks; // intentionally NOT called
    HELI_FLIGHT_ONCE.call_once(|| {
        std::thread::spawn(|| {
            let mut autohover = false; let mut prev_ins = false; let mut prev_del = false;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(12)); // ~80 Hz
                if !HELI_FLIGHT.load(Ordering::SeqCst) || HELI_PHYS_BODY.load(Ordering::SeqCst) == 0 { continue; }
                // FOCUS GATE: only fly when the game window is focused; otherwise hold (zero cyclic/yaw)
                // so typing elsewhere can't drive the drone.
                if !game_focused() {
                    HELI_PIT.store(0f32.to_bits(), Ordering::SeqCst);
                    HELI_ROL.store(0f32.to_bits(), Ordering::SeqCst);
                    HELI_YAWC.store(0, Ordering::SeqCst);
                    continue;
                }
                // WASD flight (freed by SetIgnoreMoveInput on mount — these no longer walk you):
                //   W/S = pitch fwd/back · A/D = yaw left/right · Space/PageUp + Ctrl/PageDn = collective up/down
                //   Q/E = strafe (bank) left/right.
                // Collective ratchet — 0..1 setting, 0.5 = hover, HOLDS.
                let mut c = f32::from_bits(HELI_COLL.load(Ordering::SeqCst));
                if key_down(0x20) || key_down(0x21) { c += 0.01; }   // Space / PageUp  → up
                if key_down(0x11) || key_down(0x22) { c -= 0.01; }   // Ctrl  / PageDown → down
                HELI_COLL.store(c.clamp(0.0, 1.0).to_bits(), Ordering::SeqCst);
                // Cyclic — additive into analog pitch/roll; releases self-center via tick decay.
                let mut p = f32::from_bits(HELI_PIT.load(Ordering::SeqCst));
                let mut r = f32::from_bits(HELI_ROL.load(Ordering::SeqCst));
                if key_down(0x57) { p += 0.10; } if key_down(0x53) { p -= 0.10; } // W / S = pitch fwd/back
                if key_down(0x45) { r += 0.10; } if key_down(0x51) { r -= 0.10; } // E / Q = strafe right/left
                HELI_PIT.store(p.clamp(-1.0, 1.0).to_bits(), Ordering::SeqCst);
                HELI_ROL.store(r.clamp(-1.0, 1.0).to_bits(), Ordering::SeqCst);
                // Yaw — A/D turn.
                HELI_YAWC.store(key_down(0x44) as i32 - key_down(0x41) as i32, Ordering::SeqCst); // D / A
                // Insert = hover-lock toggle; Delete = chase-camera toggle. (Non-conflicting keys.)
                let ins = key_down(0x2D);
                if ins && !prev_ins { autohover = !autohover; HELI_AH.store(autohover, Ordering::SeqCst); }
                prev_ins = ins;
                let del = key_down(0x2E);
                if del && !prev_del { HELI_CAM_PENDING.store(if HELI_CAM.load(Ordering::SeqCst) { 2 } else { 1 }, Ordering::SeqCst); }
                prev_del = del;
                let _ = post_heli_control; // (server-bridge path; unused client-side)
            }
        });
    });
    logging::log(&format!("[heli] input {}", if enable { "ENABLED (keyboard-only, non-conflicting keys)" } else { "disabled" }));
    Ok(())
}

// ProcessEvent param-frame byte helpers — we lay out structs (incl. FTransform,
// which needs 16-byte alignment) by exact offset to avoid Rust alignment surprises.
#[inline] fn put_u64(b: &mut [u8], off: usize, v: u64) { b[off..off + 8].copy_from_slice(&v.to_le_bytes()); }
#[inline] fn put_f32(b: &mut [u8], off: usize, v: f32) { b[off..off + 4].copy_from_slice(&v.to_le_bytes()); }
#[inline] fn get_u64(b: &[u8], off: usize) -> u64 { u64::from_le_bytes(b[off..off + 8].try_into().unwrap()) }
// Fill an FTransform at `t`: identity rotation, given translation, unit scale.
// Layout: Quat(x,y,z,w)@+0x00, Translation@+0x10, Scale3D@+0x20.
fn put_identity_xform(b: &mut [u8], t: usize, loc: [f32; 3]) {
    put_f32(b, t + 0x0c, 1.0); // quat W
    put_f32(b, t + 0x10, loc[0]); put_f32(b, t + 0x14, loc[1]); put_f32(b, t + 0x18, loc[2]);
    put_f32(b, t + 0x20, 1.0); put_f32(b, t + 0x24, 1.0); put_f32(b, t + 0x28, 1.0);
}

static MECH_SPAWN_PENDING: AtomicBool = AtomicBool::new(false);
static HELI_SPAWN_PENDING: AtomicBool = AtomicBool::new(false);
static RESKIN_PENDING: AtomicBool = AtomicBool::new(false); // re-skin the nearby vehicle as the heli
static CONNECT_PENDING: AtomicBool = AtomicBool::new(false); // direct-connect to the local server
static HELI_ACTOR: AtomicUsize = AtomicUsize::new(0);   // last spawned heli (for seating)
static HELI_SEATED: AtomicBool = AtomicBool::new(false); // player riding a heli seat?
static HELI_SEAT: AtomicUsize = AtomicUsize::new(0);     // which seat (0..4)
static HELI_ENTER_PENDING: AtomicUsize = AtomicUsize::new(0); // 0=none, else seat+1
static HELI_EXIT_PENDING: AtomicBool = AtomicBool::new(false);
static MOUNT_CMC: AtomicUsize = AtomicUsize::new(0); // the player's CharacterMovementComponent (to restore on exit)
// 5 seats, relative to the heli (X=forward, Y=right, Z=up), cm. Rough — the Poly heli
// is a base shell; refine once we see the interior. [pilot, copilot, rearL, rearC, rearR].
static HELI_SEATS: [[f32; 3]; 5] = [
    [150.0, -55.0, 70.0],  // pilot  (front-left)
    [150.0,  55.0, 70.0],  // copilot (front-right)
    [-40.0, -75.0, 70.0],  // rear-left
    [-40.0,   0.0, 70.0],  // rear-center
    [-40.0,  75.0, 70.0],  // rear-right
];
// Vehicle recolor via SCUM's own paint system (no crash, designed for it).
static PAINT_PENDING: AtomicBool = AtomicBool::new(false);
static PAINT_IDX: AtomicUsize = AtomicUsize::new(0);
// Read-only diagnostic: dump each Laika mesh's component + static-mesh name so we
// can target rims-not-tires, glass, etc. by real asset names instead of guessing.
static PROBE_MESHES_PENDING: AtomicBool = AtomicBool::new(false);

/// Our UGameEngine::Tick detour: run the engine tick, then (game thread) service
/// a pending welcome-panel request exactly once.
extern "system" fn tick_hook(this: usize, dt: f32, idle: bool) {
    if let Some(h) = TICK.get() {
        h.call(this, dt, idle);
    }
    // Native-physics heli: drive our simulating body with the rotor force-model each tick.
    unsafe { heli_physics_tick(if dt > 0.0 && dt < 0.2 { dt } else { 0.016 }); }
    unsafe { heli_seat_follow(); }  // pin the mounted rider to the drone seat each tick
    if SHOW_PENDING.swap(false, Ordering::SeqCst) {
        match unsafe { show_welcome() } {
            Ok(()) => {
                SHOWN.store(true, Ordering::SeqCst);
                logging::log("[native_ui] WBP_TurdMODWelcome added to viewport (native UMG)");
            }
            Err(e) => logging::log(&format!("[native_ui] show_welcome failed: {e}")),
        }
    }
    if MOUNT_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { mount_mech() } {
            logging::log(&format!("[mech] mount failed: {e}"));
        }
    }
    if DISMOUNT_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { dismount_mech() } {
            logging::log(&format!("[mech] dismount failed: {e}"));
        }
    }
    if BECOME_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { become_mech() } {
            logging::log(&format!("[mech] become failed: {e}"));
        }
    }
    if ANIM_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { mech_anim() } {
            logging::log(&format!("[mech] anim failed: {e}"));
        }
    }
    if REVERT_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { revert_mech() } {
            logging::log(&format!("[mech] revert failed: {e}"));
        }
    }
    if HELI_SPAWN_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { spawn_heli() } {
            logging::log(&format!("[heli] spawn failed: {e}"));
        }
    }
    match HELI_CAM_PENDING.swap(0, Ordering::SeqCst) {
        1 => { if let Err(e) = unsafe { heli_camera(true) }  { logging::log(&format!("[heli] cam->heli failed: {e}")); } }
        2 => { if let Err(e) = unsafe { heli_camera(false) } { logging::log(&format!("[heli] cam->self failed: {e}")); } }
        _ => {}
    }
    match HELI_MOUNT_PENDING.swap(0, Ordering::SeqCst) {
        1 => { if let Err(e) = unsafe { heli_mount() }    { logging::log(&format!("[heli] mount failed: {e}")); } }
        2 => { if let Err(e) = unsafe { heli_dismount() } { logging::log(&format!("[heli] dismount failed: {e}")); } }
        _ => {}
    }
    if RESKIN_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { reskin_vehicle() } {
            logging::log(&format!("[reskin] failed: {e}"));
        }
    }
    if CONNECT_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { connect_local() } {
            logging::log(&format!("[connect] failed: {e}"));
        }
    }
    // NO per-tick teleport seating/flight — removed. Real physics + real mounting
    // comes from a real vehicle (see the airplane-based plan), not by moving the actor.
    HELI_ENTER_PENDING.store(0, Ordering::SeqCst);
    HELI_EXIT_PENDING.store(false, Ordering::SeqCst);
    if MECH_SPAWN_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { spawn_mech() } {
            logging::log(&format!("[mech] spawn failed: {e}"));
        }
    }
    if PAINT_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { paint_vehicle() } {
            logging::log(&format!("[paint] failed: {e}"));
        }
    }
    if PROBE_MESHES_PENDING.swap(false, Ordering::SeqCst) {
        if let Err(e) = unsafe { probe_vehicle_meshes() } {
            logging::log(&format!("[probe] failed: {e}"));
        }
    }
    // GAME-THREAD WASD drive: feed the piloted pawn's ControlInputVector right here,
    // synchronized with SCUM's movement tick. Doing this off-thread (old drive thread)
    // raced the character tick and caused EXCEPTION_ACCESS_VIOLATION crashes.
    let pawn = MECH_PAWN.load(Ordering::SeqCst);
    if pawn != 0 {
        let fx = (key_down(0x57) as i32 - key_down(0x53) as i32) as f32; // W - S
        let fy = (key_down(0x44) as i32 - key_down(0x41) as i32) as f32; // D - A
        let mag = (fx * fx + fy * fy).sqrt();
        let v = if mag > 0.0 { [fx / mag, fy / mag, 0.0] } else { [0.0, 0.0, 0.0] };
        unsafe { player::set_control_input(pawn, v); }
    }
}

/// Request entering the mech (possess a Sentry-derived body). Serviced next tick.
pub fn request_mount() -> Result<(), String> {
    install_tick_hook()?;
    MOUNT_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

/// Request exiting the mech (re-possess your real body). Serviced next tick.
pub fn request_dismount() -> Result<(), String> {
    install_tick_hook()?;
    DISMOUNT_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

/// GAME-THREAD ONLY. Possess the nearest Sentry-derived Character so the player's
/// own input drives it (it WALKS via CharacterMovement). Saves the real body first.
unsafe fn mount_mech() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let process_event: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let pc = player::resolve_pc().ok_or("PlayerController null (in world?)")?;

    // Save the real body BEFORE possessing (after possess, resolve finds the mech).
    if ORIGINAL_PAWN.load(Ordering::SeqCst) == 0 {
        if let Ok(p) = player::resolve_pawn() {
            ORIGINAL_PAWN.store(p, Ordering::SeqCst);
        }
    }
    let (target, cls) =
        player::find_possess_target().ok_or("no possessable character found (any NPC nearby?)")?;
    let possess = player::find_object("Possess", Some("Controller"))
        .ok_or("Controller::Possess UFunction not found")?;
    #[repr(C)]
    struct PossessParams {
        pawn: usize,
    }
    let mut p = PossessParams { pawn: target };
    process_event(pc, possess, &mut p as *mut _ as *mut c_void);
    // Drive the possessed body from the player's WASD.
    MECH_PAWN.store(target, Ordering::SeqCst);
    logging::log(&format!(
        "[mech] POSSESSED {cls} @ {target:#x} — you are piloting it now (WASD to walk)"
    ));
    Ok(())
}

/// GAME-THREAD ONLY. Re-possess the player's original body.
unsafe fn dismount_mech() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let process_event: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let pc = player::resolve_pc().ok_or("PlayerController null")?;
    let orig = ORIGINAL_PAWN.load(Ordering::SeqCst);
    if orig == 0 {
        return Err("no saved original body".into());
    }
    let possess = player::find_object("Possess", Some("Controller"))
        .ok_or("Controller::Possess UFunction not found")?;
    #[repr(C)]
    struct PossessParams {
        pawn: usize,
    }
    MECH_PAWN.store(0, Ordering::SeqCst); // stop the WASD drive thread
    let mut p = PossessParams { pawn: orig };
    process_event(pc, possess, &mut p as *mut _ as *mut c_void);
    ORIGINAL_PAWN.store(0, Ordering::SeqCst);
    logging::log("[mech] dismounted — back in your own body");
    Ok(())
}

// --- "Become the mech": swap the PLAYER's mesh+anim to a Sentry's. You ARE the
// exo-suit — full kb/m control (it's still you, no AI, no crash). ---
const OFF_MESH: usize = 0x280; // ACharacter::Mesh (USkeletalMeshComponent*)
const OFF_SKELETALMESH: usize = 0x488; // USkinnedMeshComponent::SkeletalMesh
const OFF_ANIMCLASS: usize = 0x6b8; // USkeletalMeshComponent::AnimClass

static BECOME_PENDING: AtomicBool = AtomicBool::new(false);
static ANIM_PENDING: AtomicBool = AtomicBool::new(false);
static REVERT_PENDING: AtomicBool = AtomicBool::new(false);
static ORIG_MESH: AtomicUsize = AtomicUsize::new(0); // player's original SkeletalMesh, for revert

pub fn request_become_mech() -> Result<(), String> { install_tick_hook()?; BECOME_PENDING.store(true, Ordering::SeqCst); Ok(()) }
pub fn request_mech_anim() -> Result<(), String> { install_tick_hook()?; ANIM_PENDING.store(true, Ordering::SeqCst); Ok(()) }
pub fn request_revert_mech() -> Result<(), String> { install_tick_hook()?; REVERT_PENDING.store(true, Ordering::SeqCst); Ok(()) }

#[repr(C)]
struct SetMeshP { mesh: usize, reinit: u8 }
#[repr(C)]
struct SetAnimP { class_ptr: usize }

/// Find a UFunction by name, preferring an outer-class match but falling back to
/// name-only (SCUM subclasses components, so the outer name may differ from stock).
unsafe fn find_func(name: &str, outer: &str) -> Option<usize> {
    player::find_object(name, Some(outer)).or_else(|| player::find_object(name, None))
}

/// GAME-THREAD ONLY. Swap the player's body mesh to a loaded Sentry's mesh. All
/// offsets discovered by class-scan (build-independent — the Dumper offsets drift).
unsafe fn become_mech() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let process_event: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let player = player::resolve_pawn().map_err(|e| e)?;
    let (player_mesh, poff, pcn) =
        player::find_skeletal_mesh_comp(player).ok_or("player has no SkeletalMeshComponent")?;
    let sentry = player::find_instance_named("Sentry").ok_or("no Sentry loaded nearby (get near one)")?;
    let (sentry_mesh, soff, scn) =
        player::find_skeletal_mesh_comp(sentry).ok_or("sentry has no SkeletalMeshComponent")?;
    let (sk, skoff) = player::find_member_obj(sentry_mesh, "SkeletalMesh", 0x400, 0x600)
        .ok_or("sentry SkeletalMesh asset not found")?;
    logging::log(&format!(
        "[mech] player mesh {pcn}@+{poff:#x} | sentry mesh {scn}@+{soff:#x} | sk asset {sk:#x}@+{skoff:#x}"
    ));
    if ORIG_MESH.load(Ordering::SeqCst) == 0 {
        if let Some((o, _)) = player::find_member_obj(player_mesh, "SkeletalMesh", 0x400, 0x600) {
            ORIG_MESH.store(o, Ordering::SeqCst);
        }
    }
    // SCUM layers the visible character across several skeletal mesh comps and we
    // don't know which one renders — so robot-ify EVERY layer + physics-off each, and
    // LOG each layer (class + current mesh) so we learn the real layout.
    let sentry_class = player::class_name(sentry).unwrap_or_default();
    let comps = player::find_all_skeletal_mesh_comps(player);
    logging::log(&format!("[mech] donor Sentry class='{sentry_class}' | player has {} mesh layers", comps.len()));
    let set_mesh = find_func("SetSkeletalMesh", "SkeletalMeshComponent");
    let set_sim = find_func("SetSimulatePhysics", "PrimitiveComponent");
    let set_allsim = find_func("SetAllBodiesSimulatePhysics", "SkeletalMeshComponent");
    for comp in &comps {
        let cn = player::class_name(*comp).unwrap_or_default();
        let cur = player::find_member_obj(*comp, "SkeletalMesh", 0x400, 0x600).map(|(m, _)| m).unwrap_or(0);
        logging::log(&format!("[mech]   layer {comp:#x} {cn} curMesh={cur:#x}"));
        if let Some(f) = set_mesh {
            let mut p = SetMeshP { mesh: sk, reinit: 1 };
            process_event(*comp, f, &mut p as *mut _ as *mut c_void);
        }
        if let Some(f) = set_sim { let mut z = [0u8; 0x10]; process_event(*comp, f, z.as_mut_ptr() as *mut c_void); }
        if let Some(f) = set_allsim { let mut z = [0u8; 0x10]; process_event(*comp, f, z.as_mut_ptr() as *mut c_void); }
    }
    // walk-anim on the primary body comp
    if let (Some((anim, _)), Some(set_anim)) = (
        player::find_member_class(sentry_mesh, "AnimInstance", 0x600, 0x800),
        find_func("SetAnimClass", "SkeletalMeshComponent"),
    ) {
        let mut p = SetAnimP { class_ptr: anim };
        process_event(player_mesh, set_anim, &mut p as *mut _ as *mut c_void);
    }
    logging::log("[mech] MOUNTED — robot mesh on all layers, physics off, walk-anim 🦾");
    Ok(())
}

/// GAME-THREAD ONLY. Apply the Sentry's AnimClass so the mech mesh walk-animates.
unsafe fn mech_anim() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let process_event: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let player = player::resolve_pawn().map_err(|e| e)?;
    let (player_mesh, _, _) =
        player::find_skeletal_mesh_comp(player).ok_or("player has no SkeletalMeshComponent")?;
    let sentry = player::find_instance_named("Sentry").ok_or("no Sentry loaded nearby")?;
    let (sentry_mesh, _, _) =
        player::find_skeletal_mesh_comp(sentry).ok_or("sentry has no SkeletalMeshComponent")?;
    let (anim, animoff) = player::find_member_class(sentry_mesh, "AnimInstance", 0x600, 0x800)
        .ok_or("sentry AnimClass not found")?;
    let set_anim = find_func("SetAnimClass", "SkeletalMeshComponent")
        .ok_or("SetAnimClass UFunction not found")?;
    let mut p = SetAnimP { class_ptr: anim };
    process_event(player_mesh, set_anim, &mut p as *mut _ as *mut c_void);
    logging::log(&format!("[mech] anim applied — Sentry AnimClass {anim:#x}@+{animoff:#x} (should walk-animate)"));
    Ok(())
}

/// GAME-THREAD ONLY. Restore the player's original mesh.
unsafe fn revert_mech() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let process_event: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let player = player::resolve_pawn().map_err(|e| e)?;
    let (player_mesh, _, _) =
        player::find_skeletal_mesh_comp(player).ok_or("player has no SkeletalMeshComponent")?;
    let orig = ORIG_MESH.load(Ordering::SeqCst);
    if orig == 0 { return Err("no saved original mesh".into()); }
    let set_mesh = find_func("SetSkeletalMesh", "SkeletalMeshComponent")
        .ok_or("SetSkeletalMesh UFunction not found")?;
    let mut p = SetMeshP { mesh: orig, reinit: 1 };
    process_event(player_mesh, set_mesh, &mut p as *mut _ as *mut c_void);
    // re-show all the gear/clothing layers we hid on mount
    if let Some(setvis) = find_func("SetVisibility", "SceneComponent") {
        for comp in player::find_all_skeletal_mesh_comps(player) {
            let mut vis = [0u8; 0x10]; // bNewVisibility=true, bPropagateToChildren=true
            vis[0] = 1; vis[1] = 1;
            process_event(comp, setvis, vis.as_mut_ptr() as *mut c_void);
        }
    }
    ORIG_MESH.store(0, Ordering::SeqCst);
    logging::log("[mech] DISMOUNTED — back to your normal body + gear");
    Ok(())
}

pub fn request_spawn_mech() -> Result<(), String> {
    install_tick_hook()?;
    MECH_SPAWN_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn request_spawn_heli() -> Result<(), String> {
    install_tick_hook()?;
    HELI_SPAWN_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn request_connect_local() -> Result<(), String> {
    install_tick_hook()?;
    CONNECT_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

// Force the client to direct-connect to the local server via the engine's `open <ip>`
// console command — bypasses the (flaky) Steam server browser. PlayerController::
// ConsoleCommand(FString, bool) on the menu/world PC.
unsafe fn connect_local() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let pc = player::resolve_pc().ok_or("no PlayerController (try again from the menu)")?;
    let cc = find_func("ConsoleCommand", "PlayerController").ok_or("ConsoleCommand fn not found")?;
    let cmd: Vec<u16> = "open 127.0.0.1:7042".encode_utf16().chain(std::iter::once(0)).collect();
    #[repr(C)] struct FStr { ptr: u64, num: i32, max: i32 }
    #[repr(C)] struct CCParams { command: FStr, write_log: u8, _p: [u8; 7], ret: FStr }
    let mut p = CCParams {
        command: FStr { ptr: cmd.as_ptr() as u64, num: cmd.len() as i32, max: cmd.len() as i32 },
        write_log: 1, _p: [0; 7],
        ret: FStr { ptr: 0, num: 0, max: 0 },
    };
    pe(pc, cc, &mut p as *mut _ as *mut c_void);
    logging::log("[connect] issued `open 127.0.0.1:7042`");
    Ok(())
}

static HELI_CAM: AtomicBool = AtomicBool::new(false);
static HELI_CAM_PENDING: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0); // 0=none,1=follow heli,2=back to self

/// GAME-THREAD ONLY. Point the player camera at the heli (follow=true) or back at the
/// player pawn (follow=false) via APlayerController::SetViewTargetWithBlend. NO pawn attach,
/// so it can't drop the netsession (K2_AttachToActor on the pawn kicks the client — proven).
/// → you ride the camera on the heli and never lose it.
unsafe fn heli_camera(follow: bool) -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let pc = player::resolve_pc().ok_or("PlayerController null (in world?)")?;
    let svt = find_func("SetViewTargetWithBlend", "PlayerController")
        .ok_or("SetViewTargetWithBlend not found")?;
    let target = if follow {
        let h = HELI_ACTOR.load(Ordering::SeqCst);
        if h == 0 { return Err("no heli to follow".into()); }
        h
    } else {
        player::resolve_pawn().map_err(|_| "no player pawn".to_string())?
    };
    // SetViewTargetWithBlend(AActor* NewViewTarget, float BlendTime, EViewTargetBlendFunction
    // BlendFunc, float BlendExp, bool bLockOutgoing). Packed param frame by exact offset.
    let mut b = [0u8; 0x20];
    put_u64(&mut b, 0x00, target as u64);
    put_f32(&mut b, 0x08, 0.25); // BlendTime
    b[0x0c] = 0;                 // VTBlend_Linear
    put_f32(&mut b, 0x10, 0.0);  // BlendExp
    b[0x14] = 0;                 // bLockOutgoing
    pe(pc, svt, b.as_mut_ptr() as *mut c_void);
    HELI_CAM.store(follow, Ordering::SeqCst);
    logging::log(&format!("[heli] camera -> {}", if follow { "heli" } else { "self" }));
    Ok(())
}

/// Toggle the heli chase-camera. Serviced on the game thread next tick.
pub fn request_heli_camera(follow: bool) -> Result<(), String> {
    install_tick_hook()?;
    HELI_CAM_PENDING.store(if follow { 1 } else { 2 }, Ordering::SeqCst);
    Ok(())
}

/// Live-tune the rotor force constants (no rebuild/relaunch). Omitted fields keep current value;
/// pass 0 to reset a field to its built-in default. Applies on the very next physics tick.
/// thrust/fwd/strafe = cm/s² accel, yaw = deg/s. Pure atomic writes — no game thread needed.
pub fn request_heli_tune(thrust: Option<f32>, fwd: Option<f32>, strafe: Option<f32>, yaw: Option<f32>) -> Result<(), String> {
    if let Some(v) = thrust { HELI_THRUST.store(v.to_bits(), Ordering::Relaxed); }
    if let Some(v) = fwd    { HELI_FWD.store(v.to_bits(), Ordering::Relaxed); }
    if let Some(v) = strafe { HELI_STR.store(v.to_bits(), Ordering::Relaxed); }
    if let Some(v) = yaw    { HELI_YAWRATE.store(v.to_bits(), Ordering::Relaxed); }
    Ok(())
}

// ─── OUR OWN native MOUNT/SEAT system (no SCUM vehicle, no Possess) ───────────
// Real mount: SEAT the player's own character in the heli cockpit (attach to the heli body
// at a seat offset), suppress on-foot movement, redirect input to flight, cockpit camera.
// The character stays the player's character — we build the whole seat mechanic ourselves.
static HELI_MOUNTED: AtomicBool = AtomicBool::new(false);
static HELI_MOUNT_PENDING: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0); // 0=none,1=mount,2=dismount
static HELI_RIDER: AtomicUsize = AtomicUsize::new(0); // the seated character (to restore on dismount)
// driver-seat offset relative to the heli body origin (cm): X fwd, Y right, Z up.
// Front of the cube, ~motorcycle(Cruiser)-seat height.
static HELI_SEAT_OFFSET: [f32; 3] = [70.0, 0.0, 70.0];

// Per-tick SEAT: pin the rider to the seat point on the front of the drone (the attach alone
// gets overridden by the server/movement). Keeps the rider's own rotation → mouse-look free.
unsafe fn heli_seat_follow() {
    if !HELI_MOUNTED.load(Ordering::SeqCst) { return; }
    let rider = HELI_RIDER.load(Ordering::SeqCst);
    let heli = HELI_ACTOR.load(Ordering::SeqCst);
    if rider == 0 || heli == 0 { return; }
    if !player::looks_uobject(rider) || !player::looks_uobject(heli) {
        HELI_MOUNTED.store(false, Ordering::SeqCst); return;
    }
    static GL: AtomicUsize = AtomicUsize::new(0);
    static GR: AtomicUsize = AtomicUsize::new(0);
    static SL: AtomicUsize = AtomicUsize::new(0);
    let pe_addr = match player::resolve_process_event() { Some(p) => p, None => return };
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let mut gl = GL.load(Ordering::Relaxed); if gl == 0 { gl = find_func("K2_GetActorLocation","Actor").unwrap_or(0); GL.store(gl, Ordering::Relaxed); }
    let mut gr = GR.load(Ordering::Relaxed); if gr == 0 { gr = find_func("K2_GetActorRotation","Actor").unwrap_or(0); GR.store(gr, Ordering::Relaxed); }
    let mut sl = SL.load(Ordering::Relaxed); if sl == 0 { sl = find_func("K2_SetActorLocation","Actor").unwrap_or(0); SL.store(sl, Ordering::Relaxed); }
    if gl == 0 || sl == 0 { return; }
    let mut hloc = [0f32; 3]; pe(heli, gl, hloc.as_mut_ptr() as *mut c_void);
    let mut hrot = [0f32; 3]; if gr != 0 { pe(heli, gr, hrot.as_mut_ptr() as *mut c_void); }
    let yr = hrot[1].to_radians(); let (cy, sy) = (yr.cos(), yr.sin());
    let off = HELI_SEAT_OFFSET;
    let sx = off[0]*cy - off[1]*sy; let syy = off[0]*sy + off[1]*cy;
    // K2_SetActorLocation(FVector, bSweep, FHitResult&, bTeleport)->bool. Location-only (keeps
    // rotation → look free). bTeleport=1 (no collision sweep with the drone body).
    #[repr(C)] struct SetLoc { x: f32, y: f32, z: f32, sweep: u8, _p0: [u8; 3], hit: [u8; 0x90], teleport: u8, ret: u8, _p1: [u8; 6] }
    let mut p: SetLoc = std::mem::zeroed();
    p.x = hloc[0]+sx; p.y = hloc[1]+syy; p.z = hloc[2]+off[2]; p.teleport = 1;
    pe(rider, sl, &mut p as *mut _ as *mut c_void);
}

unsafe fn heli_mount() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let heli = HELI_ACTOR.load(Ordering::SeqCst);
    if heli == 0 { return Err("no heli to mount (spawn one first)".into()); }
    if HELI_MOUNTED.load(Ordering::SeqCst) { return Ok(()); }
    let character = player::resolve_pawn().map_err(|e| e)?;

    // 1. SEAT: attach the character to the heli body. EAttachmentRule: KeepRelative=0,KeepWorld=1,SnapToTarget=2.
    //    K2_AttachToActor(AActor* Parent, FName Socket, EAttachmentRule Loc, Rot, Scale, bool bWeld)
    let attach = find_func("K2_AttachToActor", "Actor").ok_or("K2_AttachToActor not found")?;
    #[repr(C)] struct Att { parent: usize, sock_a: i32, sock_b: i32, loc: u8, rot: u8, scale: u8, weld: u8, _p: [u8; 4] }
    // loc=SnapToTarget(2) so you ride the seat, but rot=KeepWorld(1) so mouse-look stays FREE
    // (don't lock your facing to the heli — look around like any vehicle, 1st or 3rd person).
    let mut a = Att { parent: heli, sock_a: 0, sock_b: 0, loc: 2, rot: 1, scale: 0, weld: 0, _p: [0; 4] };
    pe(character, attach, &mut a as *mut _ as *mut c_void);

    // 2. seat offset: place the character at the cockpit (relative to the now-parent heli).
    if let Some(srl) = find_func("K2_SetActorRelativeLocation", "Actor") {
        #[repr(C)] struct Rel { x: f32, y: f32, z: f32, sweep: u8, _p0: [u8; 7], hit: [u8; 0x90], teleport: u8 }
        let mut r: Rel = std::mem::zeroed();
        r.x = HELI_SEAT_OFFSET[0]; r.y = HELI_SEAT_OFFSET[1]; r.z = HELI_SEAT_OFFSET[2]; r.teleport = 1;
        pe(character, srl, &mut r as *mut _ as *mut c_void);
    }

    // 3. FREEZE on-foot input: make the PlayerController ignore move input so WASD no longer
    //    walks the character (it's seated/frozen) — frees WASD for the heli. The real suppression.
    if let Some(pc) = player::resolve_pc() {
        if let Some(simi) = find_func("SetIgnoreMoveInput", "PlayerController") {
            #[repr(C)] struct B { v: u8 } let mut p = B { v: 1 };
            pe(pc, simi, &mut p as *mut _ as *mut c_void);
        }
    }
    // also stop the movement component so it can't keep simulating the body on the ground.
    // SCUM's class is UConZCharacterMovementComponent (NOT the engine "CharacterMovementComponent").
    if let Some(cmc) = player::find_comp_of_owner(character, "ConZCharacterMovementComponent") {
        if let Some(dm) = find_func("DisableMovement", "CharacterMovementComponent")
            .or_else(|| find_func("DisableMovement", "ConZCharacterMovementComponent")) {
            pe(cmc, dm, std::ptr::null_mut());
        }
    }

    HELI_RIDER.store(character, Ordering::SeqCst);
    HELI_MOUNTED.store(true, Ordering::SeqCst);
    let _ = heli_camera(true);                 // cockpit/chase camera rides the heli
    let _ = request_heli_flight(true);         // your input now flies the heli
    logging::log(&format!("[heli] MOUNTED — character {character:#x} seated in heli {heli:#x}"));
    Ok(())
}

unsafe fn heli_dismount() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let character = HELI_RIDER.swap(0, Ordering::SeqCst);
    // restore on-foot input
    if let Some(pc) = player::resolve_pc() {
        if let Some(simi) = find_func("SetIgnoreMoveInput", "PlayerController") {
            #[repr(C)] struct B { v: u8 } let mut p = B { v: 0 };
            pe(pc, simi, &mut p as *mut _ as *mut c_void);
        }
    }
    if character == 0 { HELI_MOUNTED.store(false, Ordering::SeqCst); return Ok(()); }
    // detach from the heli, keeping world transform
    if let Some(det) = find_func("K2_DetachFromActor", "Actor") {
        #[repr(C)] struct Det { loc: u8, rot: u8, scale: u8 }
        let mut d = Det { loc: 1, rot: 1, scale: 1 }; // KeepWorld
        pe(character, det, &mut d as *mut _ as *mut c_void);
    }
    // restore on-foot movement
    if let Some(sm) = find_func("SetMovementMode", "CharacterMovementComponent") {
        if let Some(cmc) = player::find_comp_of_owner(character, "CharacterMovementComponent") {
            #[repr(C)] struct Mm { mode: u8, custom: u8 }
            let mut m = Mm { mode: 1, custom: 0 }; // MOVE_Walking
            pe(cmc, sm, &mut m as *mut _ as *mut c_void);
        }
    }
    HELI_MOUNTED.store(false, Ordering::SeqCst);
    let _ = heli_camera(false);
    logging::log("[heli] DISMOUNTED — back on foot");
    Ok(())
}

/// Mount/dismount our heli (seat the player's character in it). Serviced on the game thread.
pub fn request_heli_mount(mount: bool) -> Result<(), String> {
    install_tick_hook()?;
    HELI_MOUNT_PENDING.store(if mount { 1 } else { 2 }, Ordering::SeqCst);
    Ok(())
}

// Spawn our custom cooked helicopter mesh (SM_TurdHeli) on a plain engine
// StaticMeshActor, in front of the player. Proves the custom-MESH pipeline:
// authored in C++ (FRawMesh) -> cooked -> paked -> rendered in-world.
unsafe fn spawn_heli() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let (player, loc) = player::pawn_and_loc().ok_or("player not in world")?;

    // RECALL/idempotent: destroy a previously-spawned heli so re-firing spawnHeli drops a
    // fresh one in front of you instead of stacking cubes. K2_DestroyActor on a plain
    // StaticMeshActor is game-thread-safe. Also re-aim the camera at us so it doesn't
    // chase a destroyed actor (the new spawn re-follows at the end).
    let prev = HELI_ACTOR.swap(0, Ordering::SeqCst);
    if prev != 0 {
        HELI_PHYS_BODY.store(0, Ordering::SeqCst);
        if HELI_CAM.load(Ordering::SeqCst) { let _ = heli_camera(false); }
        if let Some(d) = find_func("K2_DestroyActor", "Actor") { pe(prev, d, std::ptr::null_mut()); }
    }

    // The PHYSICS body needs COLLISION geometry — SM_TurdHeli (procedural FRawMesh) has none,
    // so SetSimulatePhysics faults. Use the engine cube (has a collision hull) to build a valid
    // simulating body and PROVE native flight; the collidable heli mesh comes after.
    if player::find_object("Cube", None).is_none() { load_package("/Engine/BasicShapes/Cube"); }
    let mesh = player::find_object("Cube", None)
        .or_else(|| { load_package("/Game/TurdMOD/SM_TurdHeli"); player::find_object("SM_TurdHeli", None) })
        .ok_or("no collidable mesh found (engine Cube missing)")?;
    let sma_class = player::find_object("StaticMeshActor", None).ok_or("StaticMeshActor class not found")?;
    let gs = player::find_object("Default__GameplayStatics", None).ok_or("GameplayStatics CDO not found")?;
    let begin = find_func("BeginDeferredActorSpawnFromClass", "GameplayStatics").ok_or("Begin spawn fn not found")?;
    let finish = find_func("FinishSpawningActor", "GameplayStatics").ok_or("FinishSpawningActor not found")?;

    // Real player location via K2_GetActorLocation (the raw memory read is broken on this
    // build). Spawn ~5m in front + 3m up so the physics body has room to settle into a hover.
    let mut ploc = [loc[0], loc[1], loc[2]];
    if let Some(gl) = find_func("K2_GetActorLocation", "Actor") {
        let mut v = [0f32; 3];
        pe(player, gl, v.as_mut_ptr() as *mut c_void);
        if v[0].abs() > 1.0 || v[1].abs() > 1.0 { ploc = v; }
    }
    let spot = [ploc[0] + 500.0, ploc[1], ploc[2] + 300.0];
    let mut b = [0u8; 0x60];
    put_u64(&mut b, 0x00, player as u64);
    put_u64(&mut b, 0x08, sma_class as u64);
    put_identity_xform(&mut b, 0x10, spot);
    b[0x40] = 1; // AlwaysSpawn
    pe(gs, begin, b.as_mut_ptr() as *mut c_void);
    let actor = get_u64(&b, 0x50) as usize;
    if actor == 0 { return Err("BeginDeferredActorSpawnFromClass returned null".into()); }
    let mut f = [0u8; 0x50];
    put_u64(&mut f, 0x00, actor as u64);
    put_identity_xform(&mut f, 0x10, spot);
    pe(gs, finish, f.as_mut_ptr() as *mut c_void);
    let heli = { let r = get_u64(&f, 0x40) as usize; if r != 0 { r } else { actor } };

    // set the cooked mesh on the actor's StaticMeshComponent (make it Movable first
    // so SetStaticMesh re-registers cleanly on the static-by-default actor).
    let smc = player::find_comp_of_owner(heli, "StaticMeshComponent")
        .ok_or("spawned StaticMeshActor has no StaticMeshComponent")?;
    if let Some(set_mob) = find_func("SetMobility", "SceneComponent") {
        #[repr(C)] struct MobP { mob: u8 }
        let mut m = MobP { mob: 2 }; // EComponentMobility::Movable
        pe(smc, set_mob, &mut m as *mut _ as *mut c_void);
    }
    let set_mesh = find_func("SetStaticMesh", "StaticMeshComponent").ok_or("SetStaticMesh not found")?;
    #[repr(C)] struct MeshP { mesh: usize, ret: u8, _pad: [u8; 7] }
    let mut mp = MeshP { mesh, ret: 0, _pad: [0; 7] };
    pe(smc, set_mesh, &mut mp as *mut _ as *mut c_void);
    // CLIENT-SIDE NATIVE PHYSICS: make our own body a REAL simulating rigid body. The client
    // HAS a physics scene (the dedicated server does NOT — server SetSimulatePhysics crashes).
    // The engine then integrates gravity + momentum + collision; the tick force-model (below)
    // applies rotor forces → genuine native physics on a body we own.
    #[repr(C)] struct BoolP { v: u8 }
    if let Some(f) = find_func("SetEnableGravity", "PrimitiveComponent") { let mut p = BoolP { v: 1 }; pe(smc, f, &mut p as *mut _ as *mut c_void); }
    if let Some(f) = find_func("SetSimulatePhysics", "PrimitiveComponent") { let mut p = BoolP { v: 1 }; pe(smc, f, &mut p as *mut _ as *mut c_void); }
    HELI_ACTOR.store(heli, Ordering::SeqCst);
    HELI_PHYS_BODY.store(smc, Ordering::SeqCst);     // the force-model drives this each tick
    HELI_PHYS_YAW.store(0, Ordering::SeqCst);
    HELI_COLL.store((0.5f32).to_bits(), Ordering::SeqCst);   // 0.5 = hover (wheel ratchets up/down from here)
    HELI_AH.store(false, Ordering::SeqCst);           // auto-hover OFF by default → controls respond immediately (Shift engages hover-hold when you want to park)
    let _ = request_heli_flight(true);                // spawn the keyboard thread + enable control
    // Camera follow is OPT-IN (press V / heliCamera) — not auto-applied, so it isn't a confound
    // while we confirm the teardown-safety fix. The view-target on a bare actor is still unproven.
    logging::log(&format!("[heli] NATIVE physics body spawned: actor @ {heli:#x}, body @ {smc:#x} (simulating, auto-hover ON) 🚁"));
    Ok(())
}

/// Re-skin the nearby Kinglet AS the heli. Serviced next tick.
pub fn request_reskin_vehicle() -> Result<(), String> {
    install_tick_hook()?;
    RESKIN_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

/// GAME-THREAD ONLY. Make the player's REAL registered vehicle (a Kinglet) LOOK like
/// the heli: spawn SM_TurdHeli on a StaticMeshActor, ATTACH it to the vehicle (snap to
/// target → rides the vehicle pivot, follows physics — NO per-tick teleport), then HIDE
/// the vehicle's own plane meshes. Location-independent: attach snaps to the vehicle, so
/// it does NOT depend on the (currently broken) player/vehicle world-location read.
/// @dep player::find_mesh_comps_by_owner_class @dep player::safe_rd (owner @ comp+0x20)
unsafe fn reskin_vehicle() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    // WorldContext for the spawn = the player pawn (proven in spawn_heli). The vehicle
    // is NOT a valid WorldContext for BeginDeferredActorSpawnFromClass here (returns null).
    let (player, _loc) = player::pawn_and_loc().ok_or("player not in world")?;

    // Idempotent: destroy a previously-spawned heli skin so re-firing (live tuning)
    // doesn't stack meshes. Game-thread K2_DestroyActor on a plain StaticMeshActor is safe.
    let prev = HELI_ACTOR.swap(0, Ordering::SeqCst);
    if prev != 0 {
        if let Some(destroy) = find_func("K2_DestroyActor", "Actor") {
            pe(prev, destroy, std::ptr::null_mut());
        }
    }

    // 1. Find the Kinglet's visible mesh components + its MAIN CHASSIS actor. The vehicle
    //    is dozens of sub-part actors (BPC_Kinglet_Duster_Wing_..., _Propeller_C, etc.);
    //    the drivable chassis is "BPC_Kinglet_Duster_C" = the SHORTEST BPC_ owner-class
    //    name. Attaching to the chassis (not a sub-part / not the pawn) follows the
    //    vehicle's replicated movement and survives reconnects.
    let meshes = player::find_mesh_comps_by_owner_class("Kinglet");
    if meshes.is_empty() {
        return Err("no Kinglet mesh comps (stand by / sit in the Kinglet first)".into());
    }
    let (vehicle, vclass) = meshes
        .iter()
        .filter(|(_, ocls, _)| ocls.starts_with("BPC_"))
        .min_by_key(|(_, ocls, _)| ocls.len())
        .or_else(|| meshes.first())
        .and_then(|(comp, ocls, _)| player::safe_rd(comp + 0x20).map(|v| (v, ocls.clone())))
        .ok_or("could not resolve vehicle chassis actor (owner @ +0x20 null)")?;
    logging::log(&format!("[reskin] chassis '{vclass}' @ {vehicle:#x}, {} mesh comp(s)", meshes.len()));

    // 2. Load + resolve our cooked heli mesh.
    // The PHYSICS body needs COLLISION geometry — SM_TurdHeli (procedural FRawMesh) has none,
    // so SetSimulatePhysics faults. Use the engine cube (has a collision hull) to build a valid
    // simulating body and PROVE native flight; the collidable heli mesh comes after.
    if player::find_object("Cube", None).is_none() { load_package("/Engine/BasicShapes/Cube"); }
    let mesh = player::find_object("Cube", None)
        .or_else(|| { load_package("/Game/TurdMOD/SM_TurdHeli"); player::find_object("SM_TurdHeli", None) })
        .ok_or("no collidable mesh found (engine Cube missing)")?;
    let sma_class = player::find_object("StaticMeshActor", None).ok_or("StaticMeshActor class not found")?;
    let gs = player::find_object("Default__GameplayStatics", None).ok_or("GameplayStatics CDO not found")?;
    let begin = find_func("BeginDeferredActorSpawnFromClass", "GameplayStatics").ok_or("Begin spawn fn not found")?;
    let finish = find_func("FinishSpawningActor", "GameplayStatics").ok_or("FinishSpawningActor not found")?;

    // 3. Spawn a StaticMeshActor at the origin — the attach (step 5) snaps it onto the
    //    vehicle, so the spawn transform is irrelevant (dodges the broken location read).
    let spot = [0.0f32, 0.0, 0.0];
    let mut b = [0u8; 0x60];
    put_u64(&mut b, 0x00, player as u64); // WorldContext = player pawn (proven in spawn_heli)
    put_u64(&mut b, 0x08, sma_class as u64);
    put_identity_xform(&mut b, 0x10, spot);
    b[0x40] = 1; // AlwaysSpawn
    pe(gs, begin, b.as_mut_ptr() as *mut c_void);
    let actor = get_u64(&b, 0x50) as usize;
    if actor == 0 { return Err("BeginDeferredActorSpawnFromClass returned null".into()); }
    let mut f = [0u8; 0x50];
    put_u64(&mut f, 0x00, actor as u64);
    put_identity_xform(&mut f, 0x10, spot);
    pe(gs, finish, f.as_mut_ptr() as *mut c_void);
    let heli = { let r = get_u64(&f, 0x40) as usize; if r != 0 { r } else { actor } };

    // 4. Make the SMC Movable + set the cooked mesh.
    let smc = player::find_comp_of_owner(heli, "StaticMeshComponent")
        .ok_or("spawned StaticMeshActor has no StaticMeshComponent")?;
    if let Some(set_mob) = find_func("SetMobility", "SceneComponent") {
        #[repr(C)] struct MobP { mob: u8 }
        let mut m = MobP { mob: 2 }; // Movable
        pe(smc, set_mob, &mut m as *mut _ as *mut c_void);
    }
    let set_mesh = find_func("SetStaticMesh", "StaticMeshComponent").ok_or("SetStaticMesh not found")?;
    #[repr(C)] struct MeshP { mesh: usize, ret: u8, _pad: [u8; 7] }
    let mut mp = MeshP { mesh, ret: 0, _pad: [0; 7] };
    pe(smc, set_mesh, &mut mp as *mut _ as *mut c_void);

    // 5. Attach the heli to the MAIN CHASSIS: SnapToTarget loc+rot (ride the vehicle pivot,
    //    follow its replicated movement), KeepWorld scale.
    //    AActor::K2_AttachToActor(ParentActor, FName Socket, EAttachmentRule Loc, Rot,
    //    Scale, bool Weld). EAttachmentRule: KeepRelative=0, KeepWorld=1, SnapToTarget=2.
    let _ = player; // player was the spawn WorldContext; attach target is the chassis
    let attach = find_func("K2_AttachToActor", "Actor").ok_or("K2_AttachToActor not found")?;
    #[repr(C)] struct AttachP { parent: usize, socket: [i32; 2], loc: u8, rot: u8, scale: u8, weld: u8 }
    let mut ap = AttachP { parent: vehicle, socket: [0, 0], loc: 2, rot: 2, scale: 1, weld: 0 };
    pe(heli, attach, &mut ap as *mut _ as *mut c_void);
    HELI_ACTOR.store(heli, Ordering::SeqCst);
    logging::log(&format!("[reskin] heli @ {heli:#x} attached to chassis '{vclass}' @ {vehicle:#x} (snap)"));

    // 6. Hide the vehicle's own plane meshes so only the heli shows. SetVisibility(
    //    bNewVisibility=false, bPropagateToChildren=true) on each MeshComponent.
    let set_vis = find_func("SetVisibility", "SceneComponent");
    let mut hidden = 0;
    for (comp, ocls, ccls) in &meshes {
        if let Some(sv) = set_vis {
            #[repr(C)] struct VisP { visible: u8, propagate: u8 }
            let mut vp = VisP { visible: 0, propagate: 1 };
            pe(*comp, sv, &mut vp as *mut _ as *mut c_void);
            hidden += 1;
            logging::log(&format!("[reskin] hid {ocls}/{ccls} @ {comp:#x}"));
        }
    }
    logging::log(&format!("[reskin] DONE: heli rides the vehicle, {hidden} plane mesh(es) hidden 🚁"));
    Ok(())
}

// ── seating: ride a heli seat (client-side). Each tick while seated, teleport the
// player onto the seat (heli world transform · seat offset) so they move with it.
// Robust to SCUM's movement component (we override location every frame). ──
pub fn request_enter_heli(seat: usize) -> Result<(), String> {
    install_tick_hook()?;
    HELI_ENTER_PENDING.store(seat.min(4) + 1, Ordering::SeqCst);
    Ok(())
}
pub fn request_exit_heli() -> Result<(), String> {
    HELI_EXIT_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}
unsafe fn ride_seat_tick() {
    if !HELI_SEATED.load(Ordering::SeqCst) { return; }
    let heli = HELI_ACTOR.load(Ordering::SeqCst);
    if heli == 0 { return; }
    let pe_addr = match player::resolve_process_event() { Some(a) => a, None => return };
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let (player, _) = match player::pawn_and_loc() { Some(p) => p, None => return };
    let (gl, fwd_f, rgt_f, up_f, sl) = match (
        find_func("K2_GetActorLocation", "Actor"),
        find_func("GetActorForwardVector", "Actor"),
        find_func("GetActorRightVector", "Actor"),
        find_func("GetActorUpVector", "Actor"),
        find_func("K2_SetActorLocation", "Actor"),
    ) { (Some(a), Some(b), Some(c), Some(d), Some(e)) => (a, b, c, d, e), _ => return };
    #[repr(C)] struct V3 { x: f32, y: f32, z: f32 }
    let mut hl = V3 { x: 0.0, y: 0.0, z: 0.0 }; pe(heli, gl, &mut hl as *mut _ as *mut c_void);
    let mut fw = V3 { x: 0.0, y: 0.0, z: 0.0 }; pe(heli, fwd_f, &mut fw as *mut _ as *mut c_void);
    let mut rt = V3 { x: 0.0, y: 0.0, z: 0.0 }; pe(heli, rgt_f, &mut rt as *mut _ as *mut c_void);
    let mut up = V3 { x: 0.0, y: 0.0, z: 0.0 }; pe(heli, up_f, &mut up as *mut _ as *mut c_void);
    let s = HELI_SEATS[HELI_SEAT.load(Ordering::SeqCst).min(4)];
    #[repr(C)] struct SetLoc { x: f32, y: f32, z: f32, sweep: u8, _p1: [u8; 3], hit: [u8; 0x90], teleport: u8 }
    let mut sp: SetLoc = std::mem::zeroed();
    sp.x = hl.x + fw.x * s[0] + rt.x * s[1] + up.x * s[2];
    sp.y = hl.y + fw.y * s[0] + rt.y * s[1] + up.y * s[2];
    sp.z = hl.z + fw.z * s[0] + rt.z * s[1] + up.z * s[2];
    sp.teleport = 1;
    pe(player, sl, &mut sp as *mut _ as *mut c_void);
}

// Flight: while in the PILOT seat (0), WASD + Space/Ctrl moves the heli actor. The
// rider follows via ride_seat_tick. Sweep off so it flies through collision (no land).
unsafe fn fly_heli_tick(dt: f32) {
    if !HELI_SEATED.load(Ordering::SeqCst) || HELI_SEAT.load(Ordering::SeqCst) != 0 { return; }
    let heli = HELI_ACTOR.load(Ordering::SeqCst);
    if heli == 0 { return; }
    let pe_addr = match player::resolve_process_event() { Some(a) => a, None => return };
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let (gl, gr, fwd_f, sl, sr) = match (
        find_func("K2_GetActorLocation", "Actor"),
        find_func("K2_GetActorRotation", "Actor"),
        find_func("GetActorForwardVector", "Actor"),
        find_func("K2_SetActorLocation", "Actor"),
        find_func("K2_SetActorRotation", "Actor"),
    ) { (Some(a), Some(b), Some(c), Some(d), Some(e)) => (a, b, c, d, e), _ => return };
    #[repr(C)] struct V3 { x: f32, y: f32, z: f32 }
    #[repr(C)] struct Rot { pitch: f32, yaw: f32, roll: f32 }
    let mut loc = V3 { x: 0.0, y: 0.0, z: 0.0 }; pe(heli, gl, &mut loc as *mut _ as *mut c_void);
    let mut rot = Rot { pitch: 0.0, yaw: 0.0, roll: 0.0 }; pe(heli, gr, &mut rot as *mut _ as *mut c_void);
    let mut fwd = V3 { x: 0.0, y: 0.0, z: 0.0 }; pe(heli, fwd_f, &mut fwd as *mut _ as *mut c_void);
    let dt = dt.clamp(0.0, 0.1);
    let spd = 2800.0 * dt;     // cm/s
    let yaw_spd = 75.0 * dt;   // deg/s
    let mut moved = false;
    if key_down(0x57) { loc.x += fwd.x * spd; loc.y += fwd.y * spd; loc.z += fwd.z * spd; moved = true; } // W
    if key_down(0x53) { loc.x -= fwd.x * spd; loc.y -= fwd.y * spd; loc.z -= fwd.z * spd; moved = true; } // S
    if key_down(0x20) { loc.z += spd; moved = true; }                              // Space = up
    if key_down(0xA2) || key_down(0x11) { loc.z -= spd; moved = true; }            // Ctrl = down
    if key_down(0x41) { rot.yaw -= yaw_spd; moved = true; }                         // A = yaw left
    if key_down(0x44) { rot.yaw += yaw_spd; moved = true; }                         // D = yaw right
    if !moved { return; }
    #[repr(C)] struct SetLoc { x: f32, y: f32, z: f32, sweep: u8, _p: [u8; 3], hit: [u8; 0x90], teleport: u8 }
    let mut sp: SetLoc = std::mem::zeroed(); sp.x = loc.x; sp.y = loc.y; sp.z = loc.z; sp.teleport = 1;
    pe(heli, sl, &mut sp as *mut _ as *mut c_void);
    #[repr(C)] struct SetRot { pitch: f32, yaw: f32, roll: f32, teleport: u8, _p: [u8; 3] }
    let mut srp = SetRot { pitch: rot.pitch, yaw: rot.yaw, roll: rot.roll, teleport: 1, _p: [0; 3] };
    pe(heli, sr, &mut srp as *mut _ as *mut c_void);
}

/// GAME-THREAD ONLY. Spawn the cooked BP_TurdMech (clean ACharacter, NO AI), give
/// it a Sentry's mesh+anim, and possess + WASD-drive it. No AI = no crash.
unsafe fn spawn_mech() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let (player, loc) = player::pawn_and_loc().ok_or("player not in world")?;
    let pc = player::resolve_pc().ok_or("no PlayerController")?;

    // 1) load + find the cooked mech class (from pakchunk0_s22)
    if player::find_object("BP_TurdMech_C", None).is_none() {
        load_package("/Game/TurdMOD/BP_TurdMech").ok_or("LoadPackage BP_TurdMech failed (s22 mounted?)")?;
    }
    let mech_class = player::find_object("BP_TurdMech_C", None).ok_or("BP_TurdMech_C not found after load")?;
    let gs = player::find_object("Default__GameplayStatics", None).ok_or("GameplayStatics CDO not found")?;
    let begin = find_func("BeginDeferredActorSpawnFromClass", "GameplayStatics")
        .ok_or("BeginDeferredActorSpawnFromClass not found")?;
    let finish = find_func("FinishSpawningActor", "GameplayStatics").ok_or("FinishSpawningActor not found")?;

    // spawn slightly above the player so it doesn't spawn inside the ground
    let spot = [loc[0], loc[1], loc[2] + 120.0];

    // 2) BeginDeferredActorSpawnFromClass(WorldCtx, Class, FTransform, Collision, Owner) -> Actor
    let mut b = [0u8; 0x60];
    put_u64(&mut b, 0x00, player as u64);
    put_u64(&mut b, 0x08, mech_class as u64);
    put_identity_xform(&mut b, 0x10, spot);
    b[0x40] = 1; // ESpawnActorCollisionHandlingMethod::AlwaysSpawn
    pe(gs, begin, b.as_mut_ptr() as *mut c_void);
    let actor = get_u64(&b, 0x50) as usize;
    if actor == 0 {
        return Err("BeginDeferredActorSpawnFromClass returned null".into());
    }

    // 3) FinishSpawningActor(Actor, FTransform) -> Actor
    let mut f = [0u8; 0x50];
    put_u64(&mut f, 0x00, actor as u64);
    put_identity_xform(&mut f, 0x10, spot);
    pe(gs, finish, f.as_mut_ptr() as *mut c_void);
    let mech = { let r = get_u64(&f, 0x40) as usize; if r != 0 { r } else { actor } };
    logging::log(&format!("[mech] SPAWNED BP_TurdMech @ {mech:#x}"));

    // 4) DRESSING FULLY DISABLED (isolation) — test a bare clean pawn. If this still
    // crashes on move, it's SCUM's PlayerController coupling, not our mesh/anim assets.
    logging::log("[mech] dressing OFF (isolation) — bare BP_TurdMech");

    // 5) possess + WASD-drive it (clean pawn → no AI → no crash)
    if ORIGINAL_PAWN.load(Ordering::SeqCst) == 0 {
        ORIGINAL_PAWN.store(player, Ordering::SeqCst);
    }
    let possess = player::find_object("Possess", Some("Controller")).ok_or("Possess not found")?;
    #[repr(C)]
    struct PossessParams { pawn: usize }
    let mut p = PossessParams { pawn: mech };
    pe(pc, possess, &mut p as *mut _ as *mut c_void);
    MECH_PAWN.store(mech, Ordering::SeqCst);
    logging::log("[mech] possessed BP_TurdMech — WASD to walk your mech 🦾");
    Ok(())
}

pub fn request_paint() -> Result<(), String> {
    install_tick_hook()?;
    PAINT_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn request_probe_meshes() -> Result<(), String> {
    install_tick_hook()?;
    PROBE_MESHES_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

/// GAME-THREAD, READ-ONLY. Log every Laika mesh's owner class, static-mesh asset
/// name, AND its per-slot material names. Wheels are single meshes (rim+tire in
/// one), and there's no glass mesh — so the rim and the window are MATERIAL SLOTS.
/// This dump reveals which slot index is the rim / glass so paint can hit only it.
unsafe fn probe_vehicle_meshes() -> Result<(), String> {
    let meshes = player::find_mesh_comps_by_owner_class("Laika");
    if meshes.is_empty() {
        return Err("no Laika mesh components (get near your Laika)".into());
    }
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let num_fn = find_func("GetNumMaterials", "MeshComponent");
    let get_fn = find_func("GetMaterial", "MeshComponent");
    let loc_fn = find_func("K2_GetComponentLocation", "SceneComponent");
    let rot_fn = find_func("K2_GetComponentRotation", "SceneComponent");
    #[repr(C)] struct NumP { ret: i32 }
    #[repr(C)] struct GetP { idx: i32, _pad: i32, ret: usize }
    #[repr(C)] struct V3 { x: f32, y: f32, z: f32 }

    // vehicle actor world transform — lets us compute each part's RELATIVE placement
    // (the assembly offsets the raw mesh export is missing). Post-process the log.
    #[repr(C)] struct PtrP { ret: usize }
    if let Some(own_fn) = find_func("GetOwner", "ActorComponent") {
        let mut op = PtrP { ret: 0 };
        pe(meshes[0].0, own_fn, &mut op as *mut _ as *mut c_void);
        if op.ret != 0 {
            if let (Some(al), Some(ar)) = (find_func("K2_GetActorLocation", "Actor"), find_func("K2_GetActorRotation", "Actor")) {
                let mut l = V3 { x: 0., y: 0., z: 0. }; pe(op.ret, al, &mut l as *mut _ as *mut c_void);
                let mut r = V3 { x: 0., y: 0., z: 0. }; pe(op.ret, ar, &mut r as *mut _ as *mut c_void);
                logging::log(&format!("[probe] VEHICLE loc={:.2},{:.2},{:.2} rot={:.3},{:.3},{:.3}", l.x, l.y, l.z, r.x, r.y, r.z));
            }
        }
    }

    for (comp, owner_cls, _comp_cls) in &meshes {
        let sm = player::find_member_obj(*comp, "StaticMesh", 0x100, 0x600)
            .and_then(|(p, _)| player::object_name(p))
            .unwrap_or_else(|| "-".into());
        // component world transform (loc + rotation)
        if let (Some(lf), Some(rf)) = (loc_fn, rot_fn) {
            let mut cl = V3 { x: 0., y: 0., z: 0. }; pe(*comp, lf, &mut cl as *mut _ as *mut c_void);
            let mut cr = V3 { x: 0., y: 0., z: 0. }; pe(*comp, rf, &mut cr as *mut _ as *mut c_void);
            logging::log(&format!("[probe] comptf sm={sm} loc={:.2},{:.2},{:.2} rot={:.3},{:.3},{:.3}", cl.x, cl.y, cl.z, cr.x, cr.y, cr.z));
        }
        // per-slot material names
        let mut slots = String::new();
        if let (Some(nf), Some(gf)) = (num_fn, get_fn) {
            let mut np = NumP { ret: 0 };
            pe(*comp, nf, &mut np as *mut _ as *mut c_void);
            let n = np.ret.clamp(0, 12);
            for i in 0..n {
                let mut gp = GetP { idx: i, _pad: 0, ret: 0 };
                pe(*comp, gf, &mut gp as *mut _ as *mut c_void);
                let mn = if gp.ret != 0 { player::object_name(gp.ret).unwrap_or_default() } else { "null".into() };
                slots.push_str(&format!("[{i}]{mn} "));
            }
        }
        logging::log(&format!("[probe] owner={owner_cls} sm={sm} :: {slots}"));
    }
    logging::log(&format!("[probe] {} Laika mesh comps dumped", meshes.len()));

    // Light dump: every LightComponent with owner/own name + world loc + class, so we
    // can see which one is the headlight beam (its owner isn't literally "Headlight").
    let lights = player::find_comps_by_class_named("LightComponent");
    let mut nl = 0;
    for (lc, owner, own) in &lights {
        let cls = player::class_name(*lc).unwrap_or_default();
        let mut loc = String::new();
        if let Some(lf) = loc_fn {
            let mut cl = V3 { x: 0., y: 0., z: 0. }; pe(*lc, lf, &mut cl as *mut _ as *mut c_void);
            loc = format!("loc={:.0},{:.0},{:.0}", cl.x, cl.y, cl.z);
        }
        logging::log(&format!("[probe] light cls={cls} owner={owner} own={own} {loc}"));
        nl += 1;
        if nl >= 60 { break; }
    }
    logging::log(&format!("[probe] {} light comps dumped (of {})", nl, lights.len()));
    Ok(())
}

/// Beam color the studio chose, written to %LOCALAPPDATA%/TurdMOD/beam.json by the
/// Apply helper as {"r":..,"g":..,"b":..} (linear 0..1). Default = PR sky-blue #6CACE4.
fn read_beam_color() -> (f32, f32, f32) {
    let def = (0.4235, 0.6745, 0.8941);
    let local = match std::env::var("LOCALAPPDATA") { Ok(v) => v, Err(_) => return def };
    let path = format!("{local}\\TurdMOD\\beam.json");
    let s = match std::fs::read_to_string(&path) { Ok(s) => s, Err(_) => return def };
    match serde_json::from_str::<serde_json::Value>(&s) {
        Ok(v) => {
            let g = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|f| f as f32);
            match (g("r"), g("g"), g("b")) { (Some(r), Some(gg), Some(b)) => (r, gg, b), _ => def }
        }
        Err(_) => def,
    }
}

/// GAME-THREAD ONLY. PRECISE livery: classify every material SLOT by its parent
/// material name and paint only the targeted ones —
///   • Bumper part            -> "YOUR_OWNER_NAME" name (whole bumper, 1 slot)
///   • Wheel slot, parent Metal -> PR flag (the RIM, not the rubber tire)
///   • any slot, parent Glass   -> translucent window tint
/// Everything else (body paint, interior, tires, lights, engine) is LEFT ALONE so
/// the in-game spray paint still owns the body. @dep player::material_parent_name.
unsafe fn paint_vehicle() -> Result<(), String> {
    let _ = PAINT_IDX.fetch_add(1, Ordering::SeqCst);
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    let pe: extern "system" fn(usize, usize, *mut c_void) = std::mem::transmute(pe_addr);
    let meshes = player::find_mesh_comps_by_owner_class("Laika");
    if meshes.is_empty() {
        return Err("no Laika mesh components found (spawn/get near a Laika)".into());
    }

    // load our cooked materials from pakchunk0_s23
    for pkg in ["M_PRFlag", "M_PRFlagBody", "M_LaikaWheelSkin", "M_LaikaBodySkin"] {
        if player::find_object(pkg, None).is_none() {
            load_package(&format!("/Game/TurdMOD/{pkg}"));
        }
    }
    let flag = player::find_object("M_PRFlag", None).ok_or("M_PRFlag not found (s23 mounted?)")?;
    // Body: prefer the liverylab body skin (per-part paint in the body UV space); else
    // the PR-flag body atlas. M_LaikaBodySkin/MI_Laika_Outside_A share the body UV.
    let body_mat = player::find_object("M_LaikaBodySkin", None)
        .or_else(|| player::find_object("M_PRFlagBody", None))
        .unwrap_or(flag);
    // M_LaikaWheelSkin = liverylab wheel skin (rim/tire painted in the wheel's own UV
    // space). If present, the wheel takes the skin; else fall back to the whole-wheel flag.
    let wheel_mat = player::find_object("M_LaikaWheelSkin", None).unwrap_or(flag);
    let (name, tint) = (flag, 0usize);
    let set_material = find_func("SetMaterial", "MeshComponent").ok_or("SetMaterial not found")?;
    let num_fn = find_func("GetNumMaterials", "MeshComponent");
    let get_fn = find_func("GetMaterial", "MeshComponent");

    #[repr(C)] struct SetMatP { idx: i32, _pad: i32, mat: usize }
    #[repr(C)] struct NumP { ret: i32 }
    #[repr(C)] struct GetP { idx: i32, _pad: i32, ret: usize }
    let (mut rims, mut names, mut tints, mut body) = (0, 0, 0, 0);

    for (comp, owner_cls, _) in &meshes {
        // how many slots on THIS visible mesh
        let n = if let Some(nf) = num_fn {
            let mut np = NumP { ret: 0 };
            pe(*comp, nf, &mut np as *mut _ as *mut c_void);
            np.ret.clamp(0, 12)
        } else { 1 };

        for slot in 0..n {
            // read the slot's current material: its OWN name (descriptive on proxy
            // meshes) and its PARENT name (real type behind a MID on visible meshes).
            let (own, parent) = if let Some(gf) = get_fn {
                let mut gp = GetP { idx: slot, _pad: 0, ret: 0 };
                pe(*comp, gf, &mut gp as *mut _ as *mut c_void);
                if gp.ret != 0 {
                    (player::object_name(gp.ret).unwrap_or_default(),
                     player::material_parent_name(gp.ret).unwrap_or_default())
                } else { (String::new(), String::new()) }
            } else { (String::new(), String::new()) };
            let kind = format!("{own}/{parent}");

            // Match on `kind` (own OR parent) so the VISIBLE meshes — whose own name
            // is just "MaterialInstanceDynamic" with the real type in the parent —
            // are caught, not only the invisible proxy/item meshes.
            // Joel's spec (2026-06-14): flag on the SAME surfaces the in-game sprayer
            // paints = the body shell (MI_Laika_Outside_A) + keep whole-wheel flag.
            // Bumper-name + window-tint stay off (no isolated surface / crashes).
            let _ = (tint, name, &mut names);
            let target = if owner_cls.contains("Wheel") && kind.contains("Wheels") {
                // visible wheel = ONE material (rim+tire) sampling T_Laika_Wheels_D. The
                // wheel skin paints rim & tire in their separate UV regions of that atlas,
                // so one material shows both — no rim-only material swap needed.
                rims += 1; wheel_mat
            } else if kind.contains("Outside") {
                // MI_Laika_Outside_A = the spray-paintable body shell → the BAKED
                // body livery (coherent flag projected into the UV atlas).
                body += 1; body_mat
            } else {
                // diagnostic: log skips so we see real slot kinds we didn't match
                logging::log(&format!("[paint] skip {owner_cls} slot{slot} kind={kind}"));
                continue;
            };
            logging::log(&format!("[paint] {owner_cls} slot{slot} kind={kind} -> set"));
            let mut p = SetMatP { idx: slot, _pad: 0, mat: target };
            pe(*comp, set_material, &mut p as *mut _ as *mut c_void);
        }
    }
    // Vehicle lights → beam color (studio-chosen via beam.json, default sky-blue).
    // The headlight's OWNER isn't literally "Headlight" (the old narrow match found 0),
    // so scan ALL LightComponents and match the vehicle's (owner~Laika or name~Head/Light).
    let (br, bg, bb) = read_beam_color();
    let mut lights = 0;
    if let Some(set_color) = find_func("SetLightColor", "LightComponent") {
        #[repr(C)] struct ColP { r: f32, g: f32, b: f32, a: f32, srgb: u8, _pad: [u8; 3] }
        for (lc, owner, own) in player::find_comps_by_class_named("LightComponent") {
            // ONLY the Laika's headlight beam: owner is the live vehicle (BPC_Laika_C)
            // AND the light is a headlight. Probe (2026-06-15) showed world lights
            // (street lamps/chapels/drones) + CDO templates share generic names — the
            // old broad `own~Light` match lit the whole neighborhood (88 lights).
            let is_vehicle_light = owner.contains("Laika") && own.contains("Headlight");
            if !is_vehicle_light { continue; }
            let mut cp = ColP { r: br, g: bg, b: bb, a: 1.0, srgb: 1, _pad: [0; 3] };
            pe(lc, set_color, &mut cp as *mut _ as *mut c_void);
            logging::log(&format!("[paint] light {owner}/{own} -> beam ({br:.2},{bg:.2},{bb:.2})"));
            lights += 1;
        }
    }

    let _ = (names, tints);
    logging::log(&format!(
        "[paint] DONE: flag on {body} body (sprayer) slots + {rims} wheel slots; {lights} headlights blued"
    ));
    Ok(())
}

/// Install the game-thread tick hook (idempotent). Safe to call once the image
/// is loaded; the detour just trampolines UGameEngine::Tick.
pub fn install_tick_hook() -> Result<(), String> {
    if TICK.get().is_some() {
        return Ok(());
    }
    let addr = player::img_base() + RVA_UGAMEENGINE_TICK;
    let target: TickFn = unsafe { std::mem::transmute(addr) };
    let detour =
        unsafe { GenericDetour::new(target, tick_hook) }.map_err(|e| format!("detour new: {e}"))?;
    unsafe { detour.enable() }.map_err(|e| format!("detour enable: {e}"))?;
    let _ = TICK.set(detour);
    logging::log(&format!("[native_ui] UGameEngine::Tick hooked @ {addr:#x} (game-thread marshal ready)"));
    Ok(())
}

/// Request the welcome panel: ensure the hook is up, then flag it. The actual
/// creation runs on the next game-thread tick.
pub fn request_show() -> Result<(), String> {
    install_tick_hook()?;
    SHOW_PENDING.store(true, Ordering::SeqCst);
    Ok(())
}

// UObject* LoadPackage(void* InOuter, const wchar_t* PackageName, uint32 LoadFlags,
// void* InReaderOverride). 41-byte unique prologue AOB ported from the server bridge
// (same UE4.27 engine). Self-healing: AOB-scanned at runtime, survives build shifts.
const LOADPACKAGE_AOB: &str = "48 8B C4 48 89 58 10 48 89 68 18 48 89 70 20 48 89 48 08 57 41 54 41 55 41 56 41 57 48 83 EC 40 45 33 F6 48 8D 48 C8 48 8B FA";
type LoadPackageFn =
    unsafe extern "system" fn(*mut c_void, *const u16, u32, *mut c_void) -> *mut c_void;
static LOADPKG: OnceCell<usize> = OnceCell::new();

unsafe fn resolve_load_package() -> Option<usize> {
    if let Some(a) = LOADPKG.get() {
        return Some(*a);
    }
    let span = crate::sigscan::main_module_span()?;
    let pat = crate::sigscan::Pattern::parse("LoadPackage", LOADPACKAGE_AOB, None).ok()?;
    let addr = crate::sigscan::find(span, &pat)? as usize;
    let _ = LOADPKG.set(addr);
    logging::log(&format!("[native_ui] LoadPackage resolved @ {addr:#x}"));
    Some(addr)
}

/// Force-load a /Game/... package so its UClass exists in memory. Mounting a pak
/// only makes files available; nothing has a UObject until the package is loaded.
unsafe fn load_package(pkg: &str) -> Option<usize> {
    let addr = resolve_load_package()?;
    let f: LoadPackageFn = std::mem::transmute(addr);
    let wide: Vec<u16> = pkg.encode_utf16().chain(std::iter::once(0)).collect();
    let r = f(std::ptr::null_mut(), wide.as_ptr(), 0, std::ptr::null_mut());
    if r.is_null() {
        None
    } else {
        Some(r as usize)
    }
}

/// GAME-THREAD ONLY. Create WBP_TurdMODWelcome and add it to the viewport via
/// the engine's own UFunctions. @inv called only from tick_hook.
unsafe fn show_welcome() -> Result<(), String> {
    let pe_addr = player::resolve_process_event().ok_or("ProcessEvent unresolved")?;
    // UObject::ProcessEvent(this, UFunction*, void* Params) — MS x64 ABI.
    let process_event: extern "system" fn(usize, usize, *mut c_void) =
        std::mem::transmute(pe_addr);

    let pc = player::resolve_pc().ok_or("PlayerController null (in world?)")?;

    // Mounting != loading: force-load the package so its generated class exists.
    if player::find_object("WBP_TurdMODWelcome_C", None).is_none() {
        match load_package("/Game/TurdMOD/WBP_TurdMODWelcome") {
            Some(p) => logging::log(&format!("[native_ui] LoadPackage /Game/TurdMOD/WBP_TurdMODWelcome -> {p:#x}")),
            None => return Err("LoadPackage returned null (pak mounted? AOB matched?)".into()),
        }
    }
    let wbp_class = player::find_object("WBP_TurdMODWelcome_C", None)
        .ok_or("WBP_TurdMODWelcome_C not found after LoadPackage")?;
    let cdo = player::find_object("Default__WidgetBlueprintLibrary", None)
        .ok_or("WidgetBlueprintLibrary CDO not found")?;
    let create_fn = player::find_object("Create", Some("WidgetBlueprintLibrary"))
        .ok_or("WidgetBlueprintLibrary::Create not found")?;

    // UWidgetBlueprintLibrary::Create(WorldContextObject, WidgetType, OwningPlayer) -> UserWidget.
    // Params are the function's properties in order, then the return value.
    #[repr(C)]
    struct CreateParams {
        world_ctx: usize,
        widget_type: usize,
        owning_player: usize,
        ret: usize,
    }
    let mut cp = CreateParams {
        world_ctx: pc, // PlayerController is a valid WorldContextObject
        widget_type: wbp_class,
        owning_player: pc,
        ret: 0,
    };
    process_event(cdo, create_fn, &mut cp as *mut _ as *mut c_void);
    let widget = cp.ret;
    if widget == 0 {
        return Err("Create returned null widget".into());
    }
    logging::log(&format!("[native_ui] CreateWidget -> {widget:#x}"));

    // UUserWidget::AddToViewport(int32 ZOrder).
    let add_fn = player::find_object("AddToViewport", Some("UserWidget"))
        .ok_or("UserWidget::AddToViewport not found")?;
    #[repr(C)]
    struct AtvParams {
        z_order: i32,
    }
    let mut ap = AtvParams { z_order: 0 };
    process_event(widget, add_fn, &mut ap as *mut _ as *mut c_void);
    Ok(())
}
