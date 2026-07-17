//! UE4 hook layer (#169) — drawing in-game UI for TurdMOD mods.
//!
//! Architecture (post-research, 2026-05-08):
//!
//! We adopt [`hudhook`](https://github.com/veeenu/hudhook) 0.9.0 — a single
//! MIT-licensed crate that bundles: DXGI Present-hook, ImGui rendering
//! via `imgui-dx11`/`-dx12`, WndProc input forwarding, vtable extraction
//! (the temp swap chain dance), and the MinHook engine. One dep replaces
//! ~5 hand-rolled pieces.
//!
//! What this module owns:
//!
//!   1. **Engine sigscan.** UE4 4.27 globals (`GEngine`, `GWorld`,
//!      `GameViewport`, `FUObjectArray`, `FNamePool`) — found at runtime
//!      via byte-pattern scan of the SCUM module. Patterns documented in
//!      [docs/scum-internals/16-memory-signatures.md]. (Stub today.)
//!
//!   2. **Render loop.** `TurdmodRenderLoop` implements
//!      `ImguiRenderLoop`. Each frame it drains a thread-safe queue of
//!      pending panel payloads (welcome, kill ticker, admin overlay)
//!      and draws each via ImGui. The Lua runtime fills the queue from
//!      `turdmod.notify_panel`.
//!
//!   3. **Input gating.** Hudhook handles WndProc forwarding; we control
//!      whether messages are blocked via `message_filter`.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use hudhook::hooks::dx11::ImguiDx11Hooks;
use hudhook::imgui::{Condition, Context, FontConfig, FontGlyphRanges, FontId, FontSource, ImColor32, StyleColor, StyleVar, Ui};
use hudhook::Hudhook;
use hudhook::ImguiRenderLoop;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value as JsonValue;

use crate::logging;

static HOOKS_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Frame counter incremented on every hudhook render call. Used to gate
/// panel display so windows don't pop up during SCUM's splash / loading
/// screens — Joel reported the welcome panel was appearing before the
/// main menu had drawn, blocking what was behind it. We hold panels
/// until at least PANEL_FRAME_GATE frames have rendered.
static FRAMES_RENDERED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Frame gate covering SCUM's pre-menu sequence: splash → EAC → "Press
/// any key" → menu draw. With the intro videos disabled (the
/// `.disabled-by-turdmod` rename trick documented in the loader's
/// README), SCUM reaches the menu in roughly 10-15 s instead of 30+,
/// so 600 frames here matches that pace (~10s @ 60fps, ~20s @ 30fps).
/// Combined with the wall-clock gate so a frame-rate stall during
/// loading doesn't trick the gate into firing early.
const PANEL_FRAME_GATE: u64 = 600;

/// Wall-clock companion to the frame gate. Lowered from 35s to 12s
/// after intro videos were disabled. If a future build re-enables the
/// videos this needs to bump back up.
///
/// Future work tracked in IDEAS.md: replace the fixed wall-clock gate
/// with an OCR-driven detector (Windows.Media.Ocr polls a SCUM
/// screenshot for "CONTINUE" / "MULTI PLAY" / "SANDBOX" buttons and
/// signals via a flag file the moment the menu is up).
const PANEL_TIME_GATE_SECS: u64 = 12;

/// First-render-call timestamp. Started once when the render loop fires
/// for the first time, then read every frame to compute elapsed.
static ATTACH_INSTANT: Lazy<parking_lot::Mutex<Option<std::time::Instant>>> =
    Lazy::new(|| parking_lot::Mutex::new(None));

/// Pending panels waiting for the render loop to pick them up. Pushed
/// from the Lua thread via `enqueue_panel`; drained on the render thread.
static PENDING: Lazy<Mutex<VecDeque<JsonValue>>> = Lazy::new(|| Mutex::new(VecDeque::new()));

/// Panels currently being drawn. Each frame the render loop appends any
/// new pending panels and retains the ones still open.
static ACTIVE: Lazy<Mutex<Vec<ActivePanel>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone)]
struct ActivePanel {
    payload: JsonValue,
    /// Becomes false when the user dismisses the panel; render loop
    /// removes it on the next frame.
    open: bool,
    /// Wall-clock time the panel was first drawn (Some) or queued
    /// (None until first render). Used by `auto_close_after_secs` to
    /// auto-dismiss timed panels like the loading splash.
    first_drawn_at: Option<std::time::Instant>,
}

