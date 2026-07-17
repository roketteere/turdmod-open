//! TurdMOD Launcher — Tauri backend.
//!
//! Thin command surface over `turdmod_launcher_core` (the shared injection
//! lib) plus the our-servers allowlist fetch and mod enable/disable state.
//! The unsafe injection itself lives in the lib, not here.

mod commands;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

// Bring the main window back to the foreground (tray click / "Open" menu).
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // System-tray turd: the official TurdMOD mark lives in the
            // notification area so the launcher is reachable when minimized.
            // @dep icons/icon.ico (regenerated from brand/turdmod-icon-1024.png).
            let open = MenuItem::with_id(app, "open", "Open TurdMOD", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("turdmod-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("TurdMOD Launcher")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| match ev.id.as_ref() {
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = ev
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::launcher_list_servers,
            commands::launcher_list_mods,
            commands::launcher_set_enabled_mods,
            commands::launcher_launch_modded,
            commands::launcher_pid_alive,
            commands::launcher_join_progress,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TurdMOD Launcher");
}
