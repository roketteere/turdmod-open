fn main() {
    // Cargo doesn't know that the embedded Windows icon (and other
    // platform-icon resources) live under icons/ — so changing them
    // without these directives leaves the dev binary stale until you
    // `cargo clean`. tauri.conf.json is the same — a path change there
    // should also trigger a rebuild.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build();
}
