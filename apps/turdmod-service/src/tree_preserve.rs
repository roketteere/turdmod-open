// Tree preservation — at restart (PRE-START, SCUM stopped), delete
// `restorable_mesh_instance` rows (chopped trees) that fall inside any base's
// bounds, so cleared base areas STAY clear (no regrowth). @dep: engine::start_server.
// @ctx: restorable_mesh_instance = SCUM's chopped-foliage persistence layer; a row
// holds restore_interval/restore_timer. With no row, the RestorableMeshInstancesManager
// has nothing to restore, so that tree is permanently gone. Goal 1 (global slower
// regrowth) is a config key (RestorableMeshInstancesManagerRestoreTimeDilation), not here.
// @inv: pure SQLite, no foliage/HISM memory touched (which is crash-prone). Server-off only.

const DB: &str = r"C:\SCUMServer\SCUM\Saved\SaveFiles\SCUM.db";
const MARGIN: f64 = 5000.0; // buffer (cm) around each base's bounds

pub fn sweep_base_areas() {
    let conn = match rusqlite::Connection::open(DB) { Ok(c) => c, Err(_) => return };
    let mut bases: Vec<(f64, f64, f64, f64)> = Vec::new();
    if let Ok(mut s) = conn.prepare(
        "SELECT bounds_min_x,bounds_min_y,bounds_max_x,bounds_max_y FROM base WHERE bounds_min_x IS NOT NULL")
    {
        if let Ok(rows) = s.query_map([], |r| Ok((
            r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?))) {
            for r in rows.flatten() { bases.push(r); }
        }
    }
    if bases.is_empty() { return; }
    let mut deleted = 0usize;
    for (x0, y0, x1, y1) in &bases {
        if let Ok(n) = conn.execute(
            "DELETE FROM restorable_mesh_instance \
             WHERE location_x BETWEEN ?1 AND ?2 AND location_y BETWEEN ?3 AND ?4",
            rusqlite::params![x0 - MARGIN, x1 + MARGIN, y0 - MARGIN, y1 + MARGIN]) {
            deleted += n;
        }
    }
    if deleted > 0 {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        eprintln!("[tree_preserve] cleared {} chopped-tree records inside {} base area(s)", deleted, bases.len());
    }
}
