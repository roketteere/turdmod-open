//! No-op build script kept as a placeholder.
//!
//! Earlier drafts emitted a dxgi.dll proxy `.def` here, but the proxy
//! approach has a name-collision wart (a DLL named dxgi.dll can't forward
//! exports to itself), and we ship a standalone launcher.exe that does
//! standard CreateRemoteThread injection anyway. If we ever want a
//! no-launcher install path we'll bring this file back to life — for
//! now the loader DLL has no exports beyond its diagnostic API.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
