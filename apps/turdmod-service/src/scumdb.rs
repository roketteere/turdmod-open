// SCUM.db query module — read-only SQLite access to the game state DB.
// Path configurable via Config; default from checkpoint.rs constant.
// Safe for concurrent reads while SCUMServer is running (SQLite WAL mode).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_DB_PATH: &str = r"C:\SCUMServer\SCUM\Saved\SaveFiles\SCUM.db";

// Resolved SCUM.db path, set ONCE at startup from Config.scumdb_path. Mods that call
// db_path(None) (e.g. chat_cmds for profile/lock lookups) don't carry the Config, so without
// this they'd hit the hardcoded DEFAULT — wrong whenever the real DB isn't at C:\SCUMServer
// (e.g. the local Steam-path server). @brk: must be set in main before any mod runs.
static RESOLVED_DB: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Set the process-wide SCUM.db path (from Config.scumdb_path). Call once at startup.
pub fn set_db_path(p: &str) { let _ = RESOLVED_DB.set(p.to_string()); }

#[derive(Debug, Serialize)]
pub struct TableColumn {
    pub cid: i32,
    pub name: String,
    pub col_type: String,
    pub notnull: bool,
    pub pk: bool,
}

#[derive(Debug, Serialize)]
pub struct EntityRow {
    pub id: i64,
    pub class: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn db_path(cfg_path: Option<&str>) -> &str {
    cfg_path.unwrap_or_else(|| RESOLVED_DB.get().map(|s| s.as_str()).unwrap_or(DEFAULT_DB_PATH))
}

fn open_readonly(path: &str) -> Result<rusqlite::Connection> {
    let p = Path::new(path);
    anyhow::ensure!(p.exists(), "SCUM.db not found at {}", path);
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open SCUM.db at {}", path))?;
    Ok(conn)
}

pub fn list_tables(path: &str) -> Result<Vec<String>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names)
}

pub fn table_schema(path: &str, table: &str) -> Result<Vec<TableColumn>> {
    let conn = open_readonly(path)?;
    let safe_table = table.replace(['\'', '"', ';', '-'], "");
    let sql = format!("PRAGMA table_info('{}')", safe_table);
    let mut stmt = conn.prepare(&sql)?;
    let cols: Vec<TableColumn> = stmt
        .query_map([], |row| {
            Ok(TableColumn {
                cid: row.get(0)?,
                name: row.get(1)?,
                col_type: row.get(2)?,
                notnull: row.get::<_, i32>(3)? != 0,
                pk: row.get::<_, i32>(5)? != 0,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(cols)
}

// Generic table-rows reader → JSON. Powers the manager's SCUM.db viewer ("see ALL").
// Returns {table, columns, rows:[{col:val}], total, limit, offset}. Read-only, WAL-safe.
// @inv table name sanitized to alnum/_ before interpolation — no injection surface.
pub fn query_rows(path: &str, table: &str, limit: usize, offset: usize) -> Result<serde_json::Value> {
    let conn = open_readonly(path)?;
    let safe: String = table.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
    if safe.is_empty() {
        return Ok(serde_json::json!({ "error": "invalid table" }));
    }
    let limit = limit.clamp(1, 1000);
    let total: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", safe), [], |r| r.get(0))
        .unwrap_or(0);
    let sql = format!("SELECT * FROM \"{}\" LIMIT {} OFFSET {}", safe, limit, offset);
    let mut stmt = conn.prepare(&sql)?;
    let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let ncol = col_names.len();
    let mut out_rows: Vec<serde_json::Value> = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut obj = serde_json::Map::new();
        for i in 0..ncol {
            let v: serde_json::Value = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                rusqlite::types::ValueRef::Integer(n) => serde_json::Value::from(n),
                rusqlite::types::ValueRef::Real(f) => serde_json::Value::from(f),
                rusqlite::types::ValueRef::Text(t) => {
                    serde_json::Value::from(String::from_utf8_lossy(t).to_string())
                }
                rusqlite::types::ValueRef::Blob(b) => {
                    serde_json::Value::from(format!("[blob {} bytes]", b.len()))
                }
            };
            obj.insert(col_names[i].clone(), v);
        }
        out_rows.push(serde_json::Value::Object(obj));
    }
    Ok(serde_json::json!({
        "table": safe, "columns": col_names, "rows": out_rows,
        "total": total, "limit": limit, "offset": offset
    }))
}

// @ctx: confirmed SCUM.db column names via PRAGMA: class, location_x, location_y, location_z
pub fn query_entities(path: &str, class_pattern: &str, limit: usize) -> Result<Vec<EntityRow>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare(
        "SELECT id, class, location_x, location_y, location_z \
         FROM entity WHERE class LIKE ?1 LIMIT ?2"
    )?;
    let rows: Vec<EntityRow> = stmt
        .query_map(rusqlite::params![class_pattern, limit as i64], |row| {
            Ok(EntityRow {
                id: row.get(0)?,
                class: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
                z: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn query_traders(path: &str) -> Result<Vec<EntityRow>> {
    let mut all = Vec::new();
    for pattern in ["%Trader%", "%SedentaryNPC%", "%Mechanic%", "%Doctor%", "%Banker%"] {
        if let Ok(mut rows) = query_entities(path, pattern, 200) {
            all.append(&mut rows);
        }
    }
    all.sort_by_key(|r| r.id);
    all.dedup_by_key(|r| r.id);
    Ok(all)
}

// ─── player card + inventory ─────────────────────────────────────────────────

// Read a numeric UE save property (Double f64 / Float f32) from a blob by name.
// Layout: [i32 namelen][name\0][i32 typelen][type\0][i64 size][1b guid][value].
// Mirrors tools/scum-attrs.py; offsets re-derived per blob by name.
fn read_num_prop(blob: &[u8], name: &str) -> Option<f64> {
    let pat = name.as_bytes();
    let i = blob.windows(pat.len()).position(|w| w == pat)?;
    let mut pos = i + pat.len() + 1; // past name + null
    let typelen = i32::from_le_bytes(blob.get(pos..pos + 4)?.try_into().ok()?) as usize;
    pos += 4;
    let typ = std::str::from_utf8(blob.get(pos..pos + typelen.saturating_sub(1))?).ok()?;
    pos += typelen;
    pos += 8 + 1; // i64 size + guid byte
    match typ {
        "DoubleProperty" => Some(f64::from_le_bytes(blob.get(pos..pos + 8)?.try_into().ok()?)),
        "FloatProperty" => Some(f32::from_le_bytes(blob.get(pos..pos + 4)?.try_into().ok()?) as f64),
        _ => None,
    }
}

// Full player card: identity, economy, fame, attributes, vitals, skills.
pub fn player_card(path: &str, steam: &str) -> Result<serde_json::Value> {
    let conn = open_readonly(path)?;
    // Read potentially-numeric columns as f64 (tolerant of INTEGER or REAL storage);
    // name as Option to survive NULLs.
    // last_login_time is stored as TEXT (not numeric) — read as string. play_time/fame
    // read as f64 (tolerant of INTEGER or REAL).
    let (profile_id, name, prisoner_id, fame, play_time, last_login_s): (i64, Option<String>, Option<i64>, Option<f64>, Option<f64>, Option<String>) =
        conn.query_row(
            "SELECT id, name, prisoner_id, fame_points, play_time, CAST(last_login_time AS TEXT) \
             FROM user_profile WHERE CAST(user_id AS TEXT) = ?1 LIMIT 1",
            rusqlite::params![steam],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .map_err(|e| anyhow::anyhow!("player {} card lookup: {}", steam, e))?;
    let name = name.unwrap_or_else(|| steam.to_string());

    let (mut bank, mut gold) = (0i64, 0i64);
    if let Ok(mut stmt) = conn.prepare(
        "SELECT c.currency_type, c.account_balance FROM bank_account_registry a \
         JOIN bank_account_registry_currencies c ON c.bank_account_id = a.id \
         WHERE a.account_owner_user_profile_id = ?1",
    ) {
        if let Ok(rows) = stmt.query_map(rusqlite::params![profile_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))) {
            for (t, b) in rows.flatten() { if t == 1 { bank += b } else if t == 2 { gold += b } }
        }
    }

    let mut attributes = serde_json::Map::new();
    let mut vitals = serde_json::Map::new();
    let mut alive: Option<bool> = None;
    if let Some(pid) = prisoner_id {
        if let Ok((blob, is_alive)) = conn.query_row(
            "SELECT body_simulation, is_alive FROM prisoner WHERE id = ?1",
            rusqlite::params![pid],
            |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Option<i64>>(1)?)),
        ) {
            alive = is_alive.map(|v| v != 0);
            for a in ["BaseStrength", "BaseConstitution", "BaseDexterity", "BaseIntelligence"] {
                if let Some(v) = read_num_prop(&blob, a) {
                    attributes.insert(a.trim_start_matches("Base").to_string(), serde_json::json!(v));
                }
            }
            // Best-effort vital props (names vary by build; whatever parses is returned).
            for v in ["Stamina", "Mass", "BodyFatPercentage", "MuscleMassPercentage", "Vitamins",
                      "Carbohydrates", "Proteins", "Fat", "Sodium", "WaterContent", "BodyTemperature", "HeartRate"] {
                if let Some(val) = read_num_prop(&blob, v) { vitals.insert(v.to_string(), serde_json::json!(val)); }
            }
        }
    }

    let mut skills: Vec<serde_json::Value> = Vec::new();
    if let Some(pid) = prisoner_id {
        if let Ok(mut stmt) = conn.prepare("SELECT name, level, experience FROM prisoner_skill WHERE prisoner_id = ?1 ORDER BY name") {
            if let Ok(rows) = stmt.query_map(rusqlite::params![pid], |r| {
                Ok(serde_json::json!({
                    "name": r.get::<_, String>(0)?.trim_end_matches("Skill").to_string(),
                    "level": r.get::<_, Option<i64>>(1)?,
                    "xp": r.get::<_, Option<f64>>(2)?,
                }))
            }) { skills = rows.flatten().collect(); }
        }
    }

    Ok(serde_json::json!({
        "name": name, "steam": steam, "profileId": profile_id, "prisonerId": prisoner_id,
        "alive": alive, "fame": fame, "playTime": play_time, "lastLogin": last_login_s,
        "bank": bank, "gold": gold, "attributes": attributes, "vitals": vitals, "skills": skills,
    }))
}

// Read-WRITE connection. @inv only used by save-edit paths that run with SCUM STOPPED.
fn open_readwrite(path: &str) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path).with_context(|| format!("open rw {}", path))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Ok(conn)
}

/// Steam IDs currently in the `elevated_users` table (the dev-command unlock). Read-only, WAL-safe.
pub fn list_elevated(path: &str) -> Result<Vec<String>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare("SELECT user_id FROM elevated_users ORDER BY user_id")?;
    let ids: Vec<String> = stmt.query_map([], |row| row.get(0))?.filter_map(|r| r.ok()).collect();
    Ok(ids)
}

/// Apply elevated-user changes: INSERT OR IGNORE each `add`, DELETE each `remove`.
/// @inv CALL ONLY WITH SCUM STOPPED — `elevated_users` is read by SCUM at BOOT, so changes take
/// effect on the next launch; writing live risks the same save corruption as set_skills. Driven
/// from engine::start_server PRE-START (DB free). Additive/explicit; SCUM never writes this table.
pub fn apply_elevated(path: &str, add: &[String], remove: &[String]) -> Result<usize> {
    let conn = open_readwrite(path)?;
    let mut n = 0usize;
    for id in add {
        n += conn.execute("INSERT OR IGNORE INTO elevated_users (user_id) VALUES (?1)", rusqlite::params![id])?;
    }
    for id in remove {
        n += conn.execute("DELETE FROM elevated_users WHERE user_id = ?1", rusqlite::params![id])?;
    }
    Ok(n)
}

/// Steam IDs in `path` that have a prisoner (i.e. have actually spawned into the world).
pub fn list_players_with_prisoner(path: &str) -> Result<Vec<String>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare("SELECT CAST(user_id AS TEXT) FROM user_profile WHERE prisoner_id IS NOT NULL")?;
    let ids: Vec<String> = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
    Ok(ids)
}