/// Top-level entry — called from `init_thread` once the runtime is up.
pub fn install() -> Result<(), String> {
    if HOOKS_INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    logging::log("[hooks] resolving UE4 engine pointers...");
    match resolve_engine() {
        Ok(e) => logging::log(&format!(
            "[hooks] GEngine={:#x} GWorld={:#x} GameViewport={:#x}",
            e.gengine, e.gworld, e.game_viewport
        )),
        Err(e) => logging::log(&format!(
            "[hooks] engine resolve incomplete (sigscan stub): {e}"
        )),
    }

    if let Err(e) = install_render_pipeline() {
        logging::log(&format!("[hooks] render install failed: {e}"));
        return Err(e);
    }
    logging::log("[hooks] render pipeline installed via hudhook");
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct EnginePointers {
    pub gengine: usize,
    pub gworld: usize,
    pub game_viewport: usize,
    pub uobject_array: usize,
    pub fname_pool: usize,
}

fn resolve_engine() -> Result<EnginePointers, String> {
    use crate::sigscan::{find, main_module_span, Pattern};

    crate::sigscan::log_module_info();

    let span = main_module_span().ok_or("could not query main module bounds")?;

    // Pattern table — kept in sync with docs/scum-internals/16-memory-signatures.md.
    // The trailing tuple is `rip_offset` (where the 4-byte RIP-relative
    // displacement starts inside the matched bytes). For UE4 4.27 the
    // canonical patterns put the LEA/MOV opcode bytes first, the
    // 32-bit displacement starting at offset 3.
    let gengine_p = Pattern::parse(
        "GEngine",
        "48 8B 05 ?? ?? ?? ?? 48 85 C0 74 ?? 48 8B 80",
        Some(3),
    ).map_err(|e| e)?;
    let gworld_p = Pattern::parse(
        "GWorld",
        "48 89 05 ?? ?? ?? ?? E8 ?? ?? ?? ?? ?? E8",
        Some(3),
    ).map_err(|e| e)?;
    let uobject_array_p = Pattern::parse(
        "FUObjectArray",
        "48 8B 05 ?? ?? ?? ?? 48 8B 0C C8 4C 8B 04 D1",
        Some(3),
    ).map_err(|e| e)?;
    let fname_pool_p = Pattern::parse(
        "FNamePool",
        "74 09 48 8D 15 ?? ?? ?? ?? EB 16 48 8D 15 ?? ?? ?? ??",
        Some(5),
    ).map_err(|e| e)?;

    let gengine = find(span, &gengine_p).ok_or("GEngine pattern not matched")?;
    let gworld = find(span, &gworld_p).ok_or("GWorld pattern not matched")?;
    let uobject_array = find(span, &uobject_array_p).ok_or("FUObjectArray pattern not matched")?;
    let fname_pool = find(span, &fname_pool_p).ok_or("FNamePool pattern not matched")?;

    // GameViewport is a member of UGameEngine at +0x778 in UE4 4.27
    // cooked layout. Compute by reading GEngine's pointer-to-engine,
    // then offsetting. Unsafe but bounded — if our GEngine resolve is
    // wrong the read fails harmlessly into a likely garbage pointer
    // which we'll spot in the diagnostic log.
    let game_viewport = unsafe {
        let engine_ptr = (gengine as *const usize).read_unaligned();
        if engine_ptr == 0 {
            0
        } else {
            (engine_ptr + 0x778) as usize
        }
    };

    Ok(EnginePointers {
        gengine: gengine as usize,
        gworld: gworld as usize,
        game_viewport,
        uobject_array: uobject_array as usize,
        fname_pool: fname_pool as usize,
    })
}

/// Install hudhook's DX11 swap-chain hook with our render loop. Hudhook
/// handles the temp swap chain → vtable[8] → MinHook trampoline →
/// per-frame callback → WndProc forwarding internally.
fn install_render_pipeline() -> Result<(), String> {
    let hook = Hudhook::builder()
        .with::<ImguiDx11Hooks>(TurdmodRenderLoop::new())
        .build();
    hook.apply().map_err(|e| format!("hudhook apply: {e:?}"))?;
    Ok(())
}

/// Push a panel from the Lua runtime into the render queue.
pub fn enqueue_panel(payload: JsonValue) {
    PENDING.lock().push_back(payload);
}

/// Enqueue the TurdMOD loading-splash panel. Tiny, branded, no
/// dismiss button -- auto-closes after 3 seconds via the panel
/// schema's `auto_close_after_secs` field. Fired immediately on
/// attach so the player sees a TurdMOD branding moment the first
/// time the loader's render hook draws past its frame gate.
pub fn enqueue_loading_splash() {
    let payload = serde_json::json!({
        "kind": "loading_splash",
        "server_name": "TurdMOD",
        "tagline": "Loading modding engine...",
        "auto_close_after_secs": 3.0,
        "actions": [],
        "sections": [],
    });
    enqueue_panel(payload);
    logging::log("[hooks] loading splash enqueued (auto-close 3s)");
}

/// Build the default TurdMOD welcome panel and enqueue it. Called from
/// `init_thread` after the loading splash so the player sees the
/// branded loading moment first, then the full welcome panel.
///
/// The panel is content-driven via the same JSON schema Lua mods use,
/// so swapping it out for a server-driven version later is a one-line
/// change in `init_thread`.
pub fn enqueue_default_welcome_panel(loaded_mods: &[String], loader_version: &str) {
    let mod_brag = if loaded_mods.is_empty() {
        // Empty mod list still gets a visual confirmation so the player
        // knows the loader is healthy — just a "no mods loaded" hint
        // instead of an empty section.
        serde_json::json!({
            "kind": "rules",
            "title": "Loaded mods",
            "items": [
                "(none yet — drop a mod folder under %LOCALAPPDATA%/TurdMOD/mods/)",
            ],
        })
    } else {
        serde_json::json!({
            "kind": "mod_brag",
            "title": "Loaded mods",
            "mods": loaded_mods.iter().map(|name| serde_json::json!({
                // ▶ (U+25B6) is in Geometric Shapes (in our glyph
                // ranges). Avoid emoji like 🧩 — those are in the
                // Supplementary Multilingual Plane (U+1F000+) and
                // would render as a missing-glyph placeholder.
                "emoji": "▶",
                "name": name,
                "tagline": "active",
            })).collect::<Vec<_>>(),
        })
    };

    let payload = serde_json::json!({
        "server_name": "TurdMOD",
        "tagline": format!(
            "TurdMOD loader v{loader_version} is running. \
             Modding engine for SCUM — solo + private/modded servers."
        ),
        "greeting": "Press the X (top-right) or click Dismiss to close. \
                     Server-side mods register their own panels through \
                     the companion runtime; this is the loader's own \
                     welcome screen.",
        "sections": [
            mod_brag,
            {
                "kind": "rules",
                "title": "What's active",
                "items": [
                    "DXGI proxy + DLL injection — green (you wouldn't see this otherwise)",
                    "Lua sandbox — green",
                    "ImGui render hook (DX11) — green (this very panel)",
                    "Sibling decorator DLL — adds <img src/>, <a href/>, <dismiss key/> \
                     markup support to engine rich-text widgets",
                ],
            },
            {
                "kind": "rules",
                "title": "Get involved",
                "items": [
                    "github.com/roketteere/turdmod",
                    "Mods drop in %LOCALAPPDATA%/TurdMOD/mods/<id>/main.lua",
                    "Discord: ask the dev team for the invite link",
                ],
            },
        ],
        "actions": [
            { "kind": "dismiss", "label": "Dismiss" },
            { "kind": "link", "label": "github.com/roketteere/turdmod" },
        ],
    });

    enqueue_panel(payload);
    logging::log("[hooks] welcome panel enqueued");
}

pub struct TurdmodRenderLoop {
    /// Body and heading font addresses (raw FontId pointer round-tripped
    /// through usize). Both fonts are loaded from a single TTF file in
    /// `initialize`. Zero means font loading failed -- panel falls back
    /// to ImGui's default ProggyClean for everything.
    body_font_addr: usize,
    heading_font_addr: usize,
}

impl TurdmodRenderLoop {
    pub fn new() -> Self {
        Self { body_font_addr: 0, heading_font_addr: 0 }
    }

    fn body_font(&self) -> Option<FontId> {
        if self.body_font_addr == 0 { None } else {
            Some(unsafe { std::mem::transmute::<usize, FontId>(self.body_font_addr) })
        }
    }
    fn heading_font(&self) -> Option<FontId> {
        if self.heading_font_addr == 0 { None } else {
            Some(unsafe { std::mem::transmute::<usize, FontId>(self.heading_font_addr) })
        }
    }
}

/// Load a font from disk that has the box-drawing + block glyphs we want
/// for the title banner. Tries Consolas first (ships on every Windows
/// install since Vista) then falls back to Cascadia Code. Returns the
/// font bytes or None if neither is readable.
fn read_block_capable_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\consola.ttf",
        r"C:\Windows\Fonts\CascadiaCode.ttf",
        r"C:\Windows\Fonts\CascadiaMono.ttf",
        r"C:\Windows\Fonts\lucon.ttf",  // Lucida Console
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            logging::log(&format!("[hooks] heading font loaded from {path} ({} bytes)", bytes.len()));
            return Some(bytes);
        }
    }
    logging::log("[hooks] no block-capable font found in C:\\Windows\\Fonts; will fall back to default font");
    None
}

