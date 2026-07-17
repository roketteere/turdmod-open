//! IPC subscriber — connects to the companion's /events SSE stream and
//! dispatches incoming events into the Lua runtime.
//!
//! Discovery: reads `~/.scummy-map/turdmod-companion.json` for the
//! companion's URL. Falls back to `http://127.0.0.1:8911` (the historic
//! default in companion's CLI flag set). Honors
//! `TURDMOD_COMPANION_URL` env override.
//!
//! Lifecycle: `start_subscriber()` spawns a worker thread that loops:
//!   1. read discovery → URL
//!   2. open GET /events
//!   3. parse `data: <json>` lines → ServerEvent
//!   4. call `runtime.dispatch(channel, payload)` for each
//!   5. on disconnect, sleep 2s and retry
//!
//! No async runtime needed — `ureq` is blocking, the dedicated thread
//! makes that fine. The Lua runtime's dispatch is mutex-guarded so the
//! worker can call it freely.

use std::io::BufRead;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde_json::Value as JsonValue;

use crate::logging;
use crate::runtime;

const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub fn start_subscriber() {
    thread::spawn(|| {
        loop {
            match connect_and_pump() {
                Ok(()) => {
                    logging::log("[ipc] companion stream closed cleanly; reconnecting in 2s");
                }
                Err(e) => {
                    logging::log(&format!("[ipc] subscriber error: {e}; reconnecting in 2s"));
                }
            }
            thread::sleep(RECONNECT_DELAY);
        }
    });
}

fn connect_and_pump() -> Result<(), String> {
    let url = resolve_companion_url();
    let events_url = format!("{}/events", url.trim_end_matches('/'));
    logging::log(&format!("[ipc] connecting to {events_url}"));

    // Use an agent with a per-call connect timeout but no read timeout —
    // SSE streams are long-poll by design and silent intervals between
    // heartbeats are expected (the companion sends one every 25s).
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .build();
    let resp = agent
        .get(&events_url)
        .call()
        .map_err(|e| format!("get {events_url}: {e}"))?;
    if resp.status() != 200 {
        return Err(format!("status {}", resp.status()));
    }
    logging::log("[ipc] connected to companion /events");

    let reader = std::io::BufReader::new(resp.into_reader());
    for line in reader.lines() {
        let line = line.map_err(|e| format!("read: {e}"))?;
        let line = line.trim_start_matches("data: ");
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue; // SSE comment / heartbeat
        }
        let event: JsonValue = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        dispatch_event(&event);
    }
    Ok(())
}

fn dispatch_event(event: &JsonValue) {
    let kind = match event.get("kind").and_then(JsonValue::as_str) {
        Some(k) => k,
        None => return,
    };
    let rt = match runtime::get_runtime_handle() {
        Some(h) => h,
        None => return, // runtime not up yet; drop
    };
    // Mirror the companion's runtime.dispatch fan-out: each ServerEvent
    // turns into one or more `system.*` / `vehicle.*` channels.
    let channels: Vec<String> = match kind {
        "login" => vec!["system.login".into()],
        "logout" => vec!["system.logout".into()],
        "kill" => vec!["system.kill".into()],
        "vehicle" => {
            let evt = event.get("event").and_then(JsonValue::as_str).unwrap_or("");
            let slug = evt.to_ascii_lowercase().replace(' ', "_");
            vec![format!("vehicle.{slug}"), "vehicle.any".into()]
        }
        "bunker" => vec!["system.bunker".into()],
        "chat" => vec!["system.chat".into()],
        "admin" => vec!["system.admin".into()],
        _ => return,
    };
    for ch in channels {
        rt.dispatch(&ch, event.clone());
    }
}

fn resolve_companion_url() -> String {
    if let Ok(env) = std::env::var("TURDMOD_COMPANION_URL") {
        return env;
    }
    if let Some(path) = discovery_path() {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<JsonValue>(&s) {
                if let Some(url) = json.get("url").and_then(JsonValue::as_str) {
                    return url.to_string();
                }
            }
        }
    }
    "http://127.0.0.1:8911".to_string()
}

fn discovery_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let mut p = PathBuf::from(home);
    p.push(".scummy-map");
    p.push("turdmod-companion.json");
    Some(p)
}