/// Schema-break recovery: migrate player progression (skills + attributes + fame + money) from a
/// SOURCE snapshot DB into the TARGET (live, post-wipe) DB, for every steam id present with a
/// prisoner in BOTH. Reads via player_card, writes via set_skills/set_attributes/set_fame/set_money
/// (so it tolerates a changed schema as long as those tables/columns survive). @inv TARGET must be
/// SCUM-STOPPED + backed up — the handler enforces stop+checkpoint. Fame = user_profile.fame_points;
/// money = bank cash (currency_type 1) + gold (currency_type 2), restored into the player's bank
/// account (created if absent). Vehicles are NOT auto-spawned — a manifest (class+owner+location) is
/// extracted from the snapshot and saved to C:\TurdMOD\data\vehicle-manifest-<ts>.json for manual
/// #SpawnVehicle or a future spawner. Structures/inventory are not handled. Returns a per-player
/// progression report + the vehicle manifest.
pub fn migrate_progression(source: &str, target: &str) -> Result<serde_json::Value> {
    use std::collections::HashSet;
    let src: HashSet<String> = list_players_with_prisoner(source)?.into_iter().collect();
    let tgt: HashSet<String> = list_players_with_prisoner(target)?.into_iter().collect();
    let common: Vec<&String> = src.intersection(&tgt).collect();
    let mut report = Vec::new();
    let mut migrated = 0usize;
    for steam in &common {
        report.push(restore_one_player(source, target, steam));
        migrated += 1;
    }
    // Vehicle manifest from the SOURCE snapshot — class + owner + location for every vehicle.
    // NOT auto-spawned (vehicles are world entities). Saved to a file so the owner can manually
    // #SpawnVehicle them later or build a spawner system off this list. Best-effort.
    let vehicles = map_vehicles(source).unwrap_or_else(|_| serde_json::json!([]));
    let veh_count = vehicles.as_array().map(|a| a.len()).unwrap_or(0);
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let manifest_file = format!(r"C:\TurdMOD\data\vehicle-manifest-{}.json", ts);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    let _ = std::fs::write(&manifest_file, serde_json::to_string_pretty(&vehicles).unwrap_or_default());
    Ok(serde_json::json!({
        "ok": true, "source": source, "common_players": common.len(), "migrated": migrated, "report": report,
        "vehicle_manifest_file": manifest_file, "vehicle_count": veh_count, "vehicles": vehicles
    }))
}