impl ImguiRenderLoop for TurdmodRenderLoop {
    fn initialize<'a>(
        &'a mut self,
        ctx: &mut Context,
        _render_context: &'a mut dyn hudhook::RenderContext,
    ) {
        // One-shot font registration. We add a SECONDARY font sized
        // larger than the default, with extended glyph ranges (basic
        // Latin + Latin-1 supplement + Box Drawing 0x2500-0x257F + Block
        // Elements 0x2580-0x259F). Anything we draw via push_font(this)
        // gets the bigger weight and the block-character glyphs needed
        // to mirror the log banner's chunky TURDMOD art.
        //
        // If reading the font file from disk fails the panel still
        // works — it just falls back to ProggyClean and the figlet
        // version of the title.
        let Some(font_bytes) = read_block_capable_font() else { return; };
        // Heap-allocate so the slice we hand imgui lives long enough.
        // We `Box::leak` because the font atlas keeps a long-lived
        // reference to the bytes; for a process-lifetime resource this
        // is the standard pattern.
        let leaked: &'static [u8] = Box::leak(font_bytes.into_boxed_slice());
        // Glyph range list -- pairs of (lo, hi), terminated by 0. Covers
        // basic Latin + the symbol ranges we need for the panel:
        //   0x0020-0x00FF Basic Latin + Latin-1 Supplement
        //   0x2500-0x257F Box Drawing  (the chunky banner letters)
        //   0x2580-0x259F Block Elements (full / half blocks)
        //   0x25A0-0x25FF Geometric Shapes (●  ■  ▲  ►  used for bullets)
        //   0x2600-0x26FF Misc Symbols    (☰  ⚠  ★  used by future panels)
        //   0x2700-0x27BF Dingbats        (✓  ✦  ✯)
        let glyph_ranges = FontGlyphRanges::from_slice(&[
            0x0020, 0x00FF,  // Basic Latin + Latin-1 Supplement
            0x2000, 0x206F,  // General Punctuation (em-dash, en-dash, …)
            0x2500, 0x257F,  // Box Drawing
            0x2580, 0x259F,  // Block Elements
            0x25A0, 0x25FF,  // Geometric Shapes (●  ■  ▲  ►)
            0x2600, 0x26FF,  // Misc Symbols (☰  ⚠  ★)
            0x2700, 0x27BF,  // Dingbats (✓  ✦  ✯)
            0,
        ]);
        // Body font -- 24px Consolas. We add this FIRST so it becomes
        // the imgui default; everything that doesn't push a different
        // font picks this up. Replaces the unreadable 13px ProggyClean.
        let body_font_id = ctx.fonts().add_font(&[FontSource::TtfData {
            data: leaked,
            size_pixels: 24.0,
            config: Some(FontConfig {
                glyph_ranges: glyph_ranges.clone(),
                oversample_h: 2,
                oversample_v: 2,
                ..Default::default()
            }),
        }]);
        // Heading font -- 40px Consolas. Used only for the title
        // banner via push_font. 56px overflowed the panel right edge
        // (TURDMOD block art is ~6 chars * ~6 cols * heading_size px
        // wide, which at 56 = ~2016 px exceeded the 60% * 2888 =
        // 1733 px window). 40px keeps the title comfortably inside
        // the window while still feeling like a hero strip.
        let heading_font_id = ctx.fonts().add_font(&[FontSource::TtfData {
            data: leaked,
            size_pixels: 40.0,
            config: Some(FontConfig {
                glyph_ranges,
                oversample_h: 2,
                oversample_v: 2,
                ..Default::default()
            }),
        }]);
        // Round-trip via usize so the stored value is Send.
        self.body_font_addr = unsafe { std::mem::transmute::<FontId, usize>(body_font_id) };
        self.heading_font_addr = unsafe { std::mem::transmute::<FontId, usize>(heading_font_id) };
        logging::log(&format!(
            "[hooks] fonts registered with imgui atlas: body=24px, heading=56px (extended glyph range)"
        ));
    }

    fn render(&mut self, ui: &mut Ui) {
        let frame = FRAMES_RENDERED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // ProggyClean (ImGui's default font) is 13px which is barely
        // legible on a 1940x1398 SCUM viewport. We scale per-window via
        // set_window_font_scale inside the panel build closure (see
        // draw_panel) — Io.font_global_scale isn't reachable via the &mut
        // Ui that hudhook hands us. A proper fix loads a custom TTF at
        // the right pixel size; this is the "iterate later" knob until
        // then.

        // One-time diagnostic dump of imgui's reported display_size so we
        // can sanity-check window-size math against the actual viewport.
        // Logs once on first render call and then never again (`Once`
        // semantics via the AtomicBool below).
        static SIZE_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SIZE_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let d = ui.io().display_size;
            logging::log(&format!(
                "[hooks] hudhook display_size = [{}, {}], font_global_scale=2.0",
                d[0], d[1]
            ));
        }

        // Lazy-init the attach timestamp on first render — we can't do
        // it from `install()` because hudhook may not have a DX context
        // yet at that point.
        let elapsed_secs = {
            let mut slot = ATTACH_INSTANT.lock();
            let now = std::time::Instant::now();
            slot.get_or_insert(now).elapsed().as_secs()
        };

        // Hold panels off-screen during SCUM's splash / loading sequence.
        // We still drain PENDING into ACTIVE so the queue doesn't pile up;
        // we just skip drawing until BOTH gates have passed. Once we cross
        // them, ACTIVE is rendered as normal — including any panels that
        // were enqueued before the gate.
        {
            let mut pending = PENDING.lock();
            let mut active = ACTIVE.lock();
            while let Some(p) = pending.pop_front() {
                active.push(ActivePanel { payload: p, open: true, first_drawn_at: None });
            }
        }
        let gate_open = frame >= PANEL_FRAME_GATE && elapsed_secs >= PANEL_TIME_GATE_SECS;

        // Draw each open panel; mark dismissed ones for removal.
        // Loading splash panels render pre-gate (so they show during
        // SCUM's loading sequence as a branded "Loading..." moment);
        // every other panel waits until both gates open. This way the
        // splash and main welcome never visually overlap — splash plays
        // first, auto-closes, then main appears once SCUM's menu is up.
        let heading = self.heading_font();
        let body = self.body_font();
        let mut active = ACTIVE.lock();
        for panel in active.iter_mut() {
            if !panel.open { continue; }
            let is_splash = panel.payload.get("kind").and_then(JsonValue::as_str)
                == Some("loading_splash");
            if !is_splash && !gate_open { continue; }
            if is_splash {
                draw_loading_splash(ui, panel, heading);
            } else {
                draw_panel(ui, panel, heading, body);
            }
        }
        active.retain(|p| p.open);
    }
}

