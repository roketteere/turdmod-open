// Mod state — freeze flags + per-mod config I/O.
//
// mod-state.json sits alongside the mod subdirs:
//   <mods_dir>/mod-state.json  →  { "frozen": ["slug-a", "slug-b"] }
//
// mod.config.json sits inside each mod's directory:
//   <mods_dir>/<slug>/mod.config.json  →  { "key": value, ... }
//
// Manager owns both files. Companion reads mod-state.json at boot to
// skip frozen mods; per-mod config is read by the mod scripts themselves.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const STATE_FILENAME: &str = "mod-state.json";
const CONFIG_FILENAME: &str = "mod.config.json";
const MANIFEST_FILENAME: &str = "turdmod.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModState {
    #[serde(default)]
    pub frozen: Vec<String>,
}

pub fn read_mod_state(mods_dir: &Path) -> Result<ModState> {
    let path = mods_dir.join(STATE_FILENAME);
    if !path.exists() {
        return Ok(ModState::default());
    }
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", path.display()))
}

pub fn write_mod_state(mods_dir: &Path, state: &ModState) -> Result<()> {
    if !mods_dir.exists() {
        std::fs::create_dir_all(mods_dir)
            .with_context(|| format!("create {}", mods_dir.display()))?;
    }
    let path = mods_dir.join(STATE_FILENAME);
    let json = serde_json::to_vec_pretty(state).context("serialize mod-state")?;
    std::fs::write(&path, json)
        .with_context(|| format!("write {}", path.display()))
}

pub fn set_frozen(mods_dir: &Path, slug: &str, frozen: bool) -> Result<ModState> {
    let mut state = read_mod_state(mods_dir)?;
    state.frozen.retain(|s| s != slug);
    if frozen {
        state.frozen.push(slug.to_string());
        state.frozen.sort_unstable();
    }
    write_mod_state(mods_dir, &state)?;
    Ok(state)
}

pub fn read_mod_config(mods_dir: &Path, slug: &str) -> Result<serde_json::Value> {
    let path = mods_dir.join(slug).join(CONFIG_FILENAME);
    if !path.exists() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", path.display()))
}

pub fn write_mod_config(mods_dir: &Path, slug: &str, config: &serde_json::Value) -> Result<()> {
    let dir = mods_dir.join(slug);
    if !dir.exists() {
        anyhow::bail!("mod directory not found: {}", dir.display());
    }
    let path = dir.join(CONFIG_FILENAME);
    let json = serde_json::to_vec_pretty(config).context("serialize mod.config")?;
    std::fs::write(&path, json)
        .with_context(|| format!("write {}", path.display()))
}

pub fn read_mod_manifest(mods_dir: &Path, slug: &str) -> Result<serde_json::Value> {
    let path = mods_dir.join(slug).join(MANIFEST_FILENAME);
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", path.display()))
}