/// Restore ONE player's progression (skills + attributes + fame + money) from `source` into
/// `target`, returning a per-player JSON report. Shared by migrate_progression (one-shot) and
/// restore_pending (campaign). @inv TARGET must be SCUM-STOPPED + backed up. Phase 1 scope:
/// skills/attrs/fame/money only — inventory + vehicles are phase 2.
fn restore_one_player(source: &str, target: &str, steam: &str) -> serde_json::Value {
    let card = match player_card(source, steam) {
        Ok(c) => c,
        Err(e) => return serde_json::json!({ "steam": steam, "error": e.to_string() }),
    };
    let name = card["name"].as_str().unwrap_or(steam).to_string();
    let skill_edits: Vec<(String, Option<i64>, Option<f64>)> = card["skills"].as_array()
        .map(|arr| arr.iter().filter_map(|s| Some((s["name"].as_str()?.to_string(), s["level"].as_i64(), s["xp"].as_f64()))).collect())
        .unwrap_or_default();
    let attr_edits: Vec<(String, f64)> = card["attributes"].as_object()
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect())
        .unwrap_or_default();
    let s = set_skills(target, steam, &skill_edits).unwrap_or(0);
    let a = set_attributes(target, steam, &attr_edits).unwrap_or(0);
    // Fame — always carry if present (user_profile row exists post-rejoin).
    let fame = card["fame"].as_f64();
    let fame_updated = match fame { Some(f) => set_fame(target, steam, f).unwrap_or(0), None => 0 };
    // Money — only when the player actually held a balance (don't fabricate empty accounts).
    let bank = card["bank"].as_i64().unwrap_or(0);
    let gold = card["gold"].as_i64().unwrap_or(0);
    let money = if bank != 0 || gold != 0 {
        let meta = bank_account_meta(source, steam);
        match set_money(target, steam, bank, gold,
                        meta.as_ref().map(|m| m.0), meta.as_ref().and_then(|m| m.1), meta.map(|m| m.2).unwrap_or(false)) {
            Ok(v) => v,
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    } else {
        serde_json::json!({ "bank": 0, "gold": 0, "skipped": "no balance in source" })
    };
    serde_json::json!({
        "steam": steam, "name": name, "skills_updated": s, "attrs_updated": a,
        "fame": fame, "fame_updated": fame_updated, "money": money,
    })
}

/// Campaign restore: idempotent, reconnect-gated. Restores every steam id that is present in BOTH
/// the `source` snapshot and the `target` (live post-wipe DB — i.e. the player has RECONNECTED and
/// SCUM rebuilt their profile/prisoner) and is NOT already in `done`. Returns (newly_restored_ids,
/// report); the caller persists the ids so a player is restored EXACTLY ONCE — re-applying on a
/// later restart would clobber progress made since their restore. @inv TARGET must be SCUM-STOPPED
/// + backed up (the campaign calls this inside a restart stop-window). Phase 1 scope: skills/attrs/
/// fame/money only.
pub fn restore_pending(
    source: &str, target: &str, done: &std::collections::HashSet<String>,
) -> Result<(Vec<String>, serde_json::Value)> {
    use std::collections::HashSet;
    let src: HashSet<String> = list_players_with_prisoner(source)?.into_iter().collect();
    let tgt: HashSet<String> = list_players_with_prisoner(target)?.into_iter().collect();
    // reconnected (in both) AND not already restored
    let mut pending: Vec<&String> = src.intersection(&tgt).filter(|s| !done.contains(*s)).collect();
    pending.sort();
    let mut restored = Vec::new();
    let mut report = Vec::new();
    for steam in pending {
        let r = restore_one_player(source, target, steam);
        if r.get("error").is_none() { restored.push(steam.clone()); }
        report.push(r);
    }
    let summary = serde_json::json!({
        "ok": true, "source": source,
        "reconnected_total": src.intersection(&tgt).count(),
        "already_done": done.len(),
        "newly_restored": restored.len(),
        "report": report,
    });
    Ok((restored, summary))
}

/// Write skill level/xp for a player's prisoner. Returns rows updated.
/// @inv CALL ONLY WITH SCUM STOPPED + AFTER a backup — live writes to the save can
/// corrupt it (feedback_scumdb_edit_safety). handle_set_skill enforces stop + checkpoint.
/// `edits` = (skill_name, level?, xp?). Skill names match the DB with or without the
/// trailing "Skill" (the card trims it for display, e.g. "Rifles" ↔ "RiflesSkill").
pub fn set_skills(path: &str, steam: &str, edits: &[(String, Option<i64>, Option<f64>)]) -> Result<usize> {
    let conn = open_readwrite(path)?;
    let prisoner_id: i64 = conn
        .query_row(
            "SELECT prisoner_id FROM user_profile WHERE CAST(user_id AS TEXT) = ?1 AND prisoner_id IS NOT NULL LIMIT 1",
            rusqlite::params![steam],
            |r| r.get(0),
        )
        .map_err(|e| anyhow::anyhow!("no prisoner for steam {}: {}", steam, e))?;
    let mut updated = 0usize;
    for (name, level, xp) in edits {
        updated += match (level, xp) {
            (Some(lv), Some(x)) => conn.execute(
                "UPDATE prisoner_skill SET level=?1, experience=?2 WHERE prisoner_id=?3 AND (name=?4 OR name=?4||'Skill')",
                rusqlite::params![lv, x, prisoner_id, name],
            )?,
            (Some(lv), None) => conn.execute(
                "UPDATE prisoner_skill SET level=?1 WHERE prisoner_id=?2 AND (name=?3 OR name=?3||'Skill')",
                rusqlite::params![lv, prisoner_id, name],
            )?,
            (None, Some(x)) => conn.execute(
                "UPDATE prisoner_skill SET experience=?1 WHERE prisoner_id=?2 AND (name=?3 OR name=?3||'Skill')",
                rusqlite::params![x, prisoner_id, name],
            )?,
            (None, None) => 0,
        };
    }
    Ok(updated)
}

// In-place write of a numeric UE property inside a body_simulation blob — the WRITE
// twin of read_num_prop (same offset walk). Returns true if the value was written.
// Ported from tools/scum-attrs.py (proven on OVH). @inv blob layout is build-stable.
fn write_num_prop(blob: &mut [u8], name: &str, value: f64) -> bool {
    let pat = name.as_bytes();
    let Some(i) = blob.windows(pat.len()).position(|w| w == pat) else { return false };
    let mut pos = i + pat.len() + 1; // past name + null
    let Some(tl) = blob.get(pos..pos + 4) else { return false };
    let typelen = i32::from_le_bytes(tl.try_into().unwrap()) as usize;
    pos += 4;
    let Some(tb) = blob.get(pos..pos + typelen.saturating_sub(1)) else { return false };
    let typ = match std::str::from_utf8(tb) { Ok(s) => s.to_string(), Err(_) => return false };
    pos += typelen;
    pos += 8 + 1; // i64 size + guid byte
    match typ.as_str() {
        "DoubleProperty" => {
            if blob.len() < pos + 8 { return false; }
            blob[pos..pos + 8].copy_from_slice(&value.to_le_bytes());
            true
        }
        "FloatProperty" => {
            if blob.len() < pos + 4 { return false; }
            blob[pos..pos + 4].copy_from_slice(&(value as f32).to_le_bytes());
            true
        }
        _ => false,
    }
}