/// Compact splash variant: ~520x180, centered, branded title +
/// "Loading modding engine" with animated trailing dots. No dismiss
/// button, no body sections. Auto-closes after 3 s via the same
/// `auto_close_after_secs` mechanism used by `draw_panel`.
fn draw_loading_splash(ui: &Ui, panel: &mut ActivePanel, heading_font: Option<FontId>) {
    if panel.first_drawn_at.is_none() {
        panel.first_drawn_at = Some(std::time::Instant::now());
    }
    let p = &panel.payload;
    if let Some(secs) = p.get("auto_close_after_secs").and_then(JsonValue::as_f64) {
        if let Some(start) = panel.first_drawn_at {
            if start.elapsed().as_secs_f64() >= secs {
                panel.open = false;
                return;
            }
        }
    }

    // Push the same TurdMOD palette as the main panel so the splash
    // feels like the same product. Tighter color set since the splash
    // is widget-light.
    let _toks = [
        ui.push_style_color(StyleColor::WindowBg, C_BG_DEEP),
        ui.push_style_color(StyleColor::Border,   C_BORDER),
        ui.push_style_color(StyleColor::TitleBg,  C_TITLE_BG),
        ui.push_style_color(StyleColor::TitleBgActive, C_TITLE_BG_ACT),
        ui.push_style_color(StyleColor::Text,     C_TEXT),
    ];
    let _r = ui.push_style_var(StyleVar::WindowRounding(14.0));
    let _b = ui.push_style_var(StyleVar::WindowBorderSize(2.0));

    let display = ui.io().display_size;
    let win_w = 560.0_f32;
    let win_h = 220.0_f32;
    let center_x = display[0] * 0.5;
    let center_y = display[1] * 0.5;

    let label = format!("TurdMOD##turdmod-splash-{:p}", p);
    ui.window(&label)
        .size([win_w, win_h], Condition::Always)
        .position([center_x, center_y], Condition::Always)
        .position_pivot([0.5, 0.5])
        .no_decoration()              // no title bar / no resize / no scroll
        .focus_on_appearing(false)
        .build(|| {
            // Title in heading font when available.
            if let Some(fid) = heading_font {
                let _f = ui.push_font(fid);
                let title_lines = ["TURDMOD"];
                for line in title_lines.iter() {
                    // Center the title horizontally.
                    let approx_w = line.len() as f32 * 24.0;
                    let cur = ui.cursor_pos();
                    ui.set_cursor_pos([(win_w - approx_w) * 0.5, cur[1]]);
                    let _t = ui.push_style_color(StyleColor::Text, C_MUSTARD_BRIGHT);
                    ui.text(*line);
                }
            } else {
                let _t = ui.push_style_color(StyleColor::Text, C_MUSTARD_BRIGHT);
                let cur = ui.cursor_pos();
                ui.set_cursor_pos([(win_w - 80.0) * 0.5, cur[1]]);
                ui.text("TURDMOD");
            }
            ui.spacing();
            // Animated trailing dots: 0..4 dots cycling at 2 Hz.
            let phase = ((ui.time() * 2.0).rem_euclid(4.0)) as usize;
            let dots: String = ".".repeat(phase);
            let msg = format!("Loading modding engine{dots}");
            let cur = ui.cursor_pos();
            ui.set_cursor_pos([(win_w - msg.len() as f32 * 13.0) * 0.5, cur[1] + 16.0]);
            ui.text(msg);
        });
}

