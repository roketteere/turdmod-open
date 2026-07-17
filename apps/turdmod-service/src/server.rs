use crate::auth::bearer_auth;
use crate::config::Config;
use crate::engine::{self, SharedState};
use crate::map_tracker::SharedSnapshot;
use crate::permissions::SharedPerms;
use crate::ollama;
use crate::pipe_rpc;
use crate::scumdb;
use crate::scummap_push;
use crate::shutdown;
use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};

#[derive(Serialize)]
struct StatusResp {
    server_running: bool,
    pid: Option<u32>,
    uptime_secs: u64,
    restarts: u32,
    build: &'static str,
    /// Effective-next-launch BattlEye state (true = secure/no -NoBattlEye flag).
    battleye: bool,
}

#[derive(Deserialize)]
struct BattlEyeReq {
    enabled: bool,
}

#[derive(Serialize)]
struct OkResp { ok: bool }

#[derive(Serialize)]
struct HealthResp {
    service: &'static str,
    version: &'static str,
    build: &'static str,
    uptime_secs: u64,
}

#[derive(Deserialize)]
struct LogsQuery {
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct RpcReq {
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Clone)]
struct AppState {
    cfg: Arc<Config>,
    engine: SharedState,
    service_start: Instant,
    snapshot: SharedSnapshot,
    perms: SharedPerms,
}

