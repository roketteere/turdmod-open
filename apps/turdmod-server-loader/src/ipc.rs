//! Outbound IPC — pushes game events from the server loader to the
//! companion's `/ingest` endpoint.
//!
//! Role compared to the client loader's ipc.rs: the client SUBSCRIBES to
//! the companion (SSE consumer); the server loader PUBLISHES to it (HTTP
//! POST producer). This mirrors the data-flow direction: game events
//! originate inside SCUMServer.exe (via server_hooks.rs, Part C) and need
//! to flow out to the companion, which in turn pushes them to connected
//! browser clients.
//!
//! Discovery: same contract as the client — reads
//! `~/.scummy-map/turdmod-companion.json` for the companion URL, with a
//! `TURDMOD_COMPANION_URL` env override and a `http://127.0.0.1:8911`
//! fallback.
//!
//! Lifecycle: `start_outbound()` spawns a worker thread that drains a
//! shared event queue, POSTing each batch to `companion_url/ingest`.
//! The queue is filled by `enqueue_event(json)` — called from
//! server_hooks.rs (Part C) as game events occur.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value as JsonValue;

use crate::logging;

static EVENT_QUEUE: std::sync::OnceLock<Arc<Mutex<VecDeque<JsonValue>>>> =
    std::sync::OnceLock::new();

fn queue() -> &'static Arc<Mutex<VecDeque<JsonValue>>> {
    EVENT_QUEUE.get_or_init(|| Arc::new(Mutex::new(VecDeque::new())))
}

pub fn enqueue_event(event: JsonValue) {
    queue().lock().push_back(event);
}

const FLUSH_INTERVAL: Duration = Duration::from_millis(500);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub fn start_outbound() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let q = Arc::clone(queue());
    thread::spawn(move || {
        loop {
            thread::sleep(FLUSH_INTERVAL);
            let batch: Vec<JsonValue> = {
                let mut guard = q.lock();
                guard.drain(..).collect()
            };
            if batch.is_empty() {
                continue;
            }
            let url = resolve_companion_url();
            let ingest_url = format!("{}/ingest", url.trim_end_matches('/'));
            let body = serde_json::Value::Array(batch);
            let body_str = match serde_json::to_string(&body) {
                Ok(s) => s,
                Err(e) => {
                    logging::log(&format!("[ipc] serialize failed: {e}"));
                    continue;
                }
            };
            match ureq::post(&ingest_url)
                .set("Content-Type", "application/json")
                .send_string(&body_str)
            {
                Ok(resp) => {
                    if resp.status() != 200 {
                        logging::log(&format!(
                            "[ipc] companion /ingest returned status {}",
                            resp.status()
                        ));
                    }
                }
                Err(e) => {
                    logging::log(&format!("[ipc] /ingest POST failed: {e}; will retry next flush"));
                    thread::sleep(RECONNECT_DELAY);
                }
            }
        }
    });
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