/// Linear interpolation between two RGBA colors. `t` in `[0..1]`.
fn lerp_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Draw a stylized TURD glyph using only circle primitives — primitives-
/// only stand-in for the 💩 emoji that ImGui's default font can't render.
/// Three stacked brown circles for the body (no polyline ovals; previous
/// version of this function used `add_polyline` and crashed SCUM at first
/// render — we suspect imgui-rs's filled-polyline path doesn't survive
/// hudhook's DX11 buffer setup), with smaller circles for the eyes and a
/// curve of points for the smile.
fn draw_turd_glyph(ui: &Ui, center: [f32; 2], scale: f32) {
    let dl = ui.get_window_draw_list();
    let brown_dark   = ImColor32::from_rgba(80, 50, 25, 255);
    let brown_mid    = ImColor32::from_rgba(120, 75, 35, 255);
    let brown_light  = ImColor32::from_rgba(165, 110, 55, 255);
    let highlight    = ImColor32::from_rgba(220, 170, 100, 255);
    let eye_white    = ImColor32::from_rgba(245, 240, 220, 255);
    let eye_dot      = ImColor32::from_rgba(35, 22, 12, 255);
    let mouth        = ImColor32::from_rgba(35, 22, 12, 255);

    let cx = center[0];
    let cy = center[1];
    // Body — three stacked filled circles, decreasing size + slight curl.
    dl.add_circle([cx,           cy + 24.0 * scale], 28.0 * scale, brown_dark).filled(true).num_segments(24).build();
    dl.add_circle([cx,           cy + 22.0 * scale], 25.0 * scale, brown_mid).filled(true).num_segments(24).build();
    dl.add_circle([cx + 4.0 * scale, cy + 4.0  * scale], 20.0 * scale, brown_mid).filled(true).num_segments(24).build();
    dl.add_circle([cx + 4.0 * scale, cy + 2.0  * scale], 17.5 * scale, brown_light).filled(true).num_segments(24).build();
    dl.add_circle([cx - 2.0 * scale, cy - 14.0 * scale], 12.0 * scale, brown_light).filled(true).num_segments(20).build();
    dl.add_circle([cx - 4.0 * scale, cy - 22.0 * scale], 5.0  * scale, highlight).filled(true).num_segments(16).build();

    // Eyes (two white dots with dark pupils, on the middle layer).
    let eye_y = cy + 2.0 * scale;
    dl.add_circle([cx - 8.0 * scale, eye_y], 3.5 * scale, eye_white).filled(true).num_segments(12).build();
    dl.add_circle([cx + 8.0 * scale, eye_y], 3.5 * scale, eye_white).filled(true).num_segments(12).build();
    dl.add_circle([cx - 7.0 * scale, eye_y + 0.5], 1.6 * scale, eye_dot).filled(true).num_segments(8).build();
    dl.add_circle([cx + 9.0 * scale, eye_y + 0.5], 1.6 * scale, eye_dot).filled(true).num_segments(8).build();

    // Smile — three small filled circles forming a downward arc rather
    // than a polyline (polyline path is the suspected crash culprit).
    let mouth_y = cy + 11.0 * scale;
    let r = 1.6 * scale;
    dl.add_circle([cx - 5.0 * scale, mouth_y], r, mouth).filled(true).num_segments(8).build();
    dl.add_circle([cx,               mouth_y + 2.5 * scale], r, mouth).filled(true).num_segments(8).build();
    dl.add_circle([cx + 5.0 * scale, mouth_y], r, mouth).filled(true).num_segments(8).build();
}

