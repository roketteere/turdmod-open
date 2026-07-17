// Admin file I/O — read/write SCUM config files via the FTP/SFTP transport.
//
// Security: filenames are validated against a strict allowlist before any
// remote path is constructed. Callers cannot escape the config directory.

use anyhow::{anyhow, Result};

use crate::server::{self, ServerProfile};

const REMOTE_CONFIG_SUBPATH: &str = "SCUM/Saved/Config/WindowsServer";

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
        Err(anyhow!(
            "filename '{}' is not in the admin-files allowlist",
            filename
        ))
    }
}

fn config_path(scum_root: &str, filename: &str) -> String {
    let base = format!("{}/{}", scum_root.trim_end_matches('/'), REMOTE_CONFIG_SUBPATH);
    if filename.is_empty() {
        base
    } else {
        format!("{}/{}", base, filename)
    }
}

pub async fn download_admin_file(
    profile: &ServerProfile,
    secret: Option<&str>,
    filename: &str,
) -> Result<String> {
    let path = config_path(&profile.scum_root, filename);
    let bytes = server::download_file(profile, secret, &path).await?;
    // Admin INIs are plain ASCII/UTF-8 — only SCUM .log files are UTF-16 LE.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn upload_admin_file(
    profile: &ServerProfile,
    secret: Option<&str>,
    filename: &str,
    contents: &str,
) -> Result<()> {
    let path = config_path(&profile.scum_root, filename);
    server::upload_file(profile, secret, &path, contents.as_bytes()).await
}

// INI round-trip helpers ----------------------------------------------------
//
// SCUM uses standard UE4 INI syntax:
//   [SectionHeader]
//   key=value
//   ; comment
//
// We preserve every line we don't touch.

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
        assert_eq!(
            ini_get(SAMPLE, "[/Script/Scum.ScumGameMode]", "scum.MaxPlayers"),
            Some("64".to_string())
        );
    }

    #[test]
    fn ini_set_updates_existing_key() {
        let result = ini_set(SAMPLE, "[/Script/Scum.ScumGameMode]", "scum.MaxPlayers", "32");
        assert!(result.contains("scum.MaxPlayers=32"));
        assert!(!result.contains("scum.MaxPlayers=64"));
        assert!(result.contains("foo=bar"));
    }

    #[test]
    fn ini_set_inserts_new_key_in_section() {
        let result = ini_set(SAMPLE, "[/Script/Scum.ScumGameMode]", "scum.ServerPassword", "secret");
        assert!(result.contains("scum.ServerPassword=secret"));
        assert!(result.contains("foo=bar"));
    }

    #[test]
    fn ini_set_appends_missing_section() {
        let result = ini_set(SAMPLE, "[/Script/Scum.NewSection]", "scum.Foo", "bar");
        assert!(result.contains("[/Script/Scum.NewSection]"));
        assert!(result.contains("scum.Foo=bar"));
    }
}
