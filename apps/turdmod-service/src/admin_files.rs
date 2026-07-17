// Admin file I/O for the soft-RCON config surface — read/write SCUM's
// ServerSettings.ini + the user-list .ini files DIRECTLY (the service runs on
// the box, so no SSH needed; the manager's Tauri version goes over SFTP).
//
// @inv: filenames are validated against a strict allowlist before any path is
//   built — callers cannot escape the config dir. @dep: mirrors the manager's
//   admin_files.rs / admin_commands.rs (same INI keys + ini_get/ini_set logic)
//   so the web adminmap and the desktop manager edit settings identically.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

const SCUM_SECTION: &str = "[/Script/Scum.ScumGameMode]";

pub const ALLOWED_ADMIN_FILES: &[&str] = &[
    "BannedUsers.ini",
    "AdminUsers.ini",
    "WhitelistedUsers.ini",
    "SilencedUsers.ini",
    "ExclusiveUsers.ini",
    "ServerSettings.ini",
    "ServerSettingsAdminUsers.ini",
    "EconomyOverride.json", // trader economy: traders-unlimited-funds / -stock, prices, tradeables
];

pub fn validate_filename(filename: &str) -> Result<&str> {
    if ALLOWED_ADMIN_FILES.contains(&filename) {
        Ok(filename)
    } else {
        Err(anyhow!("filename '{}' is not in the admin-files allowlist", filename))
    }
}

// scum_server_exe = <root>\SCUM\Binaries\Win64\SCUMServer.exe
//   -> config dir = <root>\SCUM\Saved\Config\WindowsServer
fn config_dir(scum_server_exe: &str) -> Result<PathBuf> {
    let scum = Path::new(scum_server_exe)
        .parent() // Win64
        .and_then(|p| p.parent()) // Binaries
        .and_then(|p| p.parent()) // SCUM
        .ok_or_else(|| anyhow!("cannot derive SCUM dir from exe path '{}'", scum_server_exe))?;
    Ok(scum.join("Saved").join("Config").join("WindowsServer"))
}

pub fn admin_file_path(scum_server_exe: &str, filename: &str) -> Result<PathBuf> {
    validate_filename(filename)?;
    Ok(config_dir(scum_server_exe)?.join(filename))
}

pub fn read_admin_file(scum_server_exe: &str, filename: &str) -> Result<String> {
    let path = admin_file_path(scum_server_exe, filename)?;
    // Admin INIs are plain ASCII/UTF-8 (only SCUM .log files are UTF-16 LE).
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn write_admin_file(scum_server_exe: &str, filename: &str, contents: &str) -> Result<()> {
    let path = admin_file_path(scum_server_exe, filename)?;
    std::fs::write(&path, contents.as_bytes())
        .map_err(|e| anyhow!("write {}: {}", path.display(), e))
}

// --- ServerSettings.ini parse / patch (SCHEMA-AGNOSTIC) ---------------------
//
// @ctx: SCUM's ServerSettings.ini has hundreds of `scum.<Key>=<val>` lines
//   spread over descriptive sections ([General],[World],[Respawn],...). The
//   key set + section layout drift across SCUM builds, so we do NOT hardcode a
//   field map (the manager's old `[/Script/Scum.ScumGameMode]` + bAllow* keys
//   are already stale). Every key is `scum.`-prefixed and globally unique, so
//   we parse/patch by full key name, section-agnostic. The frontend chooses
//   which keys to surface as a friendly form; the raw editor covers the rest.

/// Parse every `scum.<Key>=<val>` line into a flat map (section-agnostic).
pub fn parse_all_scum(raw: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with(';') || t.starts_with('#') || t.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            let k = k.trim();
            if k.starts_with("scum.") {
                out.insert(k.to_string(), v.trim().to_string());
            }
        }
    }
    out
}

