// SCUM install detection — Steam-first (registry + libraryfolders.vdf), with a
// fallback sweep of common library paths.
//
// Ported from turdmod-manager's scum_paths.rs, which is the proven detector.
// @inv: keep the two in sync if Steam's layout changes.

use serde::Serialize;
use std::path::{Path, PathBuf};

const SCUM_CLIENT_APPID: &str = "513710";
const SCUM_SERVER_APPID: &str = "3792580";
// 1077840 is the legacy server appid — still present in older library files.
const SCUM_SERVER_APPID_LEGACY: &str = "1077840";

const GAME_DIR_CANDIDATES: &[&str] = &["SCUM"];
const SERVER_DIR_CANDIDATES: &[&str] = &["SCUM Server", "SCUMDedicatedServer", "SCUM_Server"];

#[derive(Debug, Clone, Default, Serialize)]
pub struct DetectedInstalls {
    pub game: Option<String>,
    pub server: Option<String>,
    /// Every Steam library we looked in — useful for the "nothing found" UI.
    pub searched: Vec<String>,
}

#[cfg(windows)]
fn steam_path_from_registry() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Software\\Valve\\Steam").ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path.replace('/', "\\")))
}

#[cfg(not(windows))]
fn steam_path_from_registry() -> Option<PathBuf> {
    None
}

// Valve's libraryfolders.vdf is a quasi-JSON KeyValues format. We only need the
// "path" of any block whose apps map lists a SCUM appid — a line scan is plenty.
fn library_paths_with_scum(vdf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current_path: Option<String> = None;
    let mut has_scum = false;
    let mut depth = 0_i32;

    for raw in vdf.lines() {
        let line = raw.trim();
        if line == "{" {
            depth += 1;
            continue;
        }
        if line == "}" {
            depth -= 1;
            if depth == 1 {
                if has_scum {
                    if let Some(p) = current_path.take() {
                        out.push(PathBuf::from(p.replace("\\\\", "\\")));
                    }
                }
                current_path = None;
                has_scum = false;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("\"path\"") {
            if let Some(start) = rest.find('"') {
                let after = &rest[start + 1..];
                if let Some(end) = after.find('"') {
                    current_path = Some(after[..end].to_string());
                }
            }
        } else if line.starts_with(&format!("\"{SCUM_CLIENT_APPID}\""))
            || line.starts_with(&format!("\"{SCUM_SERVER_APPID}\""))
            || line.starts_with(&format!("\"{SCUM_SERVER_APPID_LEGACY}\""))
        {
            has_scum = true;
        }
    }
    out
}

fn fallback_libraries() -> Vec<PathBuf> {
    ["C:", "D:", "E:", "F:"]
        .iter()
        .flat_map(|d| {
            [
                PathBuf::from(format!(r"{d}\Program Files (x86)\Steam")),
                PathBuf::from(format!(r"{d}\SteamLibrary")),
                PathBuf::from(format!(r"{d}\Steam")),
                // Common non-Steam dedicated-server drops
                PathBuf::from(format!(r"{d}\SCUMServer")),
                PathBuf::from(format!(r"{d}\SCUM Server")),
            ]
        })
        .collect()
}

/// A SCUM install root has one of these markers.
fn is_scum_root(root: &Path) -> bool {
    if !root.is_dir() {
        return false;
    }
    if root.join("SCUM_Launcher.exe").exists() {
        return true;
    }
    let win64 = root.join("SCUM").join("Binaries").join("Win64");
    win64.join("SCUM.exe").exists() || win64.join("GameServer.exe").exists()
}

fn find_under(library: &Path, candidates: &[&str]) -> Option<PathBuf> {
    // Steam layout
    let common = library.join("steamapps").join("common");
    for name in candidates {
        let c = common.join(name);
        if is_scum_root(&c) {
            return Some(c);
        }
    }
    // Bare layout — the library path IS the install (manual server drops)
    if is_scum_root(library) {
        return Some(library.to_path_buf());
    }
    None
}

/// Scan every known location for a SCUM client and/or dedicated server.
pub fn detect_all() -> DetectedInstalls {
    let mut result = DetectedInstalls::default();
    let mut libraries: Vec<PathBuf> = Vec::new();

    if let Some(steam) = steam_path_from_registry() {
        let vdf = steam.join("steamapps").join("libraryfolders.vdf");
        if let Ok(contents) = std::fs::read_to_string(&vdf) {
            libraries.extend(library_paths_with_scum(&contents));
        }
        libraries.push(steam);
    }
    libraries.extend(fallback_libraries());

    for lib in &libraries {
        if !lib.exists() {
            continue;
        }
        result.searched.push(lib.display().to_string());

        if result.game.is_none() {
            if let Some(p) = find_under(lib, GAME_DIR_CANDIDATES) {
                result.game = Some(p.display().to_string());
            }
        }
        if result.server.is_none() {
            if let Some(p) = find_under(lib, SERVER_DIR_CANDIDATES) {
                result.server = Some(p.display().to_string());
            }
        }
        if result.game.is_some() && result.server.is_some() {
            break;
        }
    }
    result
}

/// Validate a user-picked folder — same marker check, exposed for the
/// "Browse for folder" path when auto-detect finds nothing.
pub fn validate_install_path(path: &str) -> bool {
    is_scum_root(Path::new(path))
}

/// The dedicated server exe inside an install root, if present.
pub fn server_exe_in(root: &str) -> Option<String> {
    let p = Path::new(root)
        .join("SCUM")
        .join("Binaries")
        .join("Win64")
        .join("GameServer.exe");
    p.exists().then(|| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_paths_with_scum() {
        let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"		"C:\\Program Files (x86)\\Steam"
        "apps"
        {
            "513710"		"12345"
        }
    }
    "1"
    {
        "path"		"D:\\SteamLibrary"
        "apps"
        {
            "440"		"999"
        }
    }
}
"#;
        let found = library_paths_with_scum(vdf);
        assert_eq!(found.len(), 1);
        assert!(found[0].display().to_string().contains("Program Files"));
    }

    #[test]
    fn ignores_libraries_without_scum() {
        let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"		"C:\\Steam"
        "apps"
        {
            "440"		"1"
        }
    }
}
"#;
        assert!(library_paths_with_scum(vdf).is_empty());
    }
}
