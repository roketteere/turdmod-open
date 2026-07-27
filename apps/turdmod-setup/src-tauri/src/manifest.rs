// Install manifest — the record of every file we touched, so uninstall can put
// the machine back exactly as it was.
//
// @inv: nothing may create or overwrite a file in the user's install without
//   going through `before_write` first. A file changed off-manifest is a file
//   uninstall cannot restore — and silently eating someone's DLL is the worst
//   thing this app could do.
// @ctx: written to C:\TurdMOD\install-manifest.json; originals are copied to
//   C:\TurdMOD\backup\<epoch>\ under flattened names (avoids MAX_PATH and
//   cross-drive path mangling).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TURDMOD_DIR: &str = r"C:\TurdMOD";
const MANIFEST_NAME: &str = "install-manifest.json";
const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Didn't exist before us. Uninstall deletes it.
    Created,
    /// Existed; we replaced it. Uninstall restores the backup.
    Replaced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub action: Action,
    /// Absolute path of the saved original. Always set for `Replaced`.
    pub backup: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// Unix seconds — when this install ran.
    pub installed_at: u64,
    pub server_root: String,
    pub entries: Vec<Entry>,
    /// True only if WE registered the Windows service. If it was already there
    /// we leave it alone on uninstall — it may predate us.
    pub service_registered: bool,
    /// True if WE added -NoBattlEye to the operator's launch args. Uninstall
    /// takes it back out.
    /// @inv: only set when the operator had BattlEye ON. If it was already off
    ///   that was their choice and we must not "restore" it to on.
    #[serde(default)]
    pub added_no_battleye: bool,
    #[serde(skip)]
    backup_dir: Option<PathBuf>,
    /// Where backups go. Overridable so tests never write into the real
    /// C:\TurdMOD — an early version did exactly that and littered a live
    /// install with fixture files.
    #[serde(skip)]
    root: Option<PathBuf>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn manifest_path() -> PathBuf {
    PathBuf::from(TURDMOD_DIR).join(MANIFEST_NAME)
}

impl Manifest {
    pub fn new(server_root: &str) -> Self {
        Self {
            version: MANIFEST_VERSION,
            installed_at: now_secs(),
            server_root: server_root.to_string(),
            entries: Vec::new(),
            service_registered: false,
            added_no_battleye: false,
            backup_dir: None,
            root: None,
        }
    }

    /// Same, but backups land under `root`. Tests and the live harness only —
    /// keeps fixtures out of the real C:\TurdMOD.
    #[allow(dead_code)]
    pub fn new_in(server_root: &str, root: PathBuf) -> Self {
        let mut m = Self::new(server_root);
        m.root = Some(root);
        m
    }

    pub fn load() -> Option<Self> {
        let raw = std::fs::read_to_string(manifest_path()).ok()?;
        serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = manifest_path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, json).map_err(|e| format!("{}: {e}", p.display()))
    }

    /// Lazily created so a run that touches nothing leaves no empty folder.
    fn backup_dir(&mut self) -> Result<PathBuf, String> {
        if self.backup_dir.is_none() {
            let base = self.root.clone().unwrap_or_else(|| PathBuf::from(TURDMOD_DIR));
            let d = base.join("backup").join(self.installed_at.to_string());
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            self.backup_dir = Some(d);
        }
        Ok(self.backup_dir.clone().unwrap())
    }

    /// Call BEFORE writing `path`. Backs up any existing file and records what
    /// we're about to do.
    ///
    /// Returns Err only when an existing file could not be backed up — the
    /// caller must then refuse to write, because that write would be
    /// unrecoverable.
    pub fn before_write(&mut self, path: &Path) -> Result<(), String> {
        if self.entries.iter().any(|e| Path::new(&e.path) == path) {
            return Ok(()); // already recorded this run; the first backup is the real original
        }

        if !path.exists() {
            self.entries.push(Entry {
                path: path.display().to_string(),
                action: Action::Created,
                backup: None,
            });
            return Ok(());
        }

        let dir = self.backup_dir()?;
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let dest = dir.join(format!("{:04}_{}", self.entries.len(), name));
        std::fs::copy(path, &dest)
            .map_err(|e| format!("couldn't back up {}: {e}", path.display()))?;

        self.entries.push(Entry {
            path: path.display().to_string(),
            action: Action::Replaced,
            backup: Some(dest.display().to_string()),
        });
        Ok(())
    }

    /// Files we created, newest first — the order uninstall should remove them.
    pub fn created(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev().filter(|e| e.action == Action::Created)
    }

    pub fn replaced(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev().filter(|e| e.action == Action::Replaced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_new_file_is_recorded_as_created_with_no_backup() {
        let d = scratch("tm-manifest-created");
        let mut m = Manifest::new_in("root", d.clone());
        let f = d.join("brand-new.dll");
        m.before_write(&f).unwrap();

        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries[0].action, Action::Created);
        assert!(m.entries[0].backup.is_none(), "nothing existed to back up");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_existing_file_is_copied_before_being_recorded() {
        let d = scratch("tm-manifest-replaced");
        let f = d.join("theirs.dll");
        std::fs::write(&f, b"the user's original bytes").unwrap();

        let mut m = Manifest::new_in("root", d.clone());
        m.before_write(&f).unwrap();

        assert_eq!(m.entries[0].action, Action::Replaced);
        let backup = m.entries[0].backup.clone().expect("must have a backup");
        assert_eq!(
            std::fs::read(&backup).unwrap(),
            b"the user's original bytes",
            "the backup must hold the ORIGINAL content, byte for byte"
        );

        let _ = std::fs::remove_dir_all(&d);
    }

    /// Writing the same file twice in one run must not let the second backup
    /// (which is already our own content) overwrite the real original.
    #[test]
    fn the_first_backup_wins_when_a_file_is_written_twice() {
        let d = scratch("tm-manifest-twice");
        let f = d.join("twice.dll");
        std::fs::write(&f, b"ORIGINAL").unwrap();

        let mut m = Manifest::new_in("root", d.clone());
        m.before_write(&f).unwrap();
        std::fs::write(&f, b"ours-v1").unwrap();
        m.before_write(&f).unwrap();

        assert_eq!(m.entries.len(), 1, "one entry per path");
        let backup = m.entries[0].backup.clone().unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"ORIGINAL");

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn round_trips_through_json() {
        let mut m = Manifest::new(r"C:\Server");
        m.entries.push(Entry {
            path: r"C:\Server\a.dll".into(),
            action: Action::Created,
            backup: None,
        });
        m.service_registered = true;

        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.server_root, r"C:\Server");
        assert_eq!(back.entries.len(), 1);
        assert!(back.service_registered);
    }
}
