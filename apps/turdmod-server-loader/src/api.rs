//! The `turdmod` Lua API surface for server-side mods.
//!
//! Identical to the client loader's api.rs with one difference:
//! `turdmod.notify_panel` is a log-only stub on the server — there is no
//! in-game render pipeline to draw panels into. Server mods that want to
//! communicate back to the operator use `turdmod.log` or the admin API
//! (Part B, `admin_api.rs`).
//!
//! The persistence table uses the server-mods root so each server mod's
//! data directory lands under `%PROGRAMDATA%\TurdMOD\server-mods\<id>\data\`.

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value as LuaValue};
use parking_lot::Mutex;
use serde_json::Value as JsonValue;

use crate::logging;
use crate::runtime::Handlers;

pub fn build_api(
    lua: &Lua,
    handlers: Handlers,
    mods_root: PathBuf,
) -> mlua::Result<()> {
    let turdmod = lua.create_table()?;

    turdmod.set("version", env!("CARGO_PKG_VERSION"))?;
    turdmod.set("mod_id", "")?;

    // turdmod.log(level, msg [, data])
    let log_fn = lua.create_function(|lua, (level, msg, data): (String, String, Option<LuaValue>)| {
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        let suffix = if let Some(d) = data {
            let json: serde_json::Value = lua.from_value(d).unwrap_or(serde_json::Value::Null);
            format!(" {json}")
        } else { String::new() };
        logging::log(&format!("[mod:{mid}] {level} {msg}{suffix}"));
        Ok(())
    })?;
    turdmod.set("log", log_fn)?;

    // turdmod.on(channel, fn)
    let handlers_for_on = handlers.clone();
    let on_fn = lua.create_function(move |lua, (channel, func): (String, Function)| {
        let key = lua.create_registry_value(func)?;
        handlers_for_on.lock().entry(channel.clone()).or_default().push(key);
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        logging::log(&format!("[mod:{mid}] registered handler for {channel}"));
        Ok(())
    })?;
    turdmod.set("on", on_fn)?;

    // turdmod.dispatch(channel, payload)
    let handlers_for_dispatch = handlers.clone();
    let dispatch_fn = lua.create_function(move |lua, (channel, payload): (String, LuaValue)| {
        let json: JsonValue = lua.from_value(payload).unwrap_or(JsonValue::Null);
        dispatch_to_handlers(lua, &handlers_for_dispatch, &channel, json);
        Ok(())
    })?;
    turdmod.set("dispatch", dispatch_fn)?;

    // turdmod.notify_panel — stub on the server loader.
    let notify_fn = lua.create_function(|lua, _panel: Table| {
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        logging::log(&format!(
            "[mod:{mid}] turdmod.notify_panel called — no-op on server loader \
             (no rendering pipeline). Use turdmod.admin.notify() once Part B lands."
        ));
        Ok(())
    })?;
    turdmod.set("notify_panel", notify_fn)?;

    // turdmod.persistence
    let persistence = lua.create_table()?;
    let mods_root_for_get = mods_root.clone();
    let get_fn = lua.create_function(move |lua, key: String| {
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        let path = persistence_path(&mods_root_for_get, &mid, &key);
        let json: JsonValue = match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or(JsonValue::Null),
            Err(_) => return Ok(LuaValue::Nil),
        };
        if matches!(json, JsonValue::Null) {
            return Ok(LuaValue::Nil);
        }
        let val: LuaValue = lua.to_value(&json)?;
        Ok(val)
    })?;
    persistence.set("get", get_fn)?;

    let mods_root_for_set = mods_root.clone();
    let set_fn = lua.create_function(move |lua, (key, value): (String, LuaValue)| {
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        let path = persistence_path(&mods_root_for_set, &mid, &key);
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
        let json: JsonValue = lua.from_value(value).unwrap_or(JsonValue::Null);
        std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default()).ok();
        Ok(())
    })?;
    persistence.set("set", set_fn)?;

    turdmod.set("persistence", persistence)?;

    lua.globals().set("turdmod", turdmod)?;
    Ok(())
}

fn persistence_path(mods_root: &Path, mod_id: &str, key: &str) -> PathBuf {
    let safe_key = key.replace(['/', '\\', ':'], "_");
    mods_root.join(mod_id).join("data").join(format!("{safe_key}.json"))
}

pub fn dispatch_to_handlers(
    lua: &Lua,
    handlers: &Mutex<std::collections::HashMap<String, Vec<RegistryKey>>>,
    channel: &str,
    payload: JsonValue,
) {
    let g = handlers.lock();
    let v = match g.get(channel) {
        Some(v) => v,
        None => return,
    };
    for key in v.iter() {
        let func: Function = match lua.registry_value(key) {
            Ok(f) => f,
            Err(e) => {
                logging::log(&format!("[runtime] dispatch: bad handler key for {channel}: {e}"));
                continue;
            }
        };
        let lua_val: LuaValue = match lua.to_value(&payload) {
            Ok(v) => v,
            Err(e) => {
                logging::log(&format!("[runtime] dispatch: payload→lua failed: {e}"));
                continue;
            }
        };
        if let Err(e) = func.call::<()>(lua_val) {
            logging::log(&format!("[runtime] handler error on {channel}: {e}"));
        }
    }
}
