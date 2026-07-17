//! The `turdmod` Lua API surface.
//!
//! Mods see a single global table; everything they call hangs off it:
//!
//!   turdmod.log("info", "loaded")
//!   turdmod.on("system.login", function(p)
//!     turdmod.notify_panel({
//!       title = "Welcome",
//!       body  = "Hello " .. p.player,
//!       dismiss_key = "Escape",
//!     })
//!   end)
//!   turdmod.persistence.set("counter", 1)
//!   turdmod.persistence.get("counter")
//!
//! `notify_panel` is a stub today — once #169 lands the UE4 hook layer,
//! it'll route to the actual in-game panel renderer. For now it logs.

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

    // turdmod.version
    turdmod.set("version", env!("CARGO_PKG_VERSION"))?;
    // mod_id is set per-mod by Runtime::load_mod before exec.
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

    // turdmod.on(channel, fn) — register a handler
    let handlers_for_on = handlers.clone();
    let on_fn = lua.create_function(move |lua, (channel, func): (String, Function)| {
        let key = lua.create_registry_value(func)?;
        handlers_for_on.lock().entry(channel.clone()).or_default().push(key);
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        logging::log(&format!("[mod:{mid}] registered handler for {channel}"));
        Ok(())
    })?;
    turdmod.set("on", on_fn)?;

    // turdmod.dispatch(channel, payload) — manual dispatch (tests + admin)
    let handlers_for_dispatch = handlers.clone();
    let dispatch_fn = lua.create_function(move |lua, (channel, payload): (String, LuaValue)| {
        let json: JsonValue = lua.from_value(payload).unwrap_or(JsonValue::Null);
        dispatch_to_handlers(lua, &handlers_for_dispatch, &channel, json);
        Ok(())
    })?;
    turdmod.set("dispatch", dispatch_fn)?;

    // turdmod.notify_panel(payload) — queue a panel for the in-game
    // renderer (#169). Accepts the full panel-payload shape (theme,
    // banner, sections[], actions[]) AND the legacy short form
    // ({title, body, dismiss_key}); short form gets shimmed to the
    // full shape so existing mods keep working. The renderer drains
    // the queue every frame.
    let notify_fn = lua.create_function(|lua, panel: Table| {
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();

        let raw: JsonValue = lua
            .from_value(LuaValue::Table(panel.clone()))
            .unwrap_or(JsonValue::Null);

        // Shim short-form payloads ({title, body, dismiss_key}) into the
        // canonical {server_name, greeting, actions[]} shape the renderer
        // reads. Detection: short-form has `title` but no `server_name`
        // and no `sections` array.
        let payload = if raw.get("server_name").is_none()
            && raw.get("sections").is_none()
            && raw.get("title").is_some()
        {
            let title = raw.get("title").and_then(JsonValue::as_str).unwrap_or("");
            let body = raw.get("body").and_then(JsonValue::as_str).unwrap_or("");
            let dismiss = raw.get("dismiss_key").and_then(JsonValue::as_str).unwrap_or("Escape");
            serde_json::json!({
                "kind": "panel",
                "version": 1,
                "server_name": title,
                "greeting": body,
                "actions": [{"kind": "dismiss", "label": "Got it", "key": dismiss}],
            })
        } else {
            raw
        };

        crate::hooks::enqueue_panel(payload.clone());
        logging::log(&format!(
            "[panel:{mid}] queued for renderer ({} bytes)",
            serde_json::to_string(&payload).map(|s| s.len()).unwrap_or(0)
        ));
        Ok(())
    })?;
    turdmod.set("notify_panel", notify_fn)?;

    // turdmod.persistence — per-mod JSON file under <mods_root>/<id>/data/
    let persistence = lua.create_table()?;
    let mods_root_for_get = mods_root.clone();
    let get_fn = lua.create_function(move |lua, key: String| {
        let mid: String = lua.globals().get::<Table>("turdmod")?.get::<String>("mod_id").unwrap_or_default();
        let path = persistence_path(&mods_root_for_get, &mid, &key);
        // Missing-file or null-content collapses to Lua nil — NOT mlua's
        // userdata-Null — so mods can write idiomatic
        //     local x = turdmod.persistence.get("k") or {}
        // and have it work the same way it does in JS.
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

/// Run every registered handler for `channel` with the given JSON payload.
/// Errors are logged but don't unwind further — one bad mod shouldn't take
/// down event dispatch for the others.
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
