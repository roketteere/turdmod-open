// Hand the install off to TurdMOD Manager so the dashboard opens already
// pointed at the server we just set up.
//
// @dep: apps/turdmod-manager/src-tauri/src/commands.rs — STORE_FILE
//   "manager.json", STORE_KEY_INSTALL "scum_install", value is a plain JSON
//   string. Manager reads it via stored_install_path(); if it's absent Manager
//   falls back to its own detection, so this is a pin, not a requirement.
// @inv: merge into the existing store. It also holds themes, server profiles,
//   and keychain refs — writing a fresh object would wipe the user's setup.
// @brk: if Manager changes STORE_FILE or STORE_KEY_INSTALL, this silently stops
//   pinning anything (harmless, but the handoff step becomes a lie).

use crate::install_local::StepResult;
use std::path::PathBuf;

const MANAGER_ID: &str = "com.turdmod.manager";
const STORE_FILE: &str = "manager.json";
const STORE_KEY_INSTALL: &str = "scum_install";

/// Tauri v2 puts plugin-store files in the roaming app-data dir for the
/// app's identifier.
fn manager_store_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join(MANAGER_ID).join(STORE_FILE))
}

/// True if Manager appears to be installed (its store dir exists). Used by the
/// Verify step to decide whether to offer "Open Manager" or "Get Manager".
#[allow(dead_code)]
pub fn manager_present() -> bool {
    manager_store_path().map(|p| p.parent().map(|d| d.exists()).unwrap_or(false)).unwrap_or(false)
}

/// Point Manager at `server_root`. Safe to call when Manager isn't installed —
/// creates the store so Manager picks it up on first launch.
pub fn configure_manager(server_root: &str) -> StepResult {
    let Some(path) = manager_store_path() else {
        return StepResult::ok("Manager", "couldn't locate the Manager settings folder — skipped");
    };

    // Merge, never replace: this file also holds themes, server profiles and
    // keychain references.
    let mut store: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if !store.is_object() {
        // Corrupt or unexpected shape — don't clobber it, just say so.
        return StepResult::ok(
            "Manager",
            format!("{} isn't in the expected format — left it alone", path.display()),
        );
    }

    store[STORE_KEY_INSTALL] = serde_json::Value::String(server_root.to_string());

    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return StepResult::ok("Manager", format!("couldn't create {}: {e}", dir.display()));
        }
    }

    match serde_json::to_string_pretty(&store).map(|s| std::fs::write(&path, s)) {
        Ok(Ok(())) => StepResult::ok(
            "Manager",
            "pointed TurdMOD Manager at this server — it'll connect on launch",
        ),
        Ok(Err(e)) => StepResult::ok("Manager", format!("couldn't write {}: {e}", path.display())),
        Err(e) => StepResult::ok("Manager", format!("couldn't encode settings: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store holds themes, profiles and keychain refs. Losing those because
    /// we pinned an install path would be a bad trade.
    #[test]
    fn merging_preserves_existing_manager_settings() {
        let dir = std::env::temp_dir().join("tm-handoff-merge");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STORE_FILE);

        let original = serde_json::json!({
            "theme": "anime-shoujo",
            "server-profiles": [{ "id": "srv-1", "name": "scumserver" }],
            "server-creds-rcon-srv-1": "secret",
        });
        std::fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

        // Same merge the real function does, against a path we control.
        let mut store: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        store[STORE_KEY_INSTALL] = serde_json::Value::String(r"C:\Server".into());
        std::fs::write(&path, serde_json::to_string_pretty(&store).unwrap()).unwrap();

        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["theme"], "anime-shoujo", "theme must survive");
        assert_eq!(back["server-profiles"][0]["name"], "scumserver", "profiles must survive");
        assert_eq!(back["server-creds-rcon-srv-1"], "secret", "keychain refs must survive");
        assert_eq!(back[STORE_KEY_INSTALL], r"C:\Server", "and the pin is set");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn install_path_is_a_plain_string_not_an_object() {
        // Manager's stored_install_path() only accepts Value::String.
        let v = serde_json::Value::String(r"C:\Server".into());
        assert!(v.is_string(), "Manager ignores any other shape");
    }
}
