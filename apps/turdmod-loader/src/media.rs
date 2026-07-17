//! Media transport control for the in-game "radio" → Apple Music (browser) bridge.
//!
//! Sends OS media keys via SendInput. Media keys (VK_MEDIA_*) are dispatched by
//! the OS to the active media session (WM_APPCOMMAND) regardless of which window
//! has focus — so Apple Music playing in an UNFOCUSED browser tab still responds
//! while SCUM is foregrounded. No browser automation needed for play/pause/skip.
//!
//! @ctx station-by-name (jump to a specific Apple Music playlist) is NOT possible
//! via media keys — that's the gemini-bridge browser-control job (phase 2).

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
};

use crate::logging;

/// Tap a virtual key (down then up) through SendInput.
unsafe fn tap(vk: u16) {
    let mk = |flags: u32| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    let mut inputs = [mk(0), mk(KEYEVENTF_KEYUP)];
    SendInput(inputs.len() as u32, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
}

/// Toggle play/pause on the active media session (Apple Music in the browser).
pub fn play_pause() {
    unsafe { tap(VK_MEDIA_PLAY_PAUSE) };
    logging::log("[media] play/pause");
}

/// Next track — maps to "station up" on the in-game radio.
pub fn next_track() {
    unsafe { tap(VK_MEDIA_NEXT_TRACK) };
    logging::log("[media] next track");
}

/// Previous track — maps to "station down".
pub fn prev_track() {
    unsafe { tap(VK_MEDIA_PREV_TRACK) };
    logging::log("[media] prev track");
}