/// Save-edit a player's base attributes (Strength/Constitution/Dexterity/Intelligence) by
/// writing the DoubleProperty values in the prisoner.body_simulation blob. Returns count
/// written. @inv CALL ONLY WITH SCUM STOPPED + AFTER a backup — SCUM rewrites the blob on
/// save/logout, clobbering live edits (tools/scum-attrs.py). handle_set_attribute enforces
/// stop + checkpoint. `edits` = (attr_name, value); names match the card's display form
/// (e.g. "Strength" → "BaseStrength" in the blob).
pub fn set_attributes(path: &str, steam: &str, edits: &[(String, f64)]) -> Result<usize> {
    let conn = open_readwrite(path)?;
    let prisoner_id: i64 = conn
        .query_row(
            "SELECT prisoner_id FROM user_profile WHERE CAST(user_id AS TEXT) = ?1 AND prisoner_id IS NOT NULL LIMIT 1",
            rusqlite::params![steam],
            |r| r.get(0),
        )
        .map_err(|e| anyhow::anyhow!("no prisoner for steam {}: {}", steam, e))?;
    let mut blob: Vec<u8> = conn
        .query_row(
            "SELECT body_simulation FROM prisoner WHERE id = ?1",
            rusqlite::params![prisoner_id],
            |r| r.get(0),
        )
        .map_err(|e| anyhow::anyhow!("read body_simulation for prisoner {}: {}", prisoner_id, e))?;
    let mut written = 0usize;
    for (name, value) in edits {
        let key = if name.starts_with("Base") { name.clone() } else { format!("Base{}", name) };
        if write_num_prop(&mut blob, &key, *value) {
            written += 1;
        }
    }
    if written > 0 {
        conn.execute(
            "UPDATE prisoner SET body_simulation = ?1 WHERE id = ?2",
            rusqlite::params![blob, prisoner_id],
        )?;
    }
    Ok(written)
}

/// Write a player's fame_points (user_profile column, REAL). Returns rows updated (0 if the
/// player has no profile in `path` yet). The profile row exists the moment a player connects
/// to the new world (before they spawn a prisoner), so fame is the most reliable field to carry.
/// @inv CALL ONLY WITH SCUM STOPPED + AFTER a backup — handle_migrate enforces stop + checkpoint.
pub fn set_fame(path: &str, steam: &str, fame: f64) -> Result<usize> {
    let conn = open_readwrite(path)?;
    Ok(conn.execute(
        "UPDATE user_profile SET fame_points = ?1 WHERE CAST(user_id AS TEXT) = ?2",
        rusqlite::params![fame, steam],
    )?)
}

/// Source-side read of a player's owning bank account metadata (account number + map + DDP flag),
/// used by set_money's create-account fallback so a migrated account keeps the player's real number.
/// Returns None if the source player has no bank account. @inv read-only, WAL-safe.
fn bank_account_meta(path: &str, steam: &str) -> Option<(i64, Option<i64>, bool)> {
    let conn = open_readonly(path).ok()?;
    let profile_id: i64 = conn.query_row(
        "SELECT id FROM user_profile WHERE CAST(user_id AS TEXT) = ?1 LIMIT 1",
        rusqlite::params![steam], |r| r.get(0),
    ).ok()?;
    conn.query_row(
        "SELECT bank_account_number, map_id, used_digital_deluxe_privileges \
         FROM bank_account_registry WHERE account_owner_user_profile_id = ?1 LIMIT 1",
        rusqlite::params![profile_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<bool>>(2)?.unwrap_or(false))),
    ).ok()
}

/// Write a player's bank (currency_type 1) + gold (currency_type 2) balances into `path`.
/// Strategy mirrors the migrator's "update what exists" philosophy with a money-specific fallback:
/// find the player's owning bank_account_registry row → UPDATE each currency row's balance, INSERT
/// the currency row if absent. If the player has NO bank account in the target yet (hasn't visited a
/// bank in the new world), one is created using the source account's number/map (`src_*`) so the
/// money still lands — bank accounts are profile-attached financial records, not world entities, so
/// creating one is safe (unlike spawning vehicles/structures). Returns a per-field report.
/// @inv CALL ONLY WITH SCUM STOPPED + AFTER a backup — handle_migrate enforces stop + checkpoint.
/// @ctx schema: bank_account_registry(id,map_id,user_profile_id,account_owner_user_profile_id,
///      bank_account_number,save_timestamp,used_digital_deluxe_privileges);
///      bank_account_registry_currencies(id,map_id,user_profile_id,bank_account_id,currency_type,account_balance).
pub fn set_money(
    path: &str, steam: &str, bank: i64, gold: i64,
    src_acct_number: Option<i64>, src_map_id: Option<i64>, src_ddp: bool,
) -> Result<serde_json::Value> {
    let conn = open_readwrite(path)?;
    let profile_id: i64 = conn.query_row(
        "SELECT id FROM user_profile WHERE CAST(user_id AS TEXT) = ?1 LIMIT 1",
        rusqlite::params![steam],
        |r| r.get(0),
    ).map_err(|e| anyhow::anyhow!("no profile for steam {}: {}", steam, e))?;

    // Locate (or create) the player's owning bank account.
    let mut account_id: Option<i64> = conn.query_row(
        "SELECT id FROM bank_account_registry WHERE account_owner_user_profile_id = ?1 LIMIT 1",
        rusqlite::params![profile_id], |r| r.get(0),
    ).ok();
    let mut created_account = false;
    if account_id.is_none() {
        let new_id: i64 = conn.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM bank_account_registry", [], |r| r.get(0))?;
        let acct_no = src_acct_number.unwrap_or(new_id); // keep the player's real number; else a distinct one
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        conn.execute(
            "INSERT INTO bank_account_registry \
             (id, map_id, user_profile_id, account_owner_user_profile_id, bank_account_number, save_timestamp, used_digital_deluxe_privileges) \
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
            rusqlite::params![new_id, src_map_id, profile_id, acct_no, now, src_ddp],
        )?;
        account_id = Some(new_id);
        created_account = true;
    }
    let account_id = account_id.unwrap();
    let acct_map_id: Option<i64> = conn.query_row(
        "SELECT map_id FROM bank_account_registry WHERE id = ?1",
        rusqlite::params![account_id], |r| r.get(0),
    ).unwrap_or(None);

    // UPDATE-or-INSERT one currency row.
    let set_currency = |ctype: i64, bal: i64| -> Result<&'static str> {
        let updated = conn.execute(
            "UPDATE bank_account_registry_currencies SET account_balance = ?1 \
             WHERE bank_account_id = ?2 AND currency_type = ?3",
            rusqlite::params![bal, account_id, ctype],
        )?;
        if updated > 0 { return Ok("updated"); }
        let new_id: i64 = conn.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM bank_account_registry_currencies", [], |r| r.get(0))?;
        conn.execute(
            "INSERT INTO bank_account_registry_currencies \
             (id, map_id, user_profile_id, bank_account_id, currency_type, account_balance) \
             VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
            rusqlite::params![new_id, acct_map_id, account_id, ctype, bal],
        )?;
        Ok("inserted")
    };
    let bank_row = set_currency(1, bank)?;
    let gold_row = set_currency(2, gold)?;
    Ok(serde_json::json!({
        "bank": bank, "gold": gold, "bank_row": bank_row, "gold_row": gold_row,
        "created_account": created_account,
    }))
}

