//! Lua mod runtime hosted inside the loader DLL.
//!
//! Once `init_thread` decides the environment permits scripting, it calls
//! `Runtime::start()` which:
//!
//!   1. Boots a Lua 5.4 VM (vendored).
//!   2. Builds the `turdmod` global table — the mod-author surface
//!      (`turdmod.on`, `turdmod.log`, `turdmod.persistence`, ...).
//!   3. Discovers mod directories under `%LOCALAPPDATA%/TurdMOD/mods/<id>/`
//!      with a `main.lua` entry, runs each so they can register handlers.
//!
//! Event dispatch: a shared `HashMap<channel, Vec<RegistryKey>>` lives in
//! a single Arc<Mutex<...>>. The API closures (in `api.rs`) write into
//! it; `Runtime::dispatch` reads from it. Same Arc, same data.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mlua::{Lua, RegistryKey};
use parking_lot::Mutex;
use serde_json::Value as JsonValue;

use crate::logging;

pub type Handlers = Arc<Mutex<HashMap<String, Vec<RegistryKey>>>>;

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    lua: Mutex<Lua>,
    handlers: Handlers,
    mods_root: PathBuf,
    loaded_mods: Mutex<Vec<String>>,
}

impl Runtime {
    pub fn start() -> Result<Self, String> {
        let lua = Lua::new();
        let handlers: Handlers = Arc::new(Mutex::new(HashMap::new()));
        let mods_root = mods_root_dir();
        std::fs::create_dir_all(&mods_root).ok();

        crate::api::build_api(&lua, handlers.clone(), mods_root.clone())
            .map_err(|e| format!("build api: {e}"))?;

        Ok(Runtime {
            inner: Arc::new(RuntimeInner {
                lua: Mutex::new(lua),
                handlers,
                mods_root,
                loaded_mods: Mutex::new(Vec::new()),
            }),
        })
    }

    pub fn discover_and_load(&self) {
        // Enable-list gate: the launcher writes mods/enabled.json when the
        // player toggles mods. Absent ⇒ load everything (back-compat with
        // installs that predate the launcher). Present ⇒ load only the ids
        // it lists. @dep: launcher/turdmod-launcher set_enabled_mods.
        let enabled = self.read_enabled_list();
        match &enabled {
            Some(ids) => logging::log(&format!(
                "[runtime] enabled.json present — loading {} listed mod(s)",
                ids.len()
            )),
            None => logging::log("[runtime] no enabled.json — loading all installed mods"),
        }

        let entries = match std::fs::read_dir(&self.inner.mods_root) {
            Ok(e) => e,
            Err(e) => {
                logging::log(&format!(
                    "[runtime] mods dir not readable: {} ({e})",
                    self.inner.mods_root.display()
                ));
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let main_lua = path.join("main.lua");
            if !main_lua.is_file() { continue; }
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() { continue; }
            if let Some(ids) = &enabled {
                if !ids.iter().any(|e| e == &id) {
                    logging::log(&format!("[runtime] skipping disabled mod {id}"));
                    continue;
                }
            }
            if let Err(e) = self.load_mod(&id, &main_lua) {
                logging::log(&format!("[runtime] load failed {id}: {e}"));
            }
        }
    }

    /// Reads `mods/enabled.json` (`{ "enabled": ["id", ...] }`). Returns
    /// `None` if the file is absent or unparseable — callers treat `None`
    /// as "load everything" so a corrupt file never silently disables all
    /// mods. The file is a sibling of the per-mod folders, not inside one.
    fn read_enabled_list(&self) -> Option<Vec<String>> {
        let path = self.inner.mods_root.join("enabled.json");
        let raw = std::fs::read_to_string(path).ok()?;
        let v: JsonValue = serde_json::from_str(&raw).ok()?;
        let arr = v.get("enabled")?.as_array()?;
        Some(
            arr.iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect(),
        )
    }

    fn load_mod(&self, id: &str, main_lua: &std::path::Path) -> Result<(), String> {
        let src = std::fs::read_to_string(main_lua).map_err(|e| e.to_string())?;
        let lua = self.inner.lua.lock();

        let turdmod_tbl: mlua::Table = lua
            .globals()
            .get("turdmod")
            .map_err(|e| format!("no `turdmod` global: {e}"))?;
        turdmod_tbl
            .set("mod_id", id.to_string())
            .map_err(|e| format!("set mod_id: {e}"))?;

        lua.load(&src)
            .set_name(format!("mod:{id}/main.lua"))
            .exec()
            .map_err(|e| format!("exec: {e}"))?;

        self.inner.loaded_mods.lock().push(id.to_string());
        logging::log(&format!("[runtime] loaded mod {id}"));
        Ok(())
    }

    /// Dispatch a payload to every Lua handler registered for `channel`.
    /// Caller-side: companion-IPC + tests use this entry point.
    pub fn dispatch(&self, channel: &str, payload: JsonValue) {
        let lua = self.inner.lua.lock();
        crate::api::dispatch_to_handlers(&lua, &self.inner.handlers, channel, payload);
    }

    pub fn loaded_mods(&self) -> Vec<String> {
        self.inner.loaded_mods.lock().clone()
    }
}

/// Sibling of the lib-internal `runtime()` — exposed at the module level
/// so other modules (the IPC subscriber, the hook layer) can fetch a
/// clone of the live runtime handle.
pub fn get_runtime_handle() -> Option<Runtime> {
    crate::runtime_handle()
}

fn mods_root_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    let mut p = PathBuf::from(local);
    p.push("TurdMOD");
    p.push("mods");
    p
}