pub async fn run_server(cfg: Config, engine: SharedState, snapshot: SharedSnapshot, perms: SharedPerms, shutdown_rx: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
    let token = cfg.token.clone();
    let app_state = AppState {
        cfg: Arc::new(cfg.clone()),
        engine,
        service_start: Instant::now(),
        snapshot,
        perms,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let protected = Router::new()
        .route("/status", get(handle_status))
        .route("/server/start", post(handle_start))
        .route("/server/stop", post(handle_stop))
        .route("/server/restart", post(handle_restart))
        .route("/server/battleye", post(handle_set_battleye))
        .route("/server/schedule", get(handle_get_schedule).post(handle_set_schedule))
        .route("/scumdb/elevated", get(handle_get_elevated).post(handle_set_elevated))
        .route("/backup/archive", post(handle_backup_archive))
        .route("/scumdb/migrate", post(handle_migrate))
        .route("/restore/campaign", get(handle_campaign_status))
        .route("/restore/campaign/arm", post(handle_campaign_arm))
        .route("/restore/campaign/disarm", post(handle_campaign_disarm))
        .route("/loot/files", get(handle_loot_files))
        .route("/loot/file", get(handle_loot_file_get).post(handle_loot_file_set))
        .route("/server/logs", get(handle_logs))
        .route("/engine/rpc", post(handle_engine_rpc))
        .route("/ollama/generate", post(handle_ollama_generate))
        .route("/ollama/models", get(handle_ollama_models))
        .route("/rcon", post(handle_rcon))
        .route("/service/activity", get(handle_service_activity))
        .route("/scumdb/tables", get(handle_scumdb_tables))
        .route("/scumdb/schema", get(handle_scumdb_schema))
        .route("/scumdb/rows", get(handle_scumdb_rows))
        .route("/scumdb/traders", get(handle_scumdb_traders))
        .route("/scumdb/player-card", get(handle_player_card))
        .route("/scumdb/set-skill", post(handle_set_skill))
        .route("/scumdb/set-attribute", post(handle_set_attribute))
        .route("/scumdb/player-inventory", get(handle_player_inventory))
        .route("/scumdb/last-position", get(handle_last_position))
        .route("/scumdb/last-positions", get(handle_last_positions))
        .route("/scumdb/map-vehicles", get(handle_map_vehicles))
        .route("/scumdb/vehicles-detail", get(handle_vehicles_detail))
        .route("/vehicles/repo", post(handle_vehicles_repo))
        .route("/scumdb/map-zones", get(handle_map_zones))
        .route("/scumdb/entities", get(handle_scumdb_entities))
        .route("/scummap/push-traders", post(handle_scummap_push_traders))
        // scumpilot (AI admin) control — proxied to its localhost HTTP endpoint so
        // the web manager drives the brain via the service it already auths to.
        .route("/scumpilot/status", get(handle_scumpilot_status))
        .route("/scumpilot/pause", post(handle_scumpilot_pause))
        .route("/scumpilot/resume", post(handle_scumpilot_resume))
        .route("/links/issue", post(handle_links_issue))
        .route("/monitor/mods", get(handle_monitor_mods))
        .route("/monitor/activity", get(handle_monitor_activity))
        .route("/monitor/system", get(handle_monitor_system))
        .route("/monitor/players", get(handle_monitor_players))
        .route("/chat/recent", get(handle_chat_recent))
        .route("/control/mod", post(handle_control_mod))
        .route("/server/settings", get(handle_get_server_settings).post(handle_save_server_settings))
        .route("/admin/file", get(handle_get_admin_file).post(handle_write_admin_file))
        .route("/data/file", get(handle_get_data_file).post(handle_write_data_file))
        .route("/permissions", get(handle_get_permissions).post(handle_set_permissions))
        .layer(middleware::from_fn(bearer_auth))
        .layer(Extension(token));

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/map/players", get(handle_map_players))
        .merge(protected)
        .layer(cors)
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    tracing::info!("HTTP listening on {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown::wait(shutdown_rx))
        .await?;
    tracing::info!("HTTP server shut down");
    Ok(())
}

// ─── Monitoring + control — backs turdmod-manager's Live page (per-mod 🟢🟡🔴 + activity feed +
// enable/disable/maintenance). Reads/writes the registry's global Monitor; no AppState needed.
#[derive(Deserialize)]
struct ActivityQuery { limit: Option<usize> }

async fn handle_monitor_mods() -> impl IntoResponse {
    Json(crate::registry::monitor().mod_views().await)
}

async fn handle_monitor_activity(Query(q): Query<ActivityQuery>) -> impl IntoResponse {
    Json(crate::registry::monitor().recent_activity(q.limit.unwrap_or(100).min(300)).await)
}

async fn handle_chat_recent(Query(q): Query<ActivityQuery>) -> impl IntoResponse {
    Json(crate::chat_log::recent(q.limit.unwrap_or(80).min(250)))
}

async fn handle_monitor_system() -> impl IntoResponse {
    Json(crate::sysmon::snapshot())
}

async fn handle_monitor_players() -> impl IntoResponse {
    // live online steamids from the bridge (authoritative); player_db for the full roster + metrics
    let mut online_steam = std::collections::HashSet::new();
    let mut online_count = 0u64;
    if let Ok(r) = crate::pipe_rpc::call("getOnlinePlayers", Some(serde_json::json!({}))).await {
        online_count = r.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        if let Some(arr) = r.get("players").and_then(|v| v.as_array()) {
            for p in arr {
                if let Some(s) = p.get("steamId").and_then(|v| v.as_str()) { online_steam.insert(s.to_string()); }
            }
        }
    }
    let players = crate::player_db::roster(&online_steam);
    let total = players.len() as u64;
    let online = if online_count > 0 { online_count } else { players.iter().filter(|p| p.online).count() as u64 };
    Json(serde_json::json!({
        "online_count": online,
        "total_count": total,
        "offline_count": total.saturating_sub(online),
        "players": players,
    }))
}

#[derive(Deserialize)]
struct ControlReq { name: String, state: String }

async fn handle_control_mod(Json(req): Json<ControlReq>) -> impl IntoResponse {
    let st = crate::registry::ModState::parse(&req.state);
    if let Some(name) = crate::registry::resolve_name(&req.name) {
        crate::registry::monitor().set_state(name, st).await;
        Json(serde_json::json!({ "ok": true, "name": req.name, "state": req.state }))
    } else {
        Json(serde_json::json!({ "ok": false, "error": "unknown mod" }))
    }
}

// --- Admin config surface (ServerSettings.ini + user-list .ini files) -------
// Read/write happens directly on disk (the service runs on the box). Gated by
// the same bearer auth as the rest of the protected router; the web adminmap
// proxies here behind its Discord allowlist. @dep admin_files.rs.

#[derive(Deserialize)]
struct AdminFileQuery { name: String }

#[derive(Deserialize)]
struct WriteAdminFileReq { name: String, contents: String }

#[derive(Deserialize)]
struct SkillEdit { name: String, level: Option<i64>, xp: Option<f64> }
#[derive(Deserialize)]
struct SetSkillReq { steam: String, skills: Vec<SkillEdit> }

#[derive(Deserialize)]
struct AttrEdit { name: String, value: f64 }
#[derive(Deserialize)]
struct SetAttrReq { steam: String, attributes: Vec<AttrEdit> }

#[derive(Deserialize)]
struct SaveSettingsReq { patch: std::collections::HashMap<String, String> }

async fn handle_get_admin_file(
    State(s): State<AppState>,
    Query(q): Query<AdminFileQuery>,
) -> impl IntoResponse {
    match crate::admin_files::read_admin_file(&s.cfg.scum_server_exe, &q.name) {
        Ok(contents) => Json(serde_json::json!({ "ok": true, "name": q.name, "contents": contents })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn handle_write_admin_file(
    State(s): State<AppState>,
    Json(req): Json<WriteAdminFileReq>,
) -> impl IntoResponse {
    match crate::admin_files::write_admin_file(&s.cfg.scum_server_exe, &req.name, &req.contents) {
        Ok(()) => Json(serde_json::json!({ "ok": true, "name": req.name, "pendingRestart": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// ---- TurdMOD data files (C:\TurdMOD\data\*.json) — backs the Vehicle Manager +
// Safe Zones TMM tabs. Whitelisted names only (no traversal); JSON-validated on write.
const DATA_DIR: &str = r"C:\TurdMOD\data";
const DATA_WHITELIST: &[&str] = &[
    "safe_zones.json", "vehicle_ownership.json", "repo_history.json", "vehicle_durability.json",
    "death_notes.json", "permissions.json",
];
fn data_path(name: &str) -> Option<std::path::PathBuf> {
    if !DATA_WHITELIST.contains(&name) { return None; }
    Some(std::path::Path::new(DATA_DIR).join(name))
}

async fn handle_vehicles_detail(State(s): State<AppState>) -> impl IntoResponse {
    match crate::scumdb::vehicles_detail(&s.cfg.scumdb_path) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct RepoVehicle { id: i64, vehicle: Option<String>, owner: Option<String> }
#[derive(Deserialize)]
struct RepoReq { vehicles: Vec<RepoVehicle> }

// Manual repossession: destroy each vehicle by id (live DestroyVehicle), log to the
// public repo_history.json (so the !repos chat cmd + Discord + History tab show it),
// and announce. @dep vehicle_repo::destroy_vehicle.
async fn handle_vehicles_repo(Json(req): Json<RepoReq>) -> impl IntoResponse {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut destroyed = 0usize;
    let mut failed = 0usize;
    let mut hist_add: Vec<serde_json::Value> = Vec::new();
    for v in &req.vehicles {
        if crate::vehicle_repo::destroy_vehicle(v.id).await {
            destroyed += 1;
            hist_add.push(serde_json::json!({
                "entity_id": v.id,
                "vehicle": v.vehicle.clone().unwrap_or_else(|| "vehicle".into()),
                "owner": v.owner.clone().unwrap_or_else(|| "?".into()),
                "at": now, "manual": true,
            }));
        } else {
            failed += 1;
        }
    }
    if !hist_add.is_empty() {
        let path = r"C:\TurdMOD\data\repo_history.json";
        let mut hist: Vec<serde_json::Value> = std::fs::read_to_string(path).ok()
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        hist.extend(hist_add);
        let n = hist.len();
        if n > 100 { hist.drain(0..n - 100); }
        if let Ok(j) = serde_json::to_string_pretty(&hist) { let _ = std::fs::write(path, j); }
        crate::auto_announce::announce(&format!("\u{1F697} {} vehicle(s) repossessed.", destroyed)).await;
    }
    Json(serde_json::json!({ "ok": true, "destroyed": destroyed, "failed": failed })).into_response()
}

async fn handle_get_data_file(Query(q): Query<AdminFileQuery>) -> impl IntoResponse {
    let Some(p) = data_path(&q.name) else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "not a whitelisted data file" }))).into_response();
    };
    let contents = std::fs::read_to_string(&p).unwrap_or_default(); // empty string if missing
    Json(serde_json::json!({ "ok": true, "name": q.name, "contents": contents })).into_response()
}

async fn handle_write_data_file(Json(req): Json<WriteAdminFileReq>) -> impl IntoResponse {
    let Some(p) = data_path(&req.name) else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "not a whitelisted data file" }))).into_response();
    };
    if serde_json::from_str::<serde_json::Value>(&req.contents).is_err() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "invalid JSON" }))).into_response();
    }
    let _ = std::fs::create_dir_all(DATA_DIR);
    match std::fs::write(&p, &req.contents) {
        Ok(()) => Json(serde_json::json!({ "ok": true, "name": req.name })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// Permissions — read/write the SAME live SharedPerms the in-game `!perm` mutates,
// so a TMM edit takes effect immediately (no restart) and stays consistent with
// in-game changes. `mods` is the full command/feature catalog (incl. the adminless
// admin commands); `players` carries each player's tier + per-command overrides.
async fn handle_get_permissions(State(s): State<AppState>) -> impl IntoResponse {
    let perms = s.perms.read().await;
    Json(serde_json::json!({
        "ok": true,
        "players": perms.players,
        "mods": perms.mods,
        "tiers": crate::permissions::tier_names(),
    }))
}

async fn handle_set_permissions(
    State(s): State<AppState>,
    Json(body): Json<crate::permissions::PermState>,
) -> impl IntoResponse {
    {
        let mut perms = s.perms.write().await;
        *perms = body;
        crate::permissions::save_state(&perms);
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

// Save-edit a player's skills (level/xp) directly in SCUM.db. `#SetSkillLevel` is
// SCUM dev-gated ("not authorized" even for the owner), so the only reliable path is
// the save. SAFE CYCLE: stop SCUM (kills + confirms dead → DB closed) → back up scum.db
// to our external checkpoint dir → write → restart (only if it was running). Never a
// forced wipe; every edit is preceded by a fresh, restorable backup.
async fn handle_set_skill(State(s): State<AppState>, Json(req): Json<SetSkillReq>) -> impl IntoResponse {
    if req.steam.len() != 17 || !req.steam.chars().all(|c| c.is_ascii_digit()) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "valid SteamID64 required" }))).into_response();
    }
    if req.skills.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "no skill edits provided" }))).into_response();
    }
    let was_running = { s.engine.lock().unwrap().running };

    if was_running {
        if let Err(e) = engine::stop_server(Arc::clone(&s.engine)).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": format!("stop: {}", e) }))).into_response();
        }
        // brief settle so file handles release after the confirmed-dead kill
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Always back up scum.db to the external checkpoint dir BEFORE writing.
    let backup = crate::checkpoint::do_checkpoint().ok().map(|(ts, _)| ts);

    let edits: Vec<(String, Option<i64>, Option<f64>)> =
        req.skills.iter().map(|e| (e.name.clone(), e.level, e.xp)).collect();
    let write_res = crate::scumdb::set_skills(&s.cfg.scumdb_path, &req.steam, &edits);

    // Bring the server back if it was running, regardless of write outcome.
    let mut restarted = false;
    if was_running {
        match engine::start_server(&s.cfg, Arc::clone(&s.engine)).await {
            Ok(_) => restarted = true,
            Err(e) => tracing::error!("[set-skill] restart failed: {}", e),
        }
    }

    match write_res {
        Ok(updated) => Json(serde_json::json!({
            "ok": true, "updated": updated, "backup": backup, "restarted": restarted
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "ok": false, "error": e.to_string(), "backup": backup, "restarted": restarted
        }))).into_response(),
    }
}

