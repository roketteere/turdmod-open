// Prevent the second console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    turdmod_manager_lib::run()
}
