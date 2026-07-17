//! SCUM data-extraction pipeline integration — Manager-side helpers.
//!
//! The actual extraction pipeline lives in the **sibling project** at
//! `C:/Development/Claude/scumdump/` (Node + TypeScript CLI, three phases:
//! A — live UE4SS reflection dump via bridge RPC; B — Dumper-7 SDK headers;
//! C — CUE4Parse pak content extraction). This module is the Manager's
//! integration layer — it discovers the sibling repo, reads its config +
//! per-build `_meta.json`, parses Steam's `appmanifest_3792580.acf` for
//! the current SCUM build id, and provides a shell-spawn helper for the
//! `dump_commands` Tauri layer to drive the CLI from the GUI.
//!
//! All Tauri commands live in `dump_commands.rs`; this file holds only
//! the data shapes + plumbing they share.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Path discovery
// ---------------------------------------------------------------------------

const DEFAULT_SCUMDUMP_ROOT: &str = "C:/Development/Claude/scumdump";
const SCUM_SERVER_STEAM_APPID: &str = "3792580";

/// Resolve the sibling scumdump repo root. Honors `$SCUMDUMP_ROOT`,
/// falls back to the canonical path. Returns `None` if neither exists.
pub fn scumdump_root() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = std::env::var("SCUMDUMP_ROOT")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain(std::iter::once(PathBuf::from(DEFAULT_SCUMDUMP_ROOT)))
        .collect();
    candidates.into_iter().find(|p| p.is_dir())
}

/// Path to `data/extracted/` under the scumdump root.
pub fn extracted_root() -> Option<PathBuf> {
    scumdump_root().map(|r| r.join("data").join("extracted"))
}

/// Path to scumdump's config file (carries the AES key and overrides).
pub fn config_path() -> Option<PathBuf> {
    scumdump_root().map(|r| r.join("scumdump.config.json"))
}

/// Pick the lexicographically newest `v*` directory under `extracted/`.
/// SCUM build ids are increasing integers so lexicographic == numeric.
pub fn latest_build_dir() -> Option<PathBuf> {
    let root = extracted_root()?;
    let mut best: Option<(String, PathBuf)> = None;
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('v') {
            continue;
        }
        match &best {
            None => best = Some((name, path)),
            Some((b, _)) if name.as_str() > b.as_str() => best = Some((name, path)),
            _ => {}
        }
    }
    best.map(|(_, p)| p)
}

// ---------------------------------------------------------------------------
// scumdump.config.json shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpConfig {
    #[serde(default)]
    pub aes_key: Option<String>,
}

impl DumpConfig {
    /// Read the sibling repo's config. Returns `None` if scumdump isn't
    /// installed; returns `Some(default)` if the file is malformed (we
    /// don't want a single typo in JSON to wedge the GUI).
    pub fn load() -> Option<Self> {
        let path = config_path()?;
        let raw = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&raw).ok().or(Some(Self { aes_key: None }))
    }

    /// Short fingerprint for the status display: `0x0B1F4E54…CA81`.
    pub fn aes_fingerprint(&self) -> Option<String> {
        let key = self.aes_key.as_deref()?;
        if key.len() < 12 {
            return Some(key.to_string());
        }
        Some(format!("{}…{}", &key[..10], &key[key.len() - 4..]))
    }
}

