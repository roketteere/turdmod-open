// Vehicle durability — toughen loaded vehicle parts so they take less damage.
// Now CONFIG-DRIVEN: per-family _linearEnergyAbsorption values read from
// C:\TurdMOD\data\vehicle_durability.json (the TMM Vehicle Manager tab edits it).
// On first run (no file) it seeds all known families at DEFAULT_VALUE so the tab
// has values to show/edit.
// @ctx: a part's UClass/instances only exist in memory when the vehicle is LOADED
// (a player near it), so we poll: writeActorProperty on each instance (toughens it
// now) + writeClassDefault per class once (future-constructed parts inherit it).
// @inv: stock _linearEnergyAbsorption=0.2 (takes 80%); higher V => less damage taken
//   (%less = 1 - (1-V)/0.8). @dep findInstancesByClass + writeActorProperty +
//   writeClassDefault; config file backed by /data/file. [[registry]].

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

// Default seed list — findInstancesByClass PREFIX-matches each family's loaded PARTS.
const FAMILIES: &[&str] = &[
    "BPC_Rager_", "BPC_Laika_", "BPC_WolfsWagen_", "BPC_Barba_", "BPC_Cruiser_",
    "BPC_RIS_", "BPC_Tractor_", "BPC_SidecarBike_", "BPC_Dirtbike_", "BPC_MountainBike_",
    "BPC_CityBike_", "BPC_Kinglet_Duster_", "BPC_Kinglet_Mariner_", "BPC_Dinghy_", "BPC_SUP_",
];
const PROP: &str = "_linearEnergyAbsorption";
const DEFAULT_VALUE: f64 = 0.92; // ~90% less damage taken vs stock (stock 0.2)
const CONFIG_PATH: &str = r"C:\TurdMOD\data\vehicle_durability.json";
const INTERVAL: Duration = Duration::from_secs(90);

/// Load the per-family absorption config (family-prefix -> value). Seeds the file
/// with all known families @ DEFAULT_VALUE on first run so the tab has data.
fn load_config() -> BTreeMap<String, f64> {
    if let Ok(s) = std::fs::read_to_string(CONFIG_PATH) {
        if let Ok(m) = serde_json::from_str::<BTreeMap<String, f64>>(&s) {
            if !m.is_empty() { return m; }
        }
    }
    let m: BTreeMap<String, f64> = FAMILIES.iter().map(|f| (f.to_string(), DEFAULT_VALUE)).collect();
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(j) = serde_json::to_string_pretty(&m) { let _ = std::fs::write(CONFIG_PATH, j); }
    m
}

async fn bridge_ready() -> bool {
    pipe_rpc::call("ping", None).await.ok()
        .and_then(|v| v.get("pong").and_then(|p| p.as_bool()))
        .unwrap_or(false)
}

fn rpc_ok(r: &anyhow::Result<serde_json::Value>) -> bool {
    matches!(r, Ok(v) if v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false))
}

// Config changes need fresh CDO writes — when the file's value for a class changes,
// we must re-apply. We track (class -> applied value) so an edited value re-applies.
pub struct VehicleDurability { cdo_done: Mutex<HashSet<String>>, applied: Mutex<BTreeMap<String, f64>> }
impl VehicleDurability {
    pub fn new() -> Self { Self { cdo_done: Mutex::new(HashSet::new()), applied: Mutex::new(BTreeMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for VehicleDurability {
    fn name(&self) -> &'static str { "vehicle_durability" }
    fn interval(&self) -> Option<Duration> { Some(INTERVAL) }

    async fn tick(&self, _ctx: &ModCtx) {
        if !bridge_ready().await { return; }
        let config = load_config();
        let mut done = self.cdo_done.lock().await;
        let mut applied = self.applied.lock().await;
        let (parts, cdos) = apply_to_loaded(&config, &mut done, &mut applied).await;
        if parts > 0 || cdos > 0 {
            tracing::info!("vehicle_durability: toughened {} loaded parts, {} CDOs ({} families configured)", parts, cdos, config.len());
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}

async fn apply_to_loaded(
    config: &BTreeMap<String, f64>,
    cdo_done: &mut HashSet<String>,
    applied: &mut BTreeMap<String, f64>,
) -> (usize, usize) {
    let mut part_count = 0usize;
    let mut new_cdo = 0usize;
    for (family, fvalue) in config {
        let value = format!("{}", fvalue);
        let actor_class = format!("{}C", family); // BPC_Rager_C — the vehicle actor, skip (not a part)
        let find = pipe_rpc::call("findInstancesByClass", Some(serde_json::json!({
            "class": family, "limit": 300
        }))).await;
        let Ok(resp) = find else { continue; };
        let Some(instances) = resp.get("instances").and_then(|v| v.as_array()) else { continue; };

        let mut classes_seen: HashSet<String> = HashSet::new();
        for inst in instances {
            let class = inst.get("class").and_then(|v| v.as_str()).unwrap_or("");
            let ptr = inst.get("ptr").and_then(|v| v.as_str()).unwrap_or("");
            if class.is_empty() || ptr.is_empty() || class == actor_class { continue; }
            // Skip classes already at this exact value (CDO covers future parts). Re-apply
            // when the configured value changed (tab edit) — that's `applied != fvalue`.
            if cdo_done.contains(class) && applied.get(class) == Some(fvalue) { continue; }
            let r = pipe_rpc::call("writeActorProperty", Some(serde_json::json!({
                "ptr": ptr, "propertyName": PROP, "value": value, "valueKind": "float"
            }))).await;
            if rpc_ok(&r) { part_count += 1; }
            classes_seen.insert(class.to_string());
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        for class in classes_seen {
            if cdo_done.contains(&class) && applied.get(&class) == Some(fvalue) { continue; }
            let r = pipe_rpc::call("writeClassDefault", Some(serde_json::json!({
                "name": class, "propertyName": PROP, "valueKind": "float", "value": value
            }))).await;
            if rpc_ok(&r) { cdo_done.insert(class.clone()); applied.insert(class, *fvalue); new_cdo += 1; }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    (part_count, new_cdo)
}
