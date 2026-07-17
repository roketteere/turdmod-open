// Tauri entry point. All real work lives in the lib crate so it can be
// unit-tested without depending on Tauri's runtime.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    turdmod_lite_lib::run();
}