// ---------- TurdMOD theme ----------
//
// SCUM's UI is grimy: rust browns, military green, weathered cream
// accents on dark backgrounds. We lean into that and add the TurdMOD
// brand twist — mustard / dung gold accents instead of the default
// ImGui blue-grey. Every color in the palette comes from this table so
// future tuning is one block.
//
// Linear sRGB floats — ImGui multiplies these directly into the pixel.
// Backgrounds run at 0.82 alpha so the SCUM menu is faintly visible
// behind the panel — semi-transparent grunge layered over the game.
const C_BG_DEEP:        [f32; 4] = [0.10, 0.06, 0.03, 0.82]; // #1A0F08 deep brown
const C_BG_MID:         [f32; 4] = [0.18, 0.12, 0.06, 0.82]; // #2E1F0F mid brown
const C_TITLE_BG:       [f32; 4] = [0.07, 0.04, 0.02, 1.0];  // near black
const C_TITLE_BG_ACT:   [f32; 4] = [0.32, 0.20, 0.07, 1.0];  // brown highlight
const C_BORDER:         [f32; 4] = [0.55, 0.35, 0.18, 0.85]; // bronze
const C_TEXT:           [f32; 4] = [0.96, 0.87, 0.70, 1.0];  // cream
const C_TEXT_DIM:       [f32; 4] = [0.72, 0.62, 0.48, 1.0];  // muted cream
const C_MUSTARD:        [f32; 4] = [0.85, 0.63, 0.24, 1.0];  // dung gold
const C_MUSTARD_BRIGHT: [f32; 4] = [1.00, 0.78, 0.30, 1.0];
const C_BUTTON:         [f32; 4] = [0.40, 0.24, 0.08, 1.0];  // dark mustard
const C_BUTTON_HOVER:   [f32; 4] = [0.85, 0.63, 0.24, 1.0];  // mustard
const C_BUTTON_ACTIVE:  [f32; 4] = [1.00, 0.78, 0.30, 1.0];  // bright
const C_HEADER:         [f32; 4] = [0.32, 0.20, 0.07, 1.0];  // brown header bg
const C_FRAME_BG:       [f32; 4] = [0.18, 0.10, 0.04, 0.7];

