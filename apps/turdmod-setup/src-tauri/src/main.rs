// Prevents a console window from appearing alongside the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    turdmod_setup_app_lib::run()
}