// Save-edit a player's base attributes (Str/Con/Dex/Int) in the body_simulation blob.
// #SetAttributes is dev-gated, so the save is the only path. Same SAFE CYCLE as skills:
// stop SCUM → back up scum.db externally → write the blob → restart (only if it was running).
async fn handle_set_attribute(State(s): State<AppState>, Json(req): Json<SetAttrReq>) -> impl IntoResponse {
    if req.steam.len() != 17 || !req.steam.chars().all(|c| c.is_ascii_digit()) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "valid SteamID64 required" }))).into_response();
    }
    if req.attributes.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "no attribute edits provided" }))).into_response();
    }
    let was_running = { s.engine.lock().unwrap().running };

    if was_running {
        if let Err(e) = engine::stop_server(Arc::clone(&s.engine)).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": format!("stop: {}", e) }))).into_response();
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    let backup = crate::checkpoint::do_checkpoint().ok().map(|(ts, _)| ts);

    let edits: Vec<(String, f64)> = req.attributes.iter().map(|e| (e.name.clone(), e.value)).collect();
    let write_res = crate::scumdb::set_attributes(&s.cfg.scumdb_path, &req.steam, &edits);

    let mut restarted = false;
    if was_running {
        match engine::start_server(&s.cfg, Arc::clone(&s.engine)).await {
            Ok(_) => restarted = true,
            Err(e) => tracing::error!("[set-attribute] restart failed: {}", e),
        }
    }

    match write_res {
        Ok(written) => Json(serde_json::json!({
            "ok": true, "updated": written, "backup": backup, "restarted": restarted
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "ok": false, "error": e.to_string(), "backup": backup, "restarted": restarted
        }))).into_response(),
    }
}