// Full inventory: every entity owned (recursively through containers) by the
// player's prisoner entity. Item type = entity.class; cash = money-class items.
pub fn player_inventory(path: &str, steam: &str) -> Result<serde_json::Value> {
    let conn = open_readonly(path)?;
    let prisoner_id: i64 = conn.query_row(
        "SELECT prisoner_id FROM user_profile WHERE CAST(user_id AS TEXT) = ?1 AND prisoner_id IS NOT NULL LIMIT 1",
        rusqlite::params![steam], |r| r.get(0),
    ).with_context(|| format!("no prisoner for {}", steam))?;
    let root_entity: i64 = conn.query_row(
        "SELECT entity_id FROM prisoner_entity WHERE prisoner_id = ?1 LIMIT 1",
        rusqlite::params![prisoner_id], |r| r.get(0),
    ).context("no prisoner entity")?;

    let mut stmt = conn.prepare("SELECT id, class FROM entity WHERE owning_entity_id = ?1")?;
    let mut frontier = vec![root_entity];
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut guard = 0;
    while let Some(owner) = frontier.pop() {
        guard += 1; if guard > 5000 { break; }
        if let Ok(rows) = stmt.query_map(rusqlite::params![owner], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) {
            for (id, class) in rows.flatten() {
                if !seen.insert(id) { continue; }
                frontier.push(id); // containers own more entities
                items.push(serde_json::json!({ "entityId": id, "class": class, "owner": owner }));
            }
        }
    }

    let cash = items.iter().filter(|i| {
        let c = i.get("class").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        c.contains("money") || c.contains("cash") || c.contains("dollar")
    }).count() as i64;

    Ok(serde_json::json!({
        "steam": steam, "prisonerId": prisoner_id, "rootEntity": root_entity,
        "count": items.len(), "cashItems": cash, "items": items,
    }))
}

// All vehicles with map locations (vehicle_entity → entity join). For the map layer.
pub fn map_vehicles(path: &str) -> Result<serde_json::Value> {
    let conn = open_readonly(path)?;
    // profile id -> name, to resolve the owner from the lock-slot xml
    // (_owningUserProfileId on the vehicle's item-container). SCUM's
    // entity.owning_entity_id is unused for vehicles, so this is the real owner.
    let mut names: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    {
        let mut ns = conn.prepare("SELECT id, name FROM user_profile WHERE name IS NOT NULL")?;
        for row in ns
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .flatten()
        {
            names.insert(row.0, row.1);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT e.id, e.class, e.location_x, e.location_y, e.location_z, ie.xml \
         FROM vehicle_entity v JOIN entity e ON e.id = v.entity_id \
         LEFT JOIN item_entity ie ON ie.entity_id = v.item_container_entity_id \
         WHERE e.location_x IS NOT NULL",
    )?;
    let rows: Vec<serde_json::Value> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, class, x, y, z, xml)| {
            let owner = xml
                .as_deref()
                .and_then(|s| xml_attr(s, "_owningUserProfileId"))
                .and_then(|s| s.parse::<i64>().ok())
                .and_then(|pid| names.get(&pid).cloned());
            let locked = xml.as_deref().map(|s| s.contains("<LockSlot")).unwrap_or(false);
            serde_json::json!({
                "id": id, "class": class, "x": x, "y": y, "z": z,
                "owner": owner, "locked": locked,
            })
        })
        .collect();
    Ok(serde_json::json!({ "count": rows.len(), "vehicles": rows }))
}

// SCUM map sector (e.g. "B3") from world coords — mirrors chat_cmds::coords_to_sector.
pub fn sector_of(x: f64, y: f64) -> String {
    let row_letters = ["D", "C", "B", "A", "Z"];
    let cell = 1_524_000.0 / 5.0;
    let col = ((619_200.0 - x) / cell).floor().max(0.0) as usize;
    let row = ((619_200.0 - y) / cell).floor().max(0.0) as usize;
    format!("{}{}", row_letters[row.min(4)], 4 - col.min(4))
}

