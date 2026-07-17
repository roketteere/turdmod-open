// SCUM install + mods-dir auto-detection.
//
// Detection strategy is Steam-first because that's how 99% of users get the
// game. We fall back to a list of common library paths only if the registry
// lookup fails or the parsed libraryfolders.vdf doesn't list SCUM.

use std::path::{Path, PathBuf};

const SCUM_STEAM_APPID: &str = "513710";
const SCUM_SERVER_STEAM_APPID: &str = "1077840";
const MODS_DIR_NAME: &str = "turdmod-mods";

// On disk, SCUM and the dedicated server share an internal layout but live
// in differently-named library subdirectories. The game's directory is
// usually just "SCUM"; the dedicated server's is "SCUM Server" (note the
// space — installed via Steam appid 1077840).
const GAME_DIR_CANDIDATES: &[&str] = &["SCUM"];
const SERVER_DIR_CANDIDATES: &[&str] = &["SCUM Server", "SCUMDedicatedServer", "SCUM_Server"];

/// Both fields `None` when neither install is found.
#[derive(Debug, Clone, Default)]
pub struct DetectedInstalls {
    pub game: Option<PathBuf>,
    pub server: Option<PathBuf>,
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

// Parse Valve's libraryfolders.vdf — a quasi-JSON KeyValues format.
// We only care about the "path" field of any block whose "apps" map
// contains the SCUM appid. A heuristic line scan is plenty here; full
// VDF parsing is overkill for the two fields we want.
fn library_paths_with_scum(libraryfolders_vdf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_block_has_scum = false;
    let mut depth = 0_i32;

    for raw in libraryfolders_vdf.lines() {
        let line = raw.trim();
        if line == "{" {
            depth += 1;
            continue;
        }
        if line == "}" {
            depth -= 1;
            // Closing a top-level library block: commit if it had SCUM.
            if depth == 1 {
                if current_block_has_scum {
                    if let Some(p) = current_path.take() {
                        out.push(PathBuf::from(p.replace("\\\\", "\\")));
                    }
                }
                current_path = None;
                current_block_has_scum = false;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("\"path\"") {
            // "path" "C:\\..."
            if let Some(start) = rest.find('"') {
                let after = &rest[start + 1..];
                if let Some(end) = after.find('"') {
                    current_path = Some(after[..end].to_string());
                }
            }
        } else if line.starts_with(&format!("\"{}\"", SCUM_STEAM_APPID))
            || line.starts_with(&format!("\"{}\"", SCUM_SERVER_STEAM_APPID))
        {
            current_block_has_scum = true;
        }
    }

    out
}

fn fallback_steam_libraries() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        PathBuf::from(r"C:\Steam"),
        PathBuf::from(r"D:\SteamLibrary"),
        PathBuf::from(r"D:\Steam"),
        PathBuf::from(r"E:\SteamLibrary"),
        PathBuf::from(r"E:\Steam"),
        PathBuf::from(r"F:\SteamLibrary"),
        PathBuf::from(r"F:\Steam"),
    ]
}

// A SCUM install root looks like one of:
//   <root>/SCUM_Launcher.exe                       (client install root marker)
//   <root>/SCUM/Binaries/Win64/SCUM.exe            (client install — nested exe)
//   <root>/SCUM/Binaries/Win64/SCUMServer.exe      (dedicated-server install)
fn is_scum_install_root(root: &Path) -> bool {
    if !root.is_dir() {
        return false;
    }
    if root.join("SCUM_Launcher.exe").exists() {
        return true;
    }
    let win64 = root.join("SCUM").join("Binaries").join("Win64");
    win64.join("SCUM.exe").exists() || win64.join("SCUMServer.exe").exists()
}

fn game_install_under_library(library: &Path) -> Option<PathBuf> {
    let common = library.join("steamapps").join("common");
    for name in GAME_DIR_CANDIDATES {
        let candidate = common.join(name);
        if is_scum_install_root(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn server_install_under_library(library: &Path) -> Option<PathBuf> {
    let common = library.join("steamapps").join("common");
    for name in SERVER_DIR_CANDIDATES {
        let candidate = common.join(name);
        if is_scum_install_root(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Detect both the SCUM client and the SCUM Dedicated Server across all
/// Steam libraries in one pass. Either or both fields may be `None`.
pub fn detect_all_installs() -> DetectedInstalls {
    if !cfg!(windows) {
        tracing::info!("scum_paths: non-Windows host, skipping registry/Steam detection");
        return DetectedInstalls::default();
    }

    let mut result = DetectedInstalls::default();
    let mut libraries: Vec<PathBuf> = Vec::new();

    if let Some(steam) = steam_path_from_registry() {
        let vdf = steam.join("steamapps").join("libraryfolders.vdf");
        if let Ok(contents) = std::fs::read_to_string(&vdf) {
            libraries.extend(library_paths_with_scum(&contents));
        }
        libraries.push(steam);
    }
    libraries.extend(fallback_steam_libraries());

    for lib in &libraries {
        if result.game.is_none() {
            if let Some(p) = game_install_under_library(lib) {
                tracing::info!(path = %p.display(), "scum_paths: found game install");
                result.game = Some(p);
            }
        }
        if result.server.is_none() {
            if let Some(p) = server_install_under_library(lib) {
                tracing::info!(path = %p.display(), "scum_paths: found server install");
                result.server = Some(p);
            }
        }
        if result.game.is_some() && result.server.is_some() {
            break;
        }
    }

    if result.game.is_none() && result.server.is_none() {
        tracing::debug!("scum_paths: no SCUM install detected");
    }
    result
}

/// Backwards-compat shim. Prefers the server install when both exist.
#[allow(dead_code)]
pub fn detect_scum_install() -> Option<PathBuf> {
    let installs = detect_all_installs();
    installs.server.or(installs.game)
}

pub fn detect_mods_dir(install: &Path) -> PathBuf {
    let dir = install
        .join("SCUM")
        .join("Binaries")
        .join("Win64")
        .join(MODS_DIR_NAME);
    if !dir.exists() {
        if let Err(err) = std::fs::create_dir_all(&dir) {
            tracing::warn!(err = %err, dir = %dir.display(), "scum_paths: failed to create mods dir");
        } else {
            tracing::info!(dir = %dir.display(), "scum_paths: created mods dir");
        }
    }
    dir
}

pub fn is_battleye_disabled(install: &Path) -> Option<bool> {
    if !cfg!(windows) {
        return None;
    }
    let be_dir = install.join("SCUM").join("Binaries").join("Win64").join("BattlEye");
    if !be_dir.exists() {
        // Stripped — BE definitively not present.
        return Some(true);
    }
    // Dir exists; if it's empty, treat as stripped/disabled.
    match std::fs::read_dir(&be_dir) {
        Ok(mut iter) => {
            if iter.next().is_none() {
                Some(true)
            } else {
                Some(false)
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vdf_with_scum_library() {
        let vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"      "C:\\Program Files (x86)\\Steam"
        "apps"
        {
            "513710"        "12345"
            "440"           "100"
        }
    }
    "1"
    {
        "path"      "D:\\SteamLibrary"
        "apps"
        {
            "730"           "100"
        }
    }
}
"#;
        let libs = library_paths_with_scum(vdf);
        assert_eq!(libs.len(), 1);
        assert_eq!(libs[0], PathBuf::from(r"C:\Program Files (x86)\Steam"));
    }
}
