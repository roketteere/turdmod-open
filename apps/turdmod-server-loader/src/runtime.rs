//! Lua mod runtime for the server-side loader.
//!
//! Identical in structure to `apps/turdmod-loader/src/runtime.rs` but
//! discovers mods under `%PROGRAMDATA%\TurdMOD\server-mods\` instead of
//! the client's `%LOCALAPPDATA%\TurdMOD\mods\`. This keeps client mods
//! and server mods cleanly separated — a server operator drops mod
//! folders into the server-mods directory; players drop client mods into
//! their own LOCALAPPDATA directory.

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
        let mods_root = server_mods_root_dir();
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
        let entries = match std::fs::read_dir(&self.inner.mods_root) {
            Ok(e) => e,
            Err(e) => {
                logging::log(&format!(
                    "[runtime] server-mods dir not readable: {} ({e})",
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
            if let Err(e) = self.load_mod(&id, &main_lua) {
                logging::log(&format!("[runtime] load failed {id}: {e}"));
            }
        }
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
        logging::log(&format!("[runtime] loaded server mod {id}"));
        Ok(())
    }

    pub fn dispatch(&self, channel: &str, payload: JsonValue) {
        let lua = self.inner.lua.lock();
        crate::api::dispatch_to_handlers(&lua, &self.inner.handlers, channel, payload);
    }

    pub fn loaded_mods(&self) -> Vec<String> {
        self.inner.loaded_mods.lock().clone()
    }
}

pub fn get_runtime_handle() -> Option<Runtime> {
    crate::runtime_handle()
}

fn server_mods_root_dir() -> PathBuf {
    let programdata = std::env::var_os("PROGRAMDATA").unwrap_or_default();
    let mut p = PathBuf::from(programdata);
    p.push("TurdMOD");
    p.push("server-mods");
    p
}