// All vehicles with sector + coords + owner + lock + attached part classes — for the
// TMM Vehicle Manager "Server Vehicles" tab. The frontend derives "missing parts" by
// comparing each vehicle to the fullest instance of its class.
pub fn vehicles_detail(path: &str) -> Result<serde_json::Value> {
    let conn = open_readonly(path)?;
    let mut names: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    {
        let mut ns = conn.prepare("SELECT id, name FROM user_profile WHERE name IS NOT NULL")?;
        for r in ns.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.flatten() {
            names.insert(r.0, r.1);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT e.id, e.class, e.location_x, e.location_y, e.location_z, ie.xml \
         FROM vehicle_entity v JOIN entity e ON e.id = v.entity_id \
         LEFT JOIN item_entity ie ON ie.entity_id = v.item_container_entity_id \
         WHERE e.location_x IS NOT NULL",
    )?;
    let base: Vec<(i64, String, Option<f64>, Option<f64>, Option<f64>, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut parts: std::collections::HashMap<i64, Vec<String>> = std::collections::HashMap::new();
    if !base.is_empty() {
        let ids: Vec<i64> = base.iter().map(|b| b.0).collect();
        let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT owning_entity_id, class FROM entity WHERE owning_entity_id IN ({})", ph);
        if let Ok(mut ps) = conn.prepare(&sql) {
            if let Ok(rows) = ps.query_map(rusqlite::params_from_iter(ids.iter()), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) {
                for (oid, class) in rows.flatten() { parts.entry(oid).or_default().push(class); }
            }
        }
    }

    let vehicles: Vec<serde_json::Value> = base.into_iter().map(|(id, class, x, y, z, xml)| {
        let owner = xml.as_deref().and_then(|s| xml_attr(s, "_owningUserProfileId"))
            .and_then(|s| s.parse::<i64>().ok()).and_then(|pid| names.get(&pid).cloned());
        let locked = xml.as_deref().map(|s| s.contains("<LockSlot")).unwrap_or(false);
        let sector = match (x, y) { (Some(x), Some(y)) => sector_of(x, y), _ => "?".into() };
        let mut p = parts.remove(&id).unwrap_or_default();
        p.sort();
        serde_json::json!({ "id": id, "class": class, "x": x, "y": y, "z": z, "sector": sector, "owner": owner, "locked": locked, "parts": p })
    }).collect();
    Ok(serde_json::json!({ "count": vehicles.len(), "vehicles": vehicles }))
}

// Real map zones — custom_zone_region (geometry) joined to custom_zone_configuration
// (name + RGB colour). size_y≈0 → circle (radius size_x); else rectangle size_x×size_y.
pub fn map_zones(path: &str) -> Result<serde_json::Value> {
    let conn = open_readonly(path)?;
    // configuration_index is a 0-based ordinal into the configs ordered by id
    // (the global config, id 1000, is excluded from that ordering).
    let mut cfg: Vec<(String, String)> = Vec::new();
    if let Ok(mut s) = conn.prepare("SELECT name, color_red, color_green, color_blue FROM custom_zone_configuration WHERE id < 1000 ORDER BY id") {
        if let Ok(rows) = s.query_map([], |r| Ok((
            r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            r.get::<_, Option<f64>>(1)?.unwrap_or(0.5),
            r.get::<_, Option<f64>>(2)?.unwrap_or(0.5),
            r.get::<_, Option<f64>>(3)?.unwrap_or(0.5),
        ))) {
            for (name, rr, gg, bb) in rows.flatten() {
                let t = |f: f64| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
                cfg.push((name, format!("#{:02x}{:02x}{:02x}", t(rr), t(gg), t(bb))));
            }
        }
    }
    let mut s = conn.prepare("SELECT name, location_x, location_y, size_x, size_y, configuration_index FROM custom_zone_region")?;
    let zones: Vec<serde_json::Value> = s
        .query_map([], |r| Ok((
            r.get::<_, Option<String>>(0)?, r.get::<_, Option<f64>>(1)?, r.get::<_, Option<f64>>(2)?,
            r.get::<_, Option<f64>>(3)?, r.get::<_, Option<f64>>(4)?, r.get::<_, Option<i64>>(5)?,
        )))?
        .filter_map(|r| r.ok())
        .filter_map(|(name, x, y, sx, sy, ci)| {
            let (x, y) = (x?, y?);
            let (cname, color) = ci.and_then(|i| usize::try_from(i).ok()).and_then(|i| cfg.get(i)).cloned().unwrap_or((String::new(), "#888888".into()));
            Some(serde_json::json!({
                "name": name.filter(|n| !n.is_empty()).unwrap_or(cname.clone()), "config": cname,
                "x": x, "y": y, "sizeX": sx.unwrap_or(0.0), "sizeY": sy.unwrap_or(0.0), "color": color,
            }))
        })
        .collect();
    Ok(serde_json::json!({ "count": zones.len(), "zones": zones }))
}

// ─── vehicle snapshot ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ItemSnapshot {
    pub entity_id: i64,
    pub class: String,
    pub health: Option<f64>,
    pub max_health: Option<f64>,
    pub total_weight: Option<f64>,
    pub xml: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VehicleSnapshot {
    pub entity_id: i64,
    pub class: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub rotation: (f64, f64, f64),
    pub container_id: Option<i64>,
    pub items: Vec<ItemSnapshot>,
    pub vehicle_data_len: usize,
    pub last_access_time: Option<i64>,
    pub is_functional: Option<bool>,
    pub snapshot_at: u64,
}

pub fn snapshot_vehicle(path: &str, entity_id: i64) -> Result<VehicleSnapshot> {
    let conn = open_readonly(path)?;

    let (class, x, y, z, rot_x, rot_y, rot_z): (String, f64, f64, f64, f64, f64, f64) = conn
        .query_row(
            "SELECT class, location_x, location_y, location_z, \
             rotation_x, rotation_y, rotation_z FROM entity WHERE id = ?1",
            rusqlite::params![entity_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                       row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .with_context(|| format!("entity {} not found", entity_id))?;

    let (container_id, vehicle_data_len): (Option<i64>, usize) = conn
        .query_row(
            "SELECT item_container_entity_id, length(data) FROM vehicle_entity WHERE entity_id = ?1",
            rusqlite::params![entity_id],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1).unwrap_or(0) as usize)),
        )
        .unwrap_or((None, 0));

    let (last_access_time, is_functional): (Option<i64>, Option<bool>) = conn
        .query_row(
            "SELECT vehicle_last_access_time, is_vehicle_functional \
             FROM vehicle_spawner WHERE vehicle_entity_id = ?1",
            rusqlite::params![entity_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((None, None));

    // @inv: items owned by the vehicle OR its container, joined with item_entity for stats
    let mut stmt = conn.prepare(
        "SELECT e.id, e.class, ie.health, ie.max_health, ie.total_weight, ie.xml \
         FROM entity e \
         JOIN item_entity ie ON ie.entity_id = e.id \
         WHERE e.owning_entity_id = ?1 \
            OR (?2 IS NOT NULL AND e.owning_entity_id = ?2)"
    )?;
    let items: Vec<ItemSnapshot> = stmt
        .query_map(rusqlite::params![entity_id, container_id], |row| {
            Ok(ItemSnapshot {
                entity_id: row.get(0)?,
                class: row.get(1)?,
                health: row.get(2)?,
                max_health: row.get(3)?,
                total_weight: row.get(4)?,
                xml: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    let snapshot_at = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);

    Ok(VehicleSnapshot {
        entity_id, class, x, y, z,
        rotation: (rot_x, rot_y, rot_z),
        container_id, items, vehicle_data_len,
        last_access_time, is_functional, snapshot_at,
    })
}

pub fn find_vehicle_by_class_near(path: &str, class_pattern: &str, px: f64, py: f64, radius: f64) -> Result<Option<EntityRow>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare(
        "SELECT e.id, e.class, e.location_x, e.location_y, e.location_z \
         FROM entity e JOIN vehicle_entity ve ON ve.entity_id = e.id \
         WHERE e.class LIKE ?1"
    )?;
    let radius_sq = radius * radius;
    let nearest = stmt
        .query_map(rusqlite::params![class_pattern], |row| {
            Ok(EntityRow { id: row.get(0)?, class: row.get(1)?, x: row.get(2)?, y: row.get(3)?, z: row.get(4)? })
        })?
        .filter_map(|r| r.ok())
        .filter(|e| (e.x - px).powi(2) + (e.y - py).powi(2) <= radius_sq)
        .min_by(|a, b| {
            let da = (a.x - px).powi(2) + (a.y - py).powi(2);
            let db = (b.x - px).powi(2) + (b.y - py).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    Ok(nearest)
}

pub fn vehicle_alias(path: &str, entity_id: i64) -> Result<Option<String>> {
    let conn = open_readonly(path)?;
    let alias: Option<String> = conn.query_row(
        "SELECT vehicle_alias FROM vehicle_spawner WHERE vehicle_entity_id = ?1",
        rusqlite::params![entity_id],
        |row| row.get(0),
    ).ok();
    Ok(alias)
}

pub fn find_all_vehicles(path: &str) -> Result<Vec<EntityRow>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare(
        "SELECT e.id, e.class, e.location_x, e.location_y, e.location_z \
         FROM entity e JOIN vehicle_entity ve ON ve.entity_id = e.id ORDER BY e.id"
    )?;
    let rows: Vec<EntityRow> = stmt
        .query_map([], |row| {
            Ok(EntityRow { id: row.get(0)?, class: row.get(1)?, x: row.get(2)?, y: row.get(3)?, z: row.get(4)? })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ─── vehicle native ownership + lock (the lock-gated-register signal) ─────────
// @ctx: SCUM stores vehicle owner + lock TOGETHER in item_entity.xml on the
// vehicle's item-container — NOT entity.owning_entity_id (structural). Placing a
// lock sets ownership. Chain: vehicle_entity.entity_id -> .item_container_entity_id
// -> item_entity.xml carries _owningUserProfileId="N" + <Locks><LockSlot .../></Locks>.
// See memory reference_vehicle_ownership_lock_signal + docs/HOOKS-NEEDED.md.

#[derive(Debug, Clone, Serialize)]
pub struct VehicleOwnerLock {
    pub owning_profile_id: Option<i64>, // _owningUserProfileId (0/absent = unowned)
    pub has_lock: bool,                 // a <LockSlot> is present
    pub lock_asset: Option<String>,     // e.g. "Item:Lock_Item_Medium"
}

// Extract the value of attr=`name`="..." from an XML/string blob (first match).
fn xml_attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{}=\"", name);
    let start = xml.find(&key)? + key.len();
    let rest = &xml[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Owner + lock state for a vehicle ENTITY id (the Rager_ES id, not the container).
pub fn vehicle_owner_lock(path: &str, vehicle_entity_id: i64) -> Result<VehicleOwnerLock> {
    let conn = open_readonly(path)?;
    // vehicle -> container
    let container_id: Option<i64> = conn
        .query_row(
            "SELECT item_container_entity_id FROM vehicle_entity WHERE entity_id = ?1",
            rusqlite::params![vehicle_entity_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    let Some(cid) = container_id else {
        return Ok(VehicleOwnerLock { owning_profile_id: None, has_lock: false, lock_asset: None });
    };
    // container -> item_entity.xml
    let xml: Option<String> = conn
        .query_row(
            "SELECT xml FROM item_entity WHERE entity_id = ?1",
            rusqlite::params![cid],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    let Some(xml) = xml else {
        return Ok(VehicleOwnerLock { owning_profile_id: None, has_lock: false, lock_asset: None });
    };
    let owning_profile_id = xml_attr(&xml, "_owningUserProfileId")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|&n| n != 0);
    let has_lock = xml.contains("<LockSlot");
    let lock_asset = if has_lock {
        // the LockSlot's _assetId
        xml.split("<LockSlot").nth(1)
            .and_then(|s| xml_attr(s, "_assetId"))
            .map(|s| s.to_string())
    } else { None };
    Ok(VehicleOwnerLock { owning_profile_id, has_lock, lock_asset })
}

/// Resolve a SteamID64 to its SCUM user_profile.id (user_profile.user_id == steam).
/// Last SAVED world position for a player — crash-proof. SCUM writes the
/// prisoner entity's location on clean logout AND on the ~90s EntitySystem
/// auto-save, so this survives a force-close / disconnect / crash (worst case
/// ~90s stale). Read-only, no bridge poll. Joins user_profile -> prisoner_entity
/// -> entity. @dep death_notes / map last-known-location layer.
pub fn player_last_position(path: &str, steam: &str) -> Result<Option<(f64, f64, f64)>> {
    let conn = open_readonly(path)?;
    let pos = conn
        .query_row(
            "SELECT e.location_x, e.location_y, e.location_z \
             FROM user_profile up \
             JOIN prisoner_entity pe ON pe.prisoner_id = up.prisoner_id \
             JOIN entity e ON e.id = pe.entity_id \
             WHERE up.user_id = ?1 AND e.location_x IS NOT NULL",
            rusqlite::params![steam],
            |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?)),
        )
        .ok();
    Ok(pos)
}

/// Batch: last SAVED position for every player with a spawned prisoner. ONE
/// query — backs the map's last-known-location layer (ghost markers for offline
/// players). Crash-proof + read-only (same source as player_last_position).
pub fn last_positions(path: &str) -> Result<Vec<serde_json::Value>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare(
        "SELECT CAST(up.user_id AS TEXT), up.name, e.location_x, e.location_y, e.location_z, up.last_login_time \
         FROM user_profile up \
         JOIN prisoner_entity pe ON pe.prisoner_id = up.prisoner_id \
         JOIN entity e ON e.id = pe.entity_id \
         WHERE e.location_x IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
            r.get::<_, f64>(3)?,
            r.get::<_, f64>(4)?,
            r.get::<_, Option<String>>(5).unwrap_or(None),
        ))
    })?;
    let mut out = Vec::new();
    for (steam, name, x, y, z, last_login) in rows.flatten() {
        out.push(serde_json::json!({
            "steam": steam, "name": name, "x": x, "y": y, "z": z,
            "sector": sector_of(x, y), "last_login": last_login,
        }));
    }
    Ok(out)
}

pub fn profile_id_for_steam(path: &str, steam: &str) -> Result<Option<i64>> {
    let conn = open_readonly(path)?;
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM user_profile WHERE user_id = ?1",
            rusqlite::params![steam],
            |row| row.get(0),
        )
        .ok();
    Ok(id)
}

/// Resolve a player display NAME to its user_profile.id (fallback when the chat
/// event carries no SteamID). @brk: names aren't guaranteed unique — takes the
/// most-recently-active match.
pub fn profile_id_for_name(path: &str, name: &str) -> Result<Option<i64>> {
    let conn = open_readonly(path)?;
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM user_profile WHERE name = ?1 ORDER BY last_login_time DESC LIMIT 1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .ok();
    Ok(id)
}

/// Reverse: SteamID64 for a user_profile.id.
pub fn steam_for_profile_id(path: &str, profile_id: i64) -> Result<Option<String>> {
    let conn = open_readonly(path)?;
    let steam: Option<String> = conn
        .query_row(
            "SELECT user_id FROM user_profile WHERE id = ?1",
            rusqlite::params![profile_id],
            |row| row.get(0),
        )
        .ok();
    Ok(steam)
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnedVehicle {
    pub entity_id: i64,
    pub class: String,      // e.g. "Rager_ES"
    pub lock_asset: Option<String>,
    pub owner_profile: i64, // the user_profile that actually owns it (may be a dup profile)
}

/// ALL user_profile ids for a player (by steam, else by name). A single Steam
/// account can have MULTIPLE profiles (re-rolls / dup rows) — e.g. Lilac had
/// profiles 3 AND 6, with her locked vehicle owned by 6 while `profile_id_for_steam`
/// returned 3 (no ORDER BY). Resolve the full set so ownership checks match any.
pub fn profile_ids_for(path: &str, steam: &str, name: &str) -> Result<Vec<i64>> {
    let conn = open_readonly(path)?;
    let mut ids = Vec::new();
    if !steam.is_empty() {
        let mut s = conn.prepare("SELECT id FROM user_profile WHERE user_id = ?1")?;
        for r in s.query_map(rusqlite::params![steam], |row| row.get::<_, i64>(0))?.flatten() {
            ids.push(r);
        }
    }
    if ids.is_empty() && !name.is_empty() {
        let mut s = conn.prepare("SELECT id FROM user_profile WHERE name = ?1")?;
        for r in s.query_map(rusqlite::params![name], |row| row.get::<_, i64>(0))?.flatten() {
            ids.push(r);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// All vehicles natively OWNED + LOCKED by ANY of the given user_profiles (the
/// lock-gated set). Joins vehicle -> container -> item_entity.xml and filters on
/// _owningUserProfileId ∈ profile_ids AND a <LockSlot> present. Pass the full
/// profile set (see `profile_ids_for`) so dup-profile players still match.
pub fn vehicles_owned_locked_by(path: &str, profile_ids: &[i64]) -> Result<Vec<OwnedVehicle>> {
    let conn = open_readonly(path)?;
    let mut stmt = conn.prepare(
        "SELECT e.id, e.class, ie.xml \
         FROM entity e \
         JOIN vehicle_entity ve ON ve.entity_id = e.id \
         JOIN item_entity ie ON ie.entity_id = ve.item_container_entity_id \
         WHERE ie.xml IS NOT NULL"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
    })?;
    let mut out = Vec::new();
    for r in rows.flatten() {
        let (id, class, xml) = r;
        let Some(xml) = xml else { continue };
        let owner = xml_attr(&xml, "_owningUserProfileId").and_then(|s| s.parse::<i64>().ok());
        let Some(owner) = owner else { continue };
        if !profile_ids.contains(&owner) { continue; }
        if !xml.contains("<LockSlot") { continue; }
        let lock_asset = xml.split("<LockSlot").nth(1)
            .and_then(|s| xml_attr(s, "_assetId")).map(|s| s.to_string());
        out.push(OwnedVehicle { entity_id: id, class, lock_asset, owner_profile: owner });
    }
    Ok(out)
}

#[cfg(test)]
mod migrate_tests {
    use super::*;

    // End-to-end migrate test against a REAL SCUM.db clone (e.g. a production OVH pull).
    // Set TEST_MIGRATE_SRC to a self-contained .db (WAL already checkpointed in); the test
    // copies it to a temp target, ZEROES every player's fame/money/skills, asserts the zeroing
    // took, runs the shipping migrate_progression, then asserts each player's values are restored
    // to the source. Skips silently when TEST_MIGRATE_SRC is unset so normal builds/CI are unaffected.
    #[test]
    fn fame_money_skills_round_trip() {
        let Ok(src) = std::env::var("TEST_MIGRATE_SRC") else {
            eprintln!("skip fame_money_skills_round_trip: TEST_MIGRATE_SRC unset");
            return;
        };
        let tgt = std::env::var("TEST_MIGRATE_TGT").unwrap_or_else(|_| format!("{}.target.db", src));
        std::fs::copy(&src, &tgt).expect("copy src -> tgt");
        let _ = std::fs::remove_file(format!("{}-wal", tgt));
        let _ = std::fs::remove_file(format!("{}-shm", tgt));

        let players = list_players_with_prisoner(&src).expect("list source players");
        assert!(!players.is_empty(), "source has no players with a prisoner");

        // Snapshot source expectations.
        let mut expect = std::collections::HashMap::new();
        for p in &players {
            let c = player_card(&src, p).expect("source card");
            let total_skill_xp: f64 = c["skills"].as_array().map(|a| {
                a.iter().filter_map(|s| s["xp"].as_f64()).sum()
            }).unwrap_or(0.0);
            expect.insert(p.clone(), (
                c["fame"].as_f64().unwrap_or(0.0),
                c["bank"].as_i64().unwrap_or(0),
                c["gold"].as_i64().unwrap_or(0),
                total_skill_xp,
            ));
        }

        // Zero fame, money, and skills in the target (simulate a wiped world the players rejoined).
        {
            let conn = open_readwrite(&tgt).expect("open target rw");
            conn.execute("UPDATE user_profile SET fame_points = 0", []).expect("zero fame");
            conn.execute("UPDATE bank_account_registry_currencies SET account_balance = 0", []).expect("zero money");
            conn.execute("UPDATE prisoner_skill SET level = 0, experience = 0", []).expect("zero skills");
        }
        // Prove the zeroing landed.
        for p in &players {
            let c = player_card(&tgt, p).expect("zeroed card");
            assert_eq!(c["bank"].as_i64().unwrap_or(-1), 0, "pre-migrate bank not zeroed for {}", p);
            assert_eq!(c["fame"].as_f64().unwrap_or(-1.0), 0.0, "pre-migrate fame not zeroed for {}", p);
        }

        // Run the shipping migration.
        let report = migrate_progression(&src, &tgt).expect("migrate");
        eprintln!("migrate report:\n{}", serde_json::to_string_pretty(&report).unwrap());
        assert_eq!(report["migrated"].as_u64().unwrap_or(0) as usize, players.len(), "not all players migrated");

        // Assert restoration.
        for p in &players {
            let (ef, eb, eg, exp_xp) = expect[p];
            let c = player_card(&tgt, p).expect("post-migrate card");
            assert_eq!(c["bank"].as_i64().unwrap_or(-1), eb, "bank not restored for {}", p);
            assert_eq!(c["gold"].as_i64().unwrap_or(-1), eg, "gold not restored for {}", p);
            let got_fame = c["fame"].as_f64().unwrap_or(-1.0);
            assert!((got_fame - ef).abs() < 0.01, "fame not restored for {}: {} != {}", p, got_fame, ef);
            let got_xp: f64 = c["skills"].as_array().map(|a| a.iter().filter_map(|s| s["xp"].as_f64()).sum()).unwrap_or(0.0);
            assert!((got_xp - exp_xp).abs() < 1.0, "skill xp not restored for {}: {} != {}", p, got_xp, exp_xp);
        }
        let _ = std::fs::remove_file(&tgt);
    }

    // Campaign restore: proves restore_pending is reconnect-gated (only restores players present in
    // the target) and idempotent (never re-restores a done player; picks up newly-reconnected ones).
    // Requires TEST_MIGRATE_SRC (a real clone). Skips silently otherwise.
    #[test]
    fn restore_pending_is_gated_and_idempotent() {
        use std::collections::HashSet;
        let Ok(src) = std::env::var("TEST_MIGRATE_SRC") else {
            eprintln!("skip restore_pending_is_gated_and_idempotent: TEST_MIGRATE_SRC unset");
            return;
        };
        let tgt = format!("{}.campaign.db", src);
        std::fs::copy(&src, &tgt).expect("copy");
        let _ = std::fs::remove_file(format!("{}-wal", tgt));
        let _ = std::fs::remove_file(format!("{}-shm", tgt));

        let all = list_players_with_prisoner(&src).expect("players");
        assert!(all.len() >= 3, "need >=3 players in the clone for this test");

        // Simulate a partial wipe+reconnect: only the first 2 players have "reconnected"
        // (keep their prisoner); detach the rest (prisoner_id NULL => not reconnected). Zero the
        // reconnected players so a successful restore is observable.
        let reconnected: Vec<String> = all.iter().take(2).cloned().collect();
        {
            let conn = open_readwrite(&tgt).expect("rw");
            let placeholders: Vec<String> = reconnected.iter().map(|s| format!("'{}'", s)).collect();
            conn.execute(&format!(
                "UPDATE user_profile SET prisoner_id = NULL WHERE CAST(user_id AS TEXT) NOT IN ({})",
                placeholders.join(",")), []).expect("detach");
            conn.execute("UPDATE user_profile SET fame_points = 0", []).expect("zero fame");
            conn.execute("UPDATE bank_account_registry_currencies SET account_balance = 0", []).expect("zero money");
        }

        // Pass 1: nothing done yet -> restores exactly the 2 reconnected players.
        let done: HashSet<String> = HashSet::new();
        let (r1, _sum1) = restore_pending(&src, &tgt, &done).expect("pass1");
        assert_eq!(r1.len(), 2, "pass1 should restore the 2 reconnected players, got {:?}", r1);
        // and their fame must now be non-zero (restored).
        for p in &reconnected {
            let c = player_card(&tgt, p).expect("card");
            assert!(c["fame"].as_f64().unwrap_or(0.0) > 0.0, "fame not restored for {}", p);
        }

        // Pass 2: mark pass-1 players done -> idempotent, restores nobody.
        let done: HashSet<String> = r1.iter().cloned().collect();
        let (r2, _sum2) = restore_pending(&src, &tgt, &done).expect("pass2");
        assert_eq!(r2.len(), 0, "pass2 must restore nobody (idempotent), got {:?}", r2);

        // Pass 3: a 3rd player reconnects -> restores exactly that one, not the done two.
        let third = all[2].clone();
        {
            let conn = open_readwrite(&tgt).expect("rw");
            let src_pid: i64 = {
                let sc = open_readonly(&src).expect("src ro");
                sc.query_row("SELECT prisoner_id FROM user_profile WHERE CAST(user_id AS TEXT)=?1",
                    rusqlite::params![third], |r| r.get(0)).expect("src pid")
            };
            conn.execute("UPDATE user_profile SET prisoner_id = ?1 WHERE CAST(user_id AS TEXT)=?2",
                rusqlite::params![src_pid, third]).expect("reconnect 3rd");
            conn.execute("UPDATE user_profile SET fame_points = 0 WHERE CAST(user_id AS TEXT)=?1",
                rusqlite::params![third]).expect("zero 3rd fame");
        }
        let (r3, _sum3) = restore_pending(&src, &tgt, &done).expect("pass3");
        assert_eq!(r3.len(), 1, "pass3 should restore only the newly-reconnected player, got {:?}", r3);
        assert_eq!(r3[0], third, "pass3 restored the wrong player");

        let _ = std::fs::remove_file(&tgt);
    }
}