async fn handle_get_server_settings(State(s): State<AppState>) -> impl IntoResponse {
    match crate::admin_files::read_admin_file(&s.cfg.scum_server_exe, "ServerSettings.ini") {
        Ok(raw) => Json(serde_json::json!({
            "ok": true,
            "values": crate::admin_files::parse_all_scum(&raw),
            "rawIni": raw,
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn handle_save_server_settings(
    State(s): State<AppState>,
    Json(req): Json<SaveSettingsReq>,
) -> impl IntoResponse {
    let res = (|| -> anyhow::Result<()> {
        let raw = crate::admin_files::read_admin_file(&s.cfg.scum_server_exe, "ServerSettings.ini")?;
        let updated = crate::admin_files::apply_scum_patch(&raw, &req.patch);
        crate::admin_files::write_admin_file(&s.cfg.scum_server_exe, "ServerSettings.ini", &updated)
    })();
    match res {
        Ok(()) => Json(serde_json::json!({ "ok": true, "pendingRestart": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn handle_health(State(s): State<AppState>) -> impl IntoResponse {
    Json(HealthResp {
        service: "turdmod-service",
        version: env!("CARGO_PKG_VERSION"),
        build: env!("GIT_SHA"),
        uptime_secs: s.service_start.elapsed().as_secs(),
    })
}

async fn handle_map_players(State(s): State<AppState>) -> impl IntoResponse {
    let snap = s.snapshot.read().await;
    Json(serde_json::json!({
        "players": snap.players,
        "updatedAt": snap.updated_at,
        "count": snap.players.len()
    }))
}

async fn handle_status(State(s): State<AppState>) -> impl IntoResponse {
    // Read BattlEye state from the (possibly-staged) on-disk args, not the lock.
    let battleye = !s.cfg.current_args().iter().any(|a| a == "-NoBattlEye");
    let st = s.engine.lock().unwrap();
    Json(StatusResp {
        server_running: st.running,
        pid: st.pid,
        uptime_secs: st.uptime_secs(),
        restarts: st.restarts,
        build: env!("GIT_SHA"),
        battleye,
    })
}

// Toggle BattlEye for the NEXT launch by rewriting ONLY scum_server_args in
// service.json (add/remove -NoBattlEye), preserving the rest of the file. Applies
// on the next /server/start|restart (launch re-reads via Config::current_args).
// enabled=true => secure server (no -NoBattlEye); enabled=false => -NoBattlEye set.
async fn handle_set_battleye(
    State(s): State<AppState>,
    Json(req): Json<BattlEyeReq>,
) -> impl IntoResponse {
    let path = crate::config::config_path(&s.cfg.instance_id);
    let res = (|| -> anyhow::Result<Vec<String>> {
        let raw = std::fs::read_to_string(&path)?;
        let mut v: serde_json::Value = serde_json::from_str(&raw)?;
        let arr = v
            .get_mut("scum_server_args")
            .and_then(|a| a.as_array_mut())
            .ok_or_else(|| anyhow::anyhow!("scum_server_args is not an array"))?;
        arr.retain(|a| a.as_str() != Some("-NoBattlEye"));
        if !req.enabled {
            arr.push(serde_json::json!("-NoBattlEye"));
        }
        let new_args: Vec<String> = arr.iter().filter_map(|a| a.as_str().map(String::from)).collect();
        std::fs::write(&path, serde_json::to_string_pretty(&v)?)?;
        Ok(new_args)
    })();
    match res {
        Ok(args) => Json(serde_json::json!({
            "ok": true, "battleye": req.enabled, "pendingRestart": true, "args": args
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ScheduleReq {
    enabled: Option<bool>,
    interval_hours: Option<f64>,
    warn_secs: Option<u64>,
    messages: Option<Vec<crate::schedule::WarnBanner>>,
}

// --- Loot customization files (Loot/{Nodes,Items}/{Default,Override}/*.json) ---
// Read Default or Override; write Override only (never clobber the Default export). Path-scoped to
// the Loot dir, .json only, no traversal. @dep loot reload via runAdminCommand (LootPanel).
fn loot_dir(scum_server_exe: &str) -> Option<std::path::PathBuf> {
    let scum = std::path::Path::new(scum_server_exe).parent()?.parent()?.parent()?; // Win64->Binaries->SCUM
    Some(scum.join("Saved").join("Config").join("WindowsServer").join("Loot"))
}

fn safe_loot_rel(rel: &str) -> Option<String> {
    let c = rel.replace('\\', "/");
    if c.contains("..") || c.starts_with('/') || !c.to_lowercase().ends_with(".json") { return None; }
    if !(c.starts_with("Nodes/") || c.starts_with("Items/")) { return None; }
    Some(c)
}

async fn handle_loot_files(State(s): State<AppState>) -> impl IntoResponse {
    let dir = match loot_dir(&s.cfg.scum_server_exe) {
        Some(d) => d,
        None => return Json(serde_json::json!({ "ok": false, "error": "bad exe path" })).into_response(),
    };
    let mut files: Vec<String> = Vec::new();
    for sub in ["Nodes/Default", "Nodes/Override", "Items/Default", "Items/Override"] {
        let p = dir.join(sub.replace('/', "\\"));
        if let Ok(rd) = std::fs::read_dir(&p) {
            for e in rd.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if name.to_lowercase().ends_with(".json") {
                        files.push(format!("{}/{}", sub, name));
                    }
                }
            }
        }
    }
    files.sort();
    Json(serde_json::json!({ "ok": true, "files": files })).into_response()
}

#[derive(Deserialize)]
struct LootFileQuery { path: String }

async fn handle_loot_file_get(State(s): State<AppState>, Query(q): Query<LootFileQuery>) -> impl IntoResponse {
    let rel = match safe_loot_rel(&q.path) {
        Some(r) => r,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "invalid loot path" }))).into_response(),
    };
    let full = loot_dir(&s.cfg.scum_server_exe).unwrap().join(rel.replace('/', "\\"));
    match std::fs::read_to_string(&full) {
        Ok(c) => Json(serde_json::json!({ "ok": true, "path": q.path, "contents": c })).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct LootFileSet { path: String, contents: String }

async fn handle_loot_file_set(State(s): State<AppState>, Json(req): Json<LootFileSet>) -> impl IntoResponse {
    let rel = match safe_loot_rel(&req.path) {
        Some(r) => r,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "invalid loot path" }))).into_response(),
    };
    if !rel.contains("/Override/") {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "can only write Override files" }))).into_response();
    }
    if serde_json::from_str::<serde_json::Value>(&req.contents).is_err() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "contents is not valid JSON" }))).into_response();
    }
    let full = loot_dir(&s.cfg.scum_server_exe).unwrap().join(rel.replace('/', "\\"));
    if let Some(parent) = full.parent() { let _ = std::fs::create_dir_all(parent); }
    match std::fs::write(&full, req.contents.as_bytes()) {
        Ok(()) => Json(serde_json::json!({ "ok": true, "path": req.path })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// Retained "snapshot now" — a never-rolled-off archive of the live SCUM.db (+ companions + data).
#[derive(Deserialize)]
struct ArchiveQuery { reason: Option<String> }

async fn handle_backup_archive(Query(q): Query<ArchiveQuery>) -> impl IntoResponse {
    let raw = q.reason.unwrap_or_else(|| "manual".into());
    let reason: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(48).collect();
    let reason = if reason.is_empty() { "manual".to_string() } else { reason };
    match crate::checkpoint::do_archive_live(&reason) {
        Ok(path) => Json(serde_json::json!({ "ok": true, "archived": path })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
    }
}

// Schema-break recovery: re-apply player skills+attributes+fame+money from a pre-wipe snapshot DB
// into the live (post-wipe) DB. SAFE CYCLE: stop SCUM -> retained backup of the live DB -> migrate ->
// restart (if it was running). Players must have rejoined the new world (have a prisoner) to
// receive their old progression. Fame = user_profile.fame_points; money = bank cash + gold restored
// into the player's bank account (created if absent). Vehicles are NOT auto-spawned — a manifest
// (class+owner+location) is saved to C:\TurdMOD\data\vehicle-manifest-<ts>.json + returned, for
// manual spawn / a future spawner. Structures/inventory are not handled. @dep scumdb::migrate_progression.
#[derive(Deserialize)]
struct MigrateReq { source: String }

async fn handle_migrate(State(s): State<AppState>, Json(req): Json<MigrateReq>) -> impl IntoResponse {
    if !std::path::Path::new(&req.source).exists() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": format!("source snapshot not found: {}", req.source) }))).into_response();
    }
    let was_running = { s.engine.lock().unwrap().running };
    if was_running {
        if let Err(e) = engine::stop_server(Arc::clone(&s.engine)).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": format!("stop: {}", e) }))).into_response();
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let backup = crate::checkpoint::do_archive_live("premigrate").ok();
    let result = crate::scumdb::migrate_progression(&req.source, &s.cfg.scumdb_path);
    let mut restarted = false;
    if was_running {
        match engine::start_server(&s.cfg, Arc::clone(&s.engine)).await {
            Ok(_) => restarted = true,
            Err(e) => tracing::error!("[migrate] restart failed: {}", e),
        }
    }
    match result {
        Ok(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("backup".into(), serde_json::json!(backup));
                obj.insert("restarted".into(), serde_json::json!(restarted));
            }
            Json(v).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "ok": false, "error": e.to_string(), "backup": backup, "restarted": restarted
        }))).into_response(),
    }
}