/// Replace the first `scum.<Key>=` line in place (section-agnostic); append to
/// the end if the key is absent. @inv: PRESERVES the file's line-ending style
/// (SCUM ships CRLF) — splitting on the detected newline instead of `.lines()`
/// avoids silently rewriting every \r\n to \n on each save.
pub fn set_scum_key(text: &str, key: &str, value: &str) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let had_trailing_newline = text.ends_with(newline);
    let mut lines: Vec<String> = text.split(newline).map(|l| l.to_string()).collect();
    // split() on a trailing newline yields a final empty segment — drop it so we
    // don't append after it, then restore the trailing newline at the end.
    if had_trailing_newline {
        lines.pop();
    }
    let prefix = format!("{}=", key);
    let mut found = false;
    for line in lines.iter_mut() {
        if line.trim().starts_with(&prefix) {
            *line = format!("{}={}", key, value);
            found = true;
            break;
        }
    }
    if !found {
        lines.push(format!("{}={}", key, value));
    }
    let mut result = lines.join(newline);
    if had_trailing_newline {
        result.push_str(newline);
    }
    result
}

/// Apply a {full-scum-key: value} patch section-agnostically. Only keys that
/// start with `scum.` are written (defensive — frontend sends real keys).
pub fn apply_scum_patch(raw: &str, patch: &std::collections::HashMap<String, String>) -> String {
    let mut text = raw.to_string();
    for (key, value) in patch {
        if key.starts_with("scum.") {
            text = set_scum_key(&text, key, value);
        }
    }
    text
}

// --- INI round-trip (preserve untouched lines) ------------------------------

pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section;
            continue;
        }
        if !in_section {
            continue;
        }
        let prefix = format!("{}=", key);
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let had_trailing_newline = text.ends_with('\n');

    let mut section_idx: Option<usize> = None;
    let mut key_idx: Option<usize> = None;
    let mut next_section_idx: Option<usize> = None;
    let mut in_section = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_section && next_section_idx.is_none() {
                next_section_idx = Some(i);
            }
            if trimmed == section {
                section_idx = Some(i);
                in_section = true;
            } else if in_section {
                in_section = false;
            }
            continue;
        }
        if in_section && key_idx.is_none() {
            let prefix = format!("{}=", key);
            if trimmed.starts_with(&prefix) {
                key_idx = Some(i);
            }
        }
    }

    let new_line = format!("{}={}", key, value);

    if let Some(ki) = key_idx {
        lines[ki] = new_line;
    } else if let Some(si) = section_idx {
        let insert_at = next_section_idx.unwrap_or(lines.len()).min(lines.len());
        let insert_at = (si + 1).min(insert_at);
        lines.insert(insert_at, new_line);
    } else {
        if !lines.is_empty() && lines.last().map(|l| !l.is_empty()).unwrap_or(false) {
            lines.push(String::new());
        }
        lines.push(section.to_string());
        lines.push(new_line);
    }

    let mut result = lines.join("\n");
    if had_trailing_newline {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str =
        "[/Script/Scum.ScumGameMode]\nscum.MaxPlayers=64\nscum.ServerName=My Server\n\n[OtherSection]\nfoo=bar\n";

    #[test]
    fn ini_get_finds_value() {
        assert_eq!(ini_get(SAMPLE, SCUM_SECTION, "scum.MaxPlayers"), Some("64".to_string()));
    }
    #[test]
    fn ini_set_updates_existing() {
        let r = ini_set(SAMPLE, SCUM_SECTION, "scum.MaxPlayers", "32");
        assert!(r.contains("scum.MaxPlayers=32") && !r.contains("scum.MaxPlayers=64") && r.contains("foo=bar"));
    }
    #[test]
    fn config_dir_derives() {
        let d = config_dir(r"C:\X\SCUM\Binaries\Win64\SCUMServer.exe").unwrap();
        assert!(d.ends_with(r"SCUM\Saved\Config\WindowsServer"));
    }
}