// ---------------------------------------------------------------------------
// _meta.json shape (per-build, sits inside data/extracted/v<build>/)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhaseACountResult {
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhaseAResults {
    // `alias` is deserialize-only — accepts the scumdump-on-disk key
    // `dumpAllClasses` but serializes back out as `classes` for the JS
    // layer (which uses the friendlier name). Using `rename` here would
    // round-trip the long key and break the front-end shape.
    #[serde(alias = "dumpAllClasses", default)]
    pub classes: PhaseACountResult,
    #[serde(alias = "dumpAllEnums", default)]
    pub enums: PhaseACountResult,
    #[serde(alias = "dumpAllStructs", default)]
    pub structs: PhaseACountResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhaseBMeta {
    #[serde(default)]
    pub dumped_at: Option<String>,
    #[serde(default)]
    pub file_count: u64,
    #[serde(default)]
    pub byte_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhaseCCategory {
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhaseCMeta {
    #[serde(default)]
    pub dumped_at: Option<String>,
    #[serde(default)]
    pub widgets: PhaseCCategory,
    #[serde(default)]
    pub datatables: PhaseCCategory,
    #[serde(default)]
    pub strings: PhaseCCategory,
    #[serde(default)]
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DumpMeta {
    #[serde(default)]
    pub dumped_at: Option<String>,
    #[serde(default)]
    pub scum_build: Option<String>,
    #[serde(default)]
    pub results: PhaseAResults,
    // Server-side Dumper-7 SDK (target = SCUMServer.exe). Existing.
    #[serde(default)]
    pub phase_b: Option<PhaseBMeta>,
    // Client-side Dumper-7 SDK (target = SCUM.exe). Added 2026-05-21
    // alongside scumdump's `phase-b-client` subcommand.
    #[serde(default)]
    pub phase_b_client: Option<PhaseBMeta>,
    #[serde(default)]
    pub phase_c: Option<PhaseCMeta>,
}

impl DumpMeta {
    pub fn load(build_dir: &Path) -> Option<Self> {
        let meta = build_dir.join("_meta.json");
        let raw = std::fs::read_to_string(meta).ok()?;
        serde_json::from_str(&raw).ok()
    }
}

// ---------------------------------------------------------------------------
// Steam build id discovery
// ---------------------------------------------------------------------------

/// Steam library candidate roots. We try the default `C:` install first;
/// future work could scan `libraryfolders.vdf` for non-default libraries.
const STEAM_LIBRARY_ROOTS: &[&str] = &[
    "C:/Program Files (x86)/Steam/steamapps",
    "C:/SteamLibrary/steamapps",
    "D:/Steam/steamapps",
    "D:/SteamLibrary/steamapps",
    "E:/Steam/steamapps",
    "E:/SteamLibrary/steamapps",
];

/// Read Steam's `appmanifest_<appid>.acf` for an arbitrary Steam app
/// and extract the `buildid` field. Returns `None` if the manifest
/// isn't found in any candidate Steam library, or if the field is
/// missing / malformed.
pub fn read_steam_buildid_for(app_id: &str) -> Option<String> {
    let manifest = STEAM_LIBRARY_ROOTS
        .iter()
        .map(|root| PathBuf::from(format!("{}/appmanifest_{}.acf", root, app_id)))
        .find(|p| p.is_file())?;
    let raw = std::fs::read_to_string(manifest).ok()?;
    // ACF is VDF-formatted; we want the line: `\t"buildid"\t\t"23128915"`
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"buildid\"") {
            let quoted = rest.trim();
            let bytes = quoted.as_bytes();
            if bytes.first() != Some(&b'"') {
                continue;
            }
            let value = &quoted[1..];
            if let Some(end) = value.find('"') {
                return Some(value[..end].to_string());
            }
        }
    }
    None
}

/// SCUM Dedicated Server build id. Convenience wrapper over
/// `read_steam_buildid_for`.
pub fn read_steam_buildid() -> Option<String> {
    read_steam_buildid_for(SCUM_SERVER_STEAM_APPID)
}

/// SCUM (client) Steam app id — different from the dedicated server.
pub const SCUM_CLIENT_STEAM_APPID: &str = "513710";

/// SCUM (client) build id from `appmanifest_513710.acf`.
pub fn read_steam_client_buildid() -> Option<String> {
    read_steam_buildid_for(SCUM_CLIENT_STEAM_APPID)
}

/// `scumdump` writes its build dirs as `v<buildid>`. Strip the `v` prefix
/// to compare against the Steam manifest's plain integer.
pub fn extracted_buildid(build_dir: &Path) -> Option<String> {
    let name = build_dir.file_name()?.to_string_lossy().into_owned();
    name.strip_prefix('v').map(|s| s.to_string())
}

pub fn scum_server_steam_appid() -> &'static str {
    SCUM_SERVER_STEAM_APPID
}

// ---------------------------------------------------------------------------
// Forensic archive — read the sibling scumdump's data/archive/keys.jsonl
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub ts: String,
    #[serde(default)]
    pub scum_build: Option<String>,
    #[serde(default)]
    pub scum_server_sha256: Option<String>,
    #[serde(default)]
    pub scum_exe_sha256: Option<String>,
    pub key_type: String,
    pub value: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}

pub fn archive_path() -> Option<PathBuf> {
    scumdump_root().map(|r| r.join("data").join("archive").join("keys.jsonl"))
}

// ---------------------------------------------------------------------------
// Diff report — matches phase-diff.ts's DiffReport shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffNameSet {
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub changed_count: u64,
    #[serde(default)]
    pub prev_count: u64,
    #[serde(default)]
    pub curr_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffPhaseA {
    #[serde(default)]
    pub classes: DiffNameSet,
    #[serde(default)]
    pub enums: DiffNameSet,
    #[serde(default)]
    pub structs: DiffNameSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffPhaseC {
    #[serde(default)]
    pub widgets: DiffNameSet,
    #[serde(default)]
    pub datatables: DiffNameSet,
    #[serde(default)]
    pub strings: DiffNameSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffReport {
    pub previous_build: String,
    pub current_build: String,
    pub computed_at: String,
    #[serde(default)]
    pub phase_a: Option<DiffPhaseA>,
    #[serde(default)]
    pub phase_b: Option<DiffNameSet>,
    #[serde(default)]
    pub phase_b_client: Option<DiffNameSet>,
    #[serde(default)]
    pub phase_c: Option<DiffPhaseC>,
    #[serde(default)]
    pub notes: Vec<String>,
}

pub fn read_diff_report(build_dir: &Path) -> Option<DiffReport> {
    let path = build_dir.join("_diff.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// List every `v<id>` build dir under data/extracted/, newest first
/// (lexicographic order; SCUM build ids are increasing integers so
/// this matches "newest first").
pub fn list_build_ids() -> Vec<String> {
    let Some(root) = extracted_root() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_prefix('v').map(str::to_string)
        })
        .collect();
    ids.sort_by(|a, b| b.cmp(a));
    ids
}

pub fn read_archive_entries() -> Vec<ArchiveEntry> {
    let Some(path) = archive_path() else { return Vec::new() };
    let Ok(raw) = std::fs::read_to_string(&path) else { return Vec::new() };
    let mut out: Vec<ArchiveEntry> = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<ArchiveEntry>(line) {
            out.push(entry);
        }
    }
    out
}