// --- Post-wipe restore campaign: auto restore-on-reconnect over a tapered restart cadence. ---
// Arm with the pre-wipe snapshot right after a wipe; each restart's stop-window restores any
// newly-reconnected players once. @dep restore_campaign.rs, schedule.rs (cadence taper).
async fn handle_campaign_status(State(s): State<AppState>) -> impl IntoResponse {
    Json(crate::restore_campaign::status_json(&s.cfg.instance_id)).into_response()
}

#[derive(Deserialize)]
struct CampaignArmReq { snapshot: String }

async fn handle_campaign_arm(State(s): State<AppState>, Json(req): Json<CampaignArmReq>) -> impl IntoResponse {
    match crate::restore_campaign::arm(&s.cfg.instance_id, &req.snapshot) {
        Ok(_) => Json(crate::restore_campaign::status_json(&s.cfg.instance_id)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
    }
}

async fn handle_campaign_disarm(State(s): State<AppState>) -> impl IntoResponse {
    crate::restore_campaign::disarm(&s.cfg.instance_id, "manual");
    Json(crate::restore_campaign::status_json(&s.cfg.instance_id)).into_response()
}

// --- Elevated (dev-command) users: SCUM.db `elevated_users`, turdmod-managed + queued ---
// effective = active in SCUM.db now; managed = our desired set; pending_* = applies next restart.
fn elevated_state_json(scumdb_path: &str) -> serde_json::Value {
    let effective = crate::scumdb::list_elevated(scumdb_path).unwrap_or_default();
    let cfg = crate::elevated::load();
    let pending_add: Vec<String> = cfg.add.iter().filter(|id| !effective.contains(id)).cloned().collect();
    let pending_remove: Vec<String> = cfg.remove.iter().filter(|id| effective.contains(id)).cloned().collect();
    serde_json::json!({
        "ok": true,
        "effective": effective,
        "managed": cfg.add,
        "pending_add": pending_add,
        "pending_remove": pending_remove,
    })
}

async fn handle_get_elevated(State(s): State<AppState>) -> impl IntoResponse {
    Json(elevated_state_json(&s.cfg.scumdb_path))
}

#[derive(Deserialize)]
struct ElevatedReq {
    add: Option<Vec<String>>,
    remove: Option<Vec<String>>,
}

async fn handle_set_elevated(State(s): State<AppState>, Json(req): Json<ElevatedReq>) -> impl IntoResponse {
    for id in req.add.unwrap_or_default() {
        if crate::elevated::valid_steam(&id) { crate::elevated::add_user(&id); }
    }
    for id in req.remove.unwrap_or_default() {
        if crate::elevated::valid_steam(&id) { crate::elevated::remove_user(&id); }
    }
    Json(elevated_state_json(&s.cfg.scumdb_path))
}

fn schedule_json(s: &crate::schedule::RestartSchedule) -> serde_json::Value {
    serde_json::json!({
        "ok": true,
        "enabled": s.enabled,
        "interval_hours": s.interval_hours,
        "warn_secs": s.warn_secs,
        "last_restart_at": s.last_restart_at,
        "next_restart_at": s.next_restart_at(),
        "now": crate::schedule::now_epoch(),
        "messages": serde_json::to_value(&s.messages).unwrap_or_default(),
    })
}

async fn handle_get_schedule(State(s): State<AppState>) -> impl IntoResponse {
    Json(schedule_json(&crate::schedule::load(&s.cfg.instance_id)))
}

// Set the recurring restart schedule. (Re)enabling or changing the cadence re-anchors to
// now (so re-enabling never fires immediately); editing only the warning text/lead does not
// reset "next restart".
async fn handle_set_schedule(
    State(s): State<AppState>,
    Json(req): Json<ScheduleReq>,
) -> impl IntoResponse {
    let mut sched = crate::schedule::load(&s.cfg.instance_id);
    let was_enabled = sched.enabled;
    let prev_interval = sched.interval_hours;
    if let Some(e) = req.enabled {
        sched.enabled = e;
    }
    if let Some(i) = req.interval_hours {
        sched.interval_hours = i;
    }
    if let Some(w) = req.warn_secs {
        sched.warn_secs = w;
    }
    if let Some(m) = req.messages {
        sched.messages = m;
    }
    if sched.enabled
        && (!was_enabled || sched.interval_hours != prev_interval || sched.last_restart_at.is_none())
    {
        sched.last_restart_at = Some(crate::schedule::now_epoch());
    }
    match crate::schedule::save(&s.cfg.instance_id, &sched) {
        Ok(()) => Json(schedule_json(&sched)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_start(State(s): State<AppState>) -> impl IntoResponse {
    match engine::start_server(&s.cfg, Arc::clone(&s.engine)).await {
        Ok(pid) => (StatusCode::OK, Json(serde_json::json!({ "pid": pid }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

async fn handle_stop(State(s): State<AppState>) -> impl IntoResponse {
    match engine::stop_server(Arc::clone(&s.engine)).await {
        Ok(()) => Json(OkResp { ok: true }).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

#[derive(Deserialize)]
struct RestartQuery { countdown: Option<u64> }

async fn broadcast_text(msg: &str) {
    pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": msg }))).await.ok();
}

// Pre-restart countdown. Two layers:
//  1. Escalating COLORED center banners at the minute marks (5/4/3/2/~60s), written to
//     Notifications.json (re-read live by the manager — see restart_banner.rs). Color
//     ramps yellow -> red as the restart nears.
//  2. The existing bottom-left chat countdown at the second marks (30/15/10/5s).
// Restores the real Notifications.json rotation before returning (before the restart).
// Used by /server/restart?countdown=N. Banner timing is ~30-60s granular (manager cycle),
// hence the "~" in the labels — accepted by design.
async fn restart_countdown(cfg: &Config, secs: u64) {
    // Warning banners are admin-editable (manager Restart Schedule panel) — load them live.
    let mut banners = crate::schedule::load(&cfg.instance_id).messages;
    banners.sort_by(|a, b| b.at_secs.cmp(&a.at_secs)); // fire furthest-out mark first
    let chat_marks: [u64; 4] = [30, 15, 10, 5];
    let mut backup: Option<String> = None;
    let mut remaining = secs;

    broadcast_text(&format!(
        "[ScummyMap] \u{26A0} Server restart in {}s — exit vehicles & get to safety!", remaining
    )).await;

    // Layer 1: escalating colored center banners at the configured marks.
    for b in banners.iter() {
        if b.at_secs >= remaining { continue; }
        tokio::time::sleep(std::time::Duration::from_secs(remaining - b.at_secs)).await;
        remaining = b.at_secs;
        // First write returns the real rotation -> keep it for restore.
        let prev = crate::restart_banner::write_banner(cfg, &b.text, &b.color, 45);
        if backup.is_none() { backup = prev; }
    }

    // Layer 2: bottom-left chat at the second marks.
    for &m in chat_marks.iter() {
        if m >= remaining { continue; }
        tokio::time::sleep(std::time::Duration::from_secs(remaining - m)).await;
        remaining = m;
        broadcast_text(&format!("[ScummyMap] \u{26A0} Restart in {}s\u{2026}", remaining)).await;
    }
    if remaining > 0 {
        tokio::time::sleep(std::time::Duration::from_secs(remaining)).await;
    }

    // Restore the real Notifications.json so the post-restart boot has the normal rotation.
    if let Some(b) = backup { crate::restart_banner::restore(cfg, &b); }
}

async fn handle_restart(State(s): State<AppState>, Query(q): Query<RestartQuery>) -> impl IntoResponse {
    // Countdown path: DETACH the warning sequence + restart so they survive the caller dropping
    // the connection. @brk: the OVH TurdMODRestart task POSTs with a 60s client timeout, but the
    // countdown is 600s — blocking here meant the client timed out at 60s, axum cancelled this
    // handler future, and the in-flight countdown (+ the actual stop/start) was killed before the
    // 300s banner mark ever hit. Result: scary broadcast, no banners, no restart (every fire logged
    // "FAILED: timed out"). Spawning + returning immediately matches this endpoint's documented
    // contract and lets the full 5→1min banner ramp + restart run to completion.
    if let Some(cd) = q.countdown.filter(|&c| c > 0) {
        let cd = cd.min(600);
        let cfg = Arc::clone(&s.cfg);
        let engine = Arc::clone(&s.engine);
        tokio::spawn(async move {
            restart_countdown(&cfg, cd).await;
            if let Err(e) = engine::stop_server(Arc::clone(&engine)).await {
                tracing::error!("[restart] stop failed: {}", e);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            // Post-wipe restore campaign: while SCUM is stopped, restore any newly-reconnected
            // players. No-op unless a campaign is armed. Never blocks the restart.
            crate::restore_campaign::run_in_stop_window(&cfg).await;
            match engine::start_server(&cfg, Arc::clone(&engine)).await {
                Ok(pid) => tracing::info!("[restart] back up after {}s countdown, pid {}", cd, pid),
                Err(e) => tracing::error!("[restart] start failed: {}", e),
            }
        });
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "scheduled": true, "countdown": cd })),
        ).into_response();
    }

    // Immediate restart (countdown=0 / unset): block + return the new pid as before.
    if let Err(e) = engine::stop_server(Arc::clone(&s.engine)).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("stop: {}", e) })),
        ).into_response();
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    crate::restore_campaign::run_in_stop_window(&s.cfg).await;
    match engine::start_server(&s.cfg, Arc::clone(&s.engine)).await {
        Ok(pid) => Json(serde_json::json!({ "pid": pid })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("start: {}", e) })),
        ).into_response(),
    }
}

async fn handle_logs(
    State(s): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> impl IntoResponse {
    let n = q.lines.unwrap_or(100).min(5000);
    let lines = engine::tail_log(&s.cfg, n);
    Json(serde_json::json!({ "lines": lines }))
}

async fn handle_engine_rpc(Json(req): Json<RpcReq>) -> impl IntoResponse {
    match pipe_rpc::call(&req.method, req.params).await {
        Ok(result) => Json(serde_json::json!({ "result": result })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

// Discord /link slash command calls this to mint a one-time link code for a
// Discord user; the player then redeems it in-game with !link <code>.
async fn handle_links_issue(Json(req): Json<serde_json::Value>) -> impl IntoResponse {
    let did = req.get("discord_id").and_then(|v| v.as_str()).unwrap_or("");
    let name = req.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if did.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "discord_id required" }))).into_response();
    }
    let code = crate::discord_verify::issue(did, name);
    Json(serde_json::json!({ "code": code })).into_response()
}