fn draw_panel(ui: &Ui, panel: &mut ActivePanel, heading_font: Option<FontId>, body_font: Option<FontId>) {
    // Stamp first-render time the first time we draw this panel; used
    // by auto-close timing.
    if panel.first_drawn_at.is_none() {
        panel.first_drawn_at = Some(std::time::Instant::now());
    }
    // Auto-close: panels that carry an `auto_close_after_secs` field in
    // their payload (e.g. the TurdMOD loading splash) close themselves
    // once that many seconds have elapsed since first render.
    let p = &panel.payload;
    if let Some(secs) = p.get("auto_close_after_secs").and_then(JsonValue::as_f64) {
        if let Some(start) = panel.first_drawn_at {
            if start.elapsed().as_secs_f64() >= secs {
                panel.open = false;
                return;
            }
        }
    }
    let title = p.get("server_name").and_then(JsonValue::as_str).unwrap_or("TurdMOD");
    let tagline = p.get("tagline").and_then(JsonValue::as_str).unwrap_or("");
    let greeting = p.get("greeting").and_then(JsonValue::as_str).unwrap_or("");

    // Animated brand pulse: slow sin wave on the mustard channel so the
    // accent color softly breathes between mustard and bright-mustard.
    // Using ui.time() rather than a manually-tracked accumulator keeps
    // the animation independent of frame rate.
    let t = ui.time() as f32;
    let pulse = 0.5 + 0.5 * (t * 1.6).sin();          // [0..1]
    let mustard_pulse = lerp_rgba(C_MUSTARD, C_MUSTARD_BRIGHT, pulse);

    // Window/frame rounding makes the panel feel modern instead of boxy.
    // Pushed as StyleVar so it auto-pops at scope end.
    let _round_w = ui.push_style_var(StyleVar::WindowRounding(14.0));
    let _round_f = ui.push_style_var(StyleVar::FrameRounding(8.0));
    let _round_b = ui.push_style_var(StyleVar::ChildRounding(8.0));
    let _border  = ui.push_style_var(StyleVar::WindowBorderSize(2.0));

    // Push the TurdMOD palette. Every token returned must outlive the
    // window-build closure since ImGui pops in reverse order at scope end.
    // Holding them in a vec keeps the names short above.
    let _toks = [
        ui.push_style_color(StyleColor::WindowBg,         C_BG_DEEP),
        ui.push_style_color(StyleColor::ChildBg,          C_BG_MID),
        ui.push_style_color(StyleColor::PopupBg,          C_BG_DEEP),
        ui.push_style_color(StyleColor::Border,           C_BORDER),
        ui.push_style_color(StyleColor::TitleBg,          C_TITLE_BG),
        ui.push_style_color(StyleColor::TitleBgActive,    C_TITLE_BG_ACT),
        ui.push_style_color(StyleColor::TitleBgCollapsed, C_TITLE_BG),
        ui.push_style_color(StyleColor::Text,             C_TEXT),
        ui.push_style_color(StyleColor::TextDisabled,     C_TEXT_DIM),
        ui.push_style_color(StyleColor::Separator,        C_BORDER),
        ui.push_style_color(StyleColor::SeparatorHovered, C_MUSTARD),
        ui.push_style_color(StyleColor::SeparatorActive,  C_MUSTARD_BRIGHT),
        ui.push_style_color(StyleColor::Button,           C_BUTTON),
        ui.push_style_color(StyleColor::ButtonHovered,    C_BUTTON_HOVER),
        ui.push_style_color(StyleColor::ButtonActive,     C_BUTTON_ACTIVE),
        ui.push_style_color(StyleColor::Header,           C_HEADER),
        ui.push_style_color(StyleColor::HeaderHovered,    C_BUTTON_HOVER),
        ui.push_style_color(StyleColor::HeaderActive,     C_BUTTON_ACTIVE),
        ui.push_style_color(StyleColor::FrameBg,          C_FRAME_BG),
    ];

    // Size the panel to 60% of SCUM's viewport and center it. 75% was
    // too tight when combined with the 56px heading font -- the body
    // overflowed and the layout scrolled. 60% gives the heading and
    // body comfortable margins on both sides while still feeling like
    // a hero panel.
    let display = ui.io().display_size;
    let win_w = (display[0] * 0.60).max(640.0);
    let win_h = (display[1] * 0.60).max(480.0);
    let center_x = display[0] * 0.5;
    let center_y = display[1] * 0.5;

    // The window's X-button is a `&mut bool` slot; `Cell` lets the
    // dismiss-button closure mutate without re-borrowing.
    let mut x_open = panel.open;
    let dismiss_clicked = std::cell::Cell::new(false);

    // Always-on title with brand decoration. The two pipe + dash chars
    // give a weathered, stenciled feel that matches SCUM's prison-camp
    // signage motifs.
    let label = format!("== {title} == ##turdmod-panel-{:p}", p);
    ui.window(&label)
        .opened(&mut x_open)
        .size([win_w, win_h], Condition::Always)
        .position([center_x, center_y], Condition::Always)
        .position_pivot([0.5, 0.5])
        .build(|| {
            // Push the body font (24px Consolas) for every widget inside
            // this window unless we explicitly switch to the heading
            // font. Replaces ImGui's tiny ProggyClean default with
            // something that matches the brand surfaces and is legible
            // at SCUM's high-res menu scale. We DON'T use
            // set_window_font_scale here -- with a properly-sized base
            // font, scaling 2x just makes the layout overflow.
            let _body = body_font.map(|f| ui.push_font(f));

            // ----- Hero header (widget-only, no DrawList) -----
            //
            // The previous version used DrawList::add_rect_filled_multicolor
            // for a gradient + add_text + a custom-drawn turd glyph.
            // Both crashed SCUM on first render past the wall-clock
            // gate (~38s after attach). Bisected to the DrawList custom-
            // rendering path being incompatible with hudhook's per-frame
            // command stream on this build of SCUM. Until that's
            // resolved we render the hero header using ONLY standard
            // ImGui widgets, which were verified stable in the first
            // panel test (95s+ uptime, no crash).
            //
            // The brand still pops via the pushed color palette: deep
            // brown background, mustard accents, cream text. Animated
            // pulse runs on the tagline color via push_style_color
            // (no DrawList) which is also stable.
            // Banner — switches between two title styles depending on
            // whether `initialize()` successfully registered a font with
            // box-drawing glyphs. Font available → use the same chunky
            // ████ block art the log banners use, so the in-game panel
            // matches every other TurdMOD audit-log surface. No font →
            // fall back to the all-caps figlet (still readable, no
            // missing glyphs).
            if let Some(fid) = heading_font {
                let _f = ui.push_font(fid);
                // The same banner from logger output (lib.rs BANNER) +
                // the decorator DLL banner. Box-drawing chars rely on
                // the heading font; falls back to figlet if missing.
                let block_lines = [
                    "████████╗██╗   ██╗██████╗ ██████╗ ███╗   ███╗ ██████╗ ██████╗",
                    "╚══██╔══╝██║   ██║██╔══██╗██╔══██╗████╗ ████║██╔═══██╗██╔══██╗",
                    "   ██║   ██║   ██║██████╔╝██║  ██║██╔████╔██║██║   ██║██║  ██║",
                    "   ██║   ██║   ██║██╔══██╗██║  ██║██║╚██╔╝██║██║   ██║██║  ██║",
                    "   ██║   ╚██████╔╝██║  ██║██████╔╝██║ ╚═╝ ██║╚██████╔╝██████╔╝",
                    "   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═════╝",
                ];
                for line in block_lines.iter() {
                    ui.text(*line);
                }
            } else {
                let banner_lines = [
                    " _____ _   _ ____  ____  __  __  ___  ____  ",
                    "|_   _| | | |  _ \\|  _ \\|  \\/  |/ _ \\|  _ \\ ",
                    "  | | | | | | |_) | | | | |\\/| | | | | | | |",
                    "  | | | |_| |  _ <| |_| | |  | | |_| | |_| |",
                    "  |_|  \\___/|_| \\_\\____/|_|  |_|\\___/|____/ ",
                ];
                for line in banner_lines.iter() {
                    ui.text(*line);
                }
            }
            // Pulsing mustard tagline — this just changes the Text style
            // color via push_style_color; no DrawList API touched.
            {
                let _t = ui.push_style_color(StyleColor::Text, mustard_pulse);
                ui.text("                    >> modding engine for SCUM <<");
            }
            ui.spacing();
            ui.separator();

            // Live status strip: tells the player at a glance that the
            // loader is alive and what's wired up. Uses small colored
            // bullets so the eye lands on it before the body copy.
            let mode_str = match crate::detect::infer_mode() {
                crate::detect::Mode::Solo => "solo / single-player",
                crate::detect::Mode::PrivateBeOff => "private server (BE off)",
                crate::detect::Mode::PrivateBeOn => "private server (BE on) - inert",
                crate::detect::Mode::Unknown => "unknown - inert",
            };
            ui.text("status:");
            ui.same_line();
            {
                let _t = ui.push_style_color(StyleColor::Text, [0.55, 0.85, 0.40, 1.0]);
                ui.text("● loader OK");
            }
            ui.same_line();
            {
                let _t = ui.push_style_color(StyleColor::Text, [0.55, 0.85, 0.40, 1.0]);
                ui.text("● render OK");
            }
            ui.same_line();
            {
                let _t = ui.push_style_color(StyleColor::Text, mustard_pulse);
                ui.text(format!("● mode: {mode_str}"));
            }
            ui.spacing();
            ui.separator();

            if !tagline.is_empty() {
                ui.text_wrapped(tagline);
                ui.spacing();
            }
            if !greeting.is_empty() {
                ui.text_wrapped(greeting);
                ui.spacing();
            }
            ui.separator();

            if let Some(sections) = p.get("sections").and_then(JsonValue::as_array) {
                for section in sections {
                    let kind = section.get("kind").and_then(JsonValue::as_str).unwrap_or("");
                    let s_title = section.get("title").and_then(JsonValue::as_str).unwrap_or("");
                    if !s_title.is_empty() {
                        // Section heading in mustard; rest of section in cream.
                        let _t = ui.push_style_color(StyleColor::Text, C_MUSTARD);
                        ui.text(format!(">> {s_title}"));
                    }
                    match kind {
                        "rules" => {
                            if let Some(items) = section.get("items").and_then(JsonValue::as_array) {
                                for (i, item) in items.iter().enumerate() {
                                    if let Some(text) = item.as_str() {
                                        ui.bullet_text(format!("{}. {}", i + 1, text));
                                    }
                                }
                            }
                        }
                        "mod_brag" => {
                            if let Some(mods) = section.get("mods").and_then(JsonValue::as_array) {
                                for m in mods {
                                    let emoji = m.get("emoji").and_then(JsonValue::as_str).unwrap_or("*");
                                    let name = m.get("name").and_then(JsonValue::as_str).unwrap_or("");
                                    let tagline = m.get("tagline").and_then(JsonValue::as_str).unwrap_or("");
                                    ui.bullet_text(format!("{emoji} {name} -- {tagline}"));
                                }
                            }
                        }
                        "commands" => {
                            if let Some(items) = section.get("items").and_then(JsonValue::as_array) {
                                for item in items {
                                    let name = item.get("name").and_then(JsonValue::as_str).unwrap_or("");
                                    let desc = item.get("description").and_then(JsonValue::as_str).unwrap_or("");
                                    ui.bullet_text(format!("{name} -- {desc}"));
                                }
                            }
                        }
                        _ => {}
                    }
                    ui.spacing();
                    ui.separator();
                }
            }

            // Push the bottom row to the lower edge of the window. We
            // compute the remaining vertical space and pad to within
            // ~110px of the bottom (room for the button + footer).
            let region = ui.content_region_avail();
            let pad = (region[1] - 110.0).max(8.0);
            ui.dummy([0.0, pad]);

            // ----- Big centered Dismiss button -----
            //
            // Joel asked for "much bigger near the bottom middle". We
            // size to ~30% of window width (clamped to a sane minimum)
            // and 60px tall, then horizontally center via cursor_pos.
            // Link items render as small cream text below the button so
            // they don't compete with it visually.
            let win_size = ui.window_size();
            let btn_w = (win_size[0] * 0.30).max(220.0);
            let btn_h: f32 = 60.0;
            if let Some(actions) = p.get("actions").and_then(JsonValue::as_array) {
                let dismiss_label = actions.iter().find_map(|a| {
                    let k = a.get("kind").and_then(JsonValue::as_str)?;
                    if k == "dismiss" { a.get("label").and_then(JsonValue::as_str) } else { None }
                }).unwrap_or("Dismiss");
                let cur = ui.cursor_pos();
                ui.set_cursor_pos([(win_size[0] - btn_w) * 0.5, cur[1]]);
                if ui.button_with_size(dismiss_label, [btn_w, btn_h]) {
                    dismiss_clicked.set(true);
                }
                // Link items as small text below, centered.
                for action in actions {
                    let kind = action.get("kind").and_then(JsonValue::as_str).unwrap_or("");
                    if kind != "link" { continue; }
                    let lbl = action.get("label").and_then(JsonValue::as_str).unwrap_or("");
                    let lbl_w = lbl.len() as f32 * 7.5; // approx pixel width per char
                    let cur2 = ui.cursor_pos();
                    ui.set_cursor_pos([(win_size[0] - lbl_w) * 0.5, cur2[1] + 6.0]);
                    let _t = ui.push_style_color(StyleColor::Text, C_TEXT_DIM);
                    ui.text(lbl);
                }
            }

            // Footer brand line in dim cream — bottom-most.
            ui.dummy([0.0, 6.0]);
            ui.separator();
            {
                let _t = ui.push_style_color(StyleColor::Text, C_TEXT_DIM);
                let footer = format!(
                    "loader v{}  --  github.com/roketteere/turdmod",
                    env!("CARGO_PKG_VERSION")
                );
                let f_w = footer.len() as f32 * 7.5;
                let cur3 = ui.cursor_pos();
                ui.set_cursor_pos([(win_size[0] - f_w) * 0.5, cur3[1]]);
                ui.text(footer);
            }
        });

    panel.open = x_open && !dismiss_clicked.get();
}