async fn handle_ollama_generate(Json(req): Json<ollama::PromptReq>) -> impl IntoResponse {
    match ollama::generate(&req).await {
        Ok(resp) => Json(serde_json::json!({ "model": resp.model, "response": resp.response })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

// ─── scumpilot (AI admin) control proxy ──────────────────────────────────────
// Forwards to scumpilot-runner's localhost HTTP control endpoint, injecting its
// token. Lets the manager / any service client pause/resume/status the AI brain
// without exposing scumpilot's port or distributing a second token.
async fn scumpilot_forward(method: reqwest::Method, path: &str) -> axum::response::Response {
    let port = std::env::var("SCUMPILOT_HTTP_PORT").unwrap_or_default();
    let stoken = std::env::var("SCUMPILOT_HTTP_TOKEN").unwrap_or_default();
    if port.is_empty() || stoken.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error":"scumpilot control not configured (set SCUMPILOT_HTTP_PORT + SCUMPILOT_HTTP_TOKEN)"}))).into_response();
    }
    let url = format!("http://127.0.0.1:{}{}", port, path);
    match reqwest::Client::new().request(method, &url).bearer_auth(&stoken).send().await {
        Ok(resp) => {
            let code = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = resp.text().await.unwrap_or_default();
            (code, [(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error":format!("scumpilot unreachable: {}", e)}))).into_response(),
    }
}
async fn handle_scumpilot_status() -> impl IntoResponse { scumpilot_forward(reqwest::Method::GET, "/status").await }
async fn handle_scumpilot_pause() -> impl IntoResponse { scumpilot_forward(reqwest::Method::POST, "/pause").await }
async fn handle_scumpilot_resume() -> impl IntoResponse { scumpilot_forward(reqwest::Method::POST, "/resume").await }

async fn handle_rcon(Json(req): Json<serde_json::Value>) -> impl IntoResponse {
    let command = req.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"command required"}))).into_response();
    }
    let Some((host, port, pass)) = crate::rcon::get_rcon_config() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":"RCON not configured — create C:\\TurdMOD\\data\\rcon.json"}))).into_response();
    };
    match crate::rcon::exec(&host, port, &pass, command).await {
        Ok(response) => Json(serde_json::json!({"ok":true,"command":command,"response":response})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response(),
    }
}

async fn handle_service_activity() -> impl IntoResponse {
    let mut activity = serde_json::json!({});

    // Read economy state
    if let Ok(data) = std::fs::read_to_string(r"C:\TurdMOD\data\economy.json") {
        if let Ok(econ) = serde_json::from_str::<serde_json::Value>(&data) {
            let player_count = econ["players"].as_object().map(|p| p.len()).unwrap_or(0);
            activity["economy"] = serde_json::json!({"players": player_count});
        }
    }

    // Read vehicle ownership
    if let Ok(data) = std::fs::read_to_string(r"C:\TurdMOD\data\vehicle_ownership.json") {
        if let Ok(own) = serde_json::from_str::<serde_json::Value>(&data) {
            let count = own["vehicles"].as_array().map(|a| a.len()).unwrap_or(0);
            let insured = own["vehicles"].as_array()
                .map(|a| a.iter().filter(|v| v["insured"].as_bool().unwrap_or(false)).count())
                .unwrap_or(0);
            activity["vehicles"] = serde_json::json!({"registered": count, "insured": insured});
        }
    }

    // Read leaderboard
    if let Ok(data) = std::fs::read_to_string(r"C:\TurdMOD\data\leaderboard.json") {
        if let Ok(lb) = serde_json::from_str::<serde_json::Value>(&data) {
            let players = lb["players"].as_object().map(|p| p.len()).unwrap_or(0);
            activity["leaderboard"] = serde_json::json!({"tracked_players": players});
        }
    }

    // Read reputation
    if let Ok(data) = std::fs::read_to_string(r"C:\TurdMOD\data\reputation.json") {
        if let Ok(rep) = serde_json::from_str::<serde_json::Value>(&data) {
            let players = rep["players"].as_object().map(|p| p.len()).unwrap_or(0);
            activity["reputation"] = serde_json::json!({"tracked_players": players});
        }
    }

    // List all data files
    if let Ok(entries) = std::fs::read_dir(r"C:\TurdMOD\data") {
        let files: Vec<String> = entries.flatten()
            .filter(|e| e.path().is_file())
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                format!("{} ({}b)", name, size)
            }).collect();
        activity["data_files"] = serde_json::json!(files);
    }

    Json(activity)
}

async fn handle_ollama_models() -> impl IntoResponse {
    match ollama::list_models().await {
        Ok(data) => Json(data).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

// ─── SCUM.db endpoints ──────────────────────────────────────────────────────

async fn handle_scumdb_tables(State(s): State<AppState>) -> impl IntoResponse {
    let path = &s.cfg.scumdb_path;
    match scumdb::list_tables(path) {
        Ok(tables) => Json(serde_json::json!({ "tables": tables, "count": tables.len() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct SchemaQuery { table: String }

async fn handle_scumdb_schema(
    State(s): State<AppState>,
    Query(q): Query<SchemaQuery>,
) -> impl IntoResponse {
    let path = &s.cfg.scumdb_path;
    match scumdb::table_schema(path, &q.table) {
        Ok(cols) => Json(serde_json::json!({ "table": q.table, "columns": cols })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct RowsQuery {
    table: String,
    #[serde(default = "default_rows_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}
fn default_rows_limit() -> usize { 100 }

async fn handle_scumdb_rows(
    State(s): State<AppState>,
    Query(q): Query<RowsQuery>,
) -> impl IntoResponse {
    match scumdb::query_rows(&s.cfg.scumdb_path, &q.table, q.limit, q.offset) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn handle_scumdb_traders(State(s): State<AppState>) -> impl IntoResponse {
    let path = &s.cfg.scumdb_path;
    match scumdb::query_traders(path) {
        Ok(rows) => Json(serde_json::json!({ "traders": rows, "count": rows.len() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct SteamQuery { steam: String }

async fn handle_player_card(State(s): State<AppState>, Query(q): Query<SteamQuery>) -> impl IntoResponse {
    match scumdb::player_card(&s.cfg.scumdb_path, &q.steam) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn handle_player_inventory(State(s): State<AppState>, Query(q): Query<SteamQuery>) -> impl IntoResponse {
    match scumdb::player_inventory(&s.cfg.scumdb_path, &q.steam) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

// Batch last-known positions for every player with a spawned prisoner — one DB
// query, backs the map's last-known-location layer + dossier (offline players).
async fn handle_last_positions(State(s): State<AppState>) -> impl IntoResponse {
    match scumdb::last_positions(&s.cfg.scumdb_path) {
        Ok(v) => Json(serde_json::json!({ "players": v })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

// Last SAVED position for a player — crash-proof (scum.db, no bridge poll). Backs
// the map's last-known-location layer + the player dossier.
async fn handle_last_position(State(s): State<AppState>, Query(q): Query<SteamQuery>) -> impl IntoResponse {
    match scumdb::player_last_position(&s.cfg.scumdb_path, &q.steam) {
        Ok(Some((x, y, z))) => Json(serde_json::json!({
            "found": true, "x": x, "y": y, "z": z, "sector": scumdb::sector_of(x, y),
        })).into_response(),
        Ok(None) => Json(serde_json::json!({ "found": false })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn handle_map_vehicles(State(s): State<AppState>) -> impl IntoResponse {
    match scumdb::map_vehicles(&s.cfg.scumdb_path) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn handle_map_zones(State(s): State<AppState>) -> impl IntoResponse {
    match scumdb::map_zones(&s.cfg.scumdb_path) {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct EntityQuery {
    class: String,
    #[serde(default = "default_entity_limit")]
    limit: usize,
}
fn default_entity_limit() -> usize { 50 }

async fn handle_scumdb_entities(
    State(s): State<AppState>,
    Query(q): Query<EntityQuery>,
) -> impl IntoResponse {
    let path = &s.cfg.scumdb_path;
    match scumdb::query_entities(path, &q.class, q.limit) {
        Ok(rows) => Json(serde_json::json!({ "entities": rows, "count": rows.len(), "pattern": q.class })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

// ─── scummymap push ─────────────────────────────────────────────────────────

async fn handle_scummap_push_traders(State(s): State<AppState>) -> impl IntoResponse {
    let Some(ref api_url) = s.cfg.scummap_api_url else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "scummap_api_url not configured in service.json" }))).into_response();
    };
    let Some(ref api_token) = s.cfg.scummap_api_token else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "scummap_api_token not configured in service.json" }))).into_response();
    };
    let Some(ref map_id) = s.cfg.scummap_map_id else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "scummap_map_id not configured in service.json" }))).into_response();
    };

    let traders = match scumdb::query_traders(&s.cfg.scumdb_path) {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("scumdb: {}", e) }))).into_response(),
    };

    let zones = scummap_push::entities_to_zones(&traders, 35000);
    match scummap_push::push_trader_zones(api_url, api_token, map_id, &zones).await {
        Ok(n) => Json(serde_json::json!({
            "pushed": n,
            "zones": zones.iter().map(|z| serde_json::json!({
                "id": z.id, "name": z.name,
                "center": { "x": z.center.x, "y": z.center.y },
                "radiusCm": z.radius_cm,
            })).collect::<Vec<_>>()
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}
