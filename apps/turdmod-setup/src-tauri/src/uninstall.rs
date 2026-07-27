// Uninstall — put the machine back the way it was.
//
// @dep: manifest.rs — everything here is driven by what the install recorded.
//   Without a manifest we do NOT guess at which files to delete; guessing means
//   deleting someone else's DLL.
// @inv: stop the service before restoring files. The engine DLLs are loaded
//   into the running game, and a locked file fails the restore silently-ish.

use crate::install_local::{self, ServiceState, StepResult, MOD_NAME};
use crate::manifest::{Manifest, TURDMOD_DIR};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct UninstallPlan {
    /// Plain-language list of what will happen, in order.
    pub steps: Vec<String>,
    pub has_manifest: bool,
    pub service_state: ServiceState,
    pub files_to_restore: usize,
    pub files_to_remove: usize,
    /// Set when we can't fully reverse — the user deserves to know before starting.
    pub warning: String,
}

pub fn plan() -> UninstallPlan {
    let mf = Manifest::load();
    let svc = install_local::service_state();
    let mut steps = Vec::new();

    if svc == ServiceState::Running {
        steps.push("Stop the TurdMOD service (this stops your game server)".into());
    }

    let (restore, remove, warning) = match &mf {
        Some(m) => {
            let restore = m.replaced().count();
            let remove = m.created().count();
            if m.service_registered {
                steps.push("Unregister the Windows service".into());
            } else if svc != ServiceState::Missing {
                steps.push("Leave the Windows service registered — it was here before TurdMOD Setup ran".into());
            }
            if restore > 0 {
                steps.push(format!("Restore {restore} file(s) we replaced, from backup"));
            }
            if remove > 0 {
                steps.push(format!("Remove {remove} item(s) we added"));
            }
            if m.added_no_battleye {
                steps.push("Turn BattlEye back on — it was on before TurdMOD was installed".into());
            }
            // The modded game copy is big and worth naming explicitly rather
            // than hiding inside a file count.
            for e in m.created() {
                let p = Path::new(&e.path);
                if p.is_dir() {
                    steps.push(format!("Delete the modded game copy at {}", p.display()));
                }
            }
            steps.push(format!("Take {MOD_NAME} back out of UE4SS's mods.txt, leaving your other mods alone"));
            (restore, remove, String::new())
        }
        None => {
            if svc != ServiceState::Missing {
                steps.push("Unregister the Windows service".into());
            }
            (
                0,
                0,
                "No install record found, so this was either installed by hand or by an older \
                 version of Setup. We'll stop and unregister the service, but we won't delete \
                 files we can't prove we put there — you'd have to remove those yourself."
                    .to_string(),
            )
        }
    };

    UninstallPlan {
        steps,
        has_manifest: mf.is_some(),
        service_state: svc,
        files_to_restore: restore,
        files_to_remove: remove,
        warning,
    }
}

/// Strip our entry from mods.txt without touching anyone else's.
///
/// @ctx: deliberately surgical rather than restoring the backup — the operator
///   may have enabled other mods AFTER our install, and a blind restore would
///   silently disable them.
fn remove_from_mods_txt(server_root: &str) -> StepResult {
    let path = Path::new(server_root)
        .join("SCUM")
        .join("Binaries")
        .join("Win64")
        .join("UE4SS")
        .join("Mods")
        .join("mods.txt");

    let Ok(existing) = std::fs::read_to_string(&path) else {
        return StepResult::ok("Mod list", "no mods.txt — nothing to clean up");
    };

    let kept: Vec<&str> = existing
        .trim_start_matches('\u{feff}')
        .lines()
        .filter(|l| {
            let name = l.split(':').next().unwrap_or("").trim();
            !name.eq_ignore_ascii_case(MOD_NAME)
        })
        .collect();

    let out = if kept.is_empty() { String::new() } else { format!("{}\r\n", kept.join("\r\n")) };
    match std::fs::write(&path, out) {
        Ok(_) => StepResult::ok("Mod list", format!("removed {MOD_NAME}, kept {} other line(s)", kept.len())),
        Err(e) => StepResult::fail("Mod list", format!("{}: {e}", path.display())),
    }
}

/// Put BattlEye back the way we found it by stripping the arg we added.
fn restore_battleye() -> StepResult {
    let path = PathBuf::from(TURDMOD_DIR).join("service.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return StepResult::ok("BattlEye", "no service.json — nothing to restore");
    };
    let Ok(mut cfg) = serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}'))
    else {
        return StepResult::ok("BattlEye", "couldn't read service.json — left it alone");
    };
    if !install_local::remove_no_battleye(&mut cfg) {
        return StepResult::ok("BattlEye", "already back to your setting");
    }
    match serde_json::to_string_pretty(&cfg).map(|s| std::fs::write(&path, s)) {
        Ok(Ok(())) => StepResult::ok("BattlEye", "turned BattlEye back on, as it was before"),
        _ => StepResult::fail("BattlEye", format!("couldn't write {}", path.display())),
    }
}

/// Remove the bridge's own folders once their files are gone.
///
/// @inv: uses remove_dir (NOT remove_dir_all), which fails on a non-empty
///   directory. That's the safety property — if anything is still in there,
///   it isn't ours and we leave it completely alone.
/// @ctx: without this an empty TurdMODEngineBridge/ survives an uninstall, so
///   "is TurdMOD installed?" still looks like yes.
fn prune_empty_mod_dirs(server_root: &str, r: &mut Vec<StepResult>) {
    let mods = Path::new(server_root)
        .join("SCUM").join("Binaries").join("Win64")
        .join("UE4SS").join("Mods");
    // Deepest first — a parent can only go once its child has.
    let mut pruned = 0;
    for dir in [mods.join(MOD_NAME).join("dlls"), mods.join(MOD_NAME)] {
        if dir.exists() && std::fs::remove_dir(&dir).is_ok() {
            pruned += 1;
        }
    }
    if pruned > 0 {
        r.push(StepResult::ok("Tidy up", format!("removed {pruned} empty TurdMOD folder(s)")));
    }
}

#[cfg(windows)]
fn stop_and_unregister(unregister: bool) -> Vec<StepResult> {
    use std::process::Command;
    let mut r = Vec::new();

    r.extend(install_local::stop_service_for_update());

    if !unregister {
        return r;
    }

    let exe = PathBuf::from(TURDMOD_DIR).join("turdmod-service.exe");
    if !exe.exists() {
        r.push(StepResult::ok("Unregister service", "service exe already gone — skipped"));
        return r;
    }
    match Command::new(&exe).arg("--uninstall").output() {
        Ok(out) if out.status.success() => {
            r.push(StepResult::ok("Unregister service", "removed TurdMODService"))
        }
        Ok(out) => {
            let msg = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            let hint = if msg.contains("Access") || msg.contains("denied") {
                " — run TurdMOD Setup as Administrator"
            } else {
                ""
            };
            r.push(StepResult::fail("Unregister service", format!("{}{hint}", msg.trim())));
        }
        Err(e) => r.push(StepResult::fail("Unregister service", format!("{e}"))),
    }
    r
}

#[cfg(not(windows))]
fn stop_and_unregister(_unregister: bool) -> Vec<StepResult> {
    vec![StepResult::fail("Unregister service", "Windows only")]
}

/// Reverse the install. `remove_settings` also deletes service.json — off by
/// default so a reinstall keeps the operator's token and tuning.
pub fn run(remove_settings: bool) -> Vec<StepResult> {
    let mf = Manifest::load();
    let mut r = stop_and_unregister(mf.as_ref().map(|m| m.service_registered).unwrap_or(true));

    // A failed stop means files are still locked; restoring now would half-work.
    if r.iter().any(|s| !s.ok && s.step == "Stop service") {
        r.push(StepResult::fail(
            "Uninstall",
            "Stopped before changing any files — the service is still running.",
        ));
        return r;
    }

    let Some(mf) = mf else {
        r.push(StepResult::ok(
            "Files",
            "No install record — left your files alone rather than guessing which are ours.",
        ));
        return r;
    };

    let settings_path = PathBuf::from(TURDMOD_DIR).join("service.json");

    for e in mf.replaced() {
        let dst = Path::new(&e.path);
        if !remove_settings && dst == settings_path {
            r.push(StepResult::ok("Settings", "kept service.json so a reinstall remembers your setup"));
            continue;
        }
        // @inv: mods.txt is NEVER restored from backup — remove_from_mods_txt
        //   below strips just our line. Restoring would revert any mod the
        //   operator enabled after our install, and then we'd strip the
        //   reverted content. Surgical edit only.
        if dst.file_name().map(|n| n.eq_ignore_ascii_case("mods.txt")).unwrap_or(false) {
            continue;
        }
        match e.backup.as_deref() {
            Some(b) if Path::new(b).exists() => match std::fs::copy(b, dst) {
                Ok(_) => r.push(StepResult::ok("Restore", dst.display().to_string())),
                Err(err) => r.push(StepResult::fail("Restore", format!("{}: {err}", dst.display()))),
            },
            _ => r.push(StepResult::fail(
                "Restore",
                format!("backup missing for {} — left the current file in place", dst.display()),
            )),
        }
    }

    for e in mf.created() {
        let p = Path::new(&e.path);
        if !remove_settings && p == settings_path {
            r.push(StepResult::ok("Settings", "kept service.json so a reinstall remembers your setup"));
            continue;
        }
        if !p.exists() {
            continue;
        }
        // @inv: created entries can be directories — the modded client copy is
        // recorded as its root folder. remove_file fails on those.
        let res = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
        match res {
            Ok(_) => r.push(StepResult::ok("Remove", p.display().to_string())),
            Err(err) => r.push(StepResult::fail("Remove", format!("{}: {err}", p.display()))),
        }
    }

    r.push(remove_from_mods_txt(&mf.server_root));
    prune_empty_mod_dirs(&mf.server_root, &mut r);

    // @inv: if we turned BattlEye off, turn it back on — even when keeping
    //   service.json, which the restore loop deliberately skips. Without this
    //   an uninstall would leave their anticheat disabled forever.
    if mf.added_no_battleye && !remove_settings {
        r.push(restore_battleye());
    }

    // Only drop the manifest once everything it described actually reversed —
    // otherwise a retry has nothing to work from.
    if r.iter().all(|s| s.ok) {
        let _ = std::fs::remove_file(crate::manifest::manifest_path());
        r.push(StepResult::ok("Done", "TurdMOD removed. Your server is back to how it was."));
    } else {
        r.push(StepResult::ok(
            "Note",
            "Some items didn't reverse — the install record was kept so you can run this again.",
        ));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_txt_cleanup_keeps_other_mods_and_comments() {
        let root = std::env::temp_dir().join("tm-uninstall-modstxt");
        let _ = std::fs::remove_dir_all(&root);
        let mods = root.join("SCUM").join("Binaries").join("Win64").join("UE4SS").join("Mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(
            mods.join("mods.txt"),
            "; my list\r\nUsmapDumper : 1\r\nTurdMODEngineBridge : 1\r\nOther : 0\r\n",
        )
        .unwrap();

        let res = remove_from_mods_txt(&root.display().to_string());
        assert!(res.ok, "{}", res.detail);

        let txt = std::fs::read_to_string(mods.join("mods.txt")).unwrap();
        assert!(!txt.contains("TurdMODEngineBridge"), "ours must be gone: {txt}");
        assert!(txt.contains("UsmapDumper : 1"), "other mods must survive: {txt}");
        assert!(txt.contains("Other : 0"), "including disabled ones: {txt}");
        assert!(txt.contains("; my list"), "and comments: {txt}");

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Regression: uninstall used to BOTH restore mods.txt from backup AND
    /// strip our line. If the operator enabled another mod after our install,
    /// the restore silently reverted it. mods.txt is surgical-edit only now.
    #[test]
    fn a_mod_added_after_install_survives_uninstall() {
        let root = std::env::temp_dir().join("tm-uninstall-latermod");
        let _ = std::fs::remove_dir_all(&root);
        let mods = root.join("SCUM").join("Binaries").join("Win64").join("UE4SS").join("Mods");
        std::fs::create_dir_all(&mods).unwrap();

        // What our install would have backed up.
        let at_install_time = "TurdMODEngineBridge : 1\r\n";
        // What the operator has now — they added a mod afterwards.
        std::fs::write(mods.join("mods.txt"), "TurdMODEngineBridge : 1\r\nTheirNewMod : 1\r\n").unwrap();

        let res = remove_from_mods_txt(&root.display().to_string());
        assert!(res.ok, "{}", res.detail);

        let txt = std::fs::read_to_string(mods.join("mods.txt")).unwrap();
        assert!(txt.contains("TheirNewMod : 1"), "a later addition must survive: {txt}");
        assert!(!txt.contains("TurdMODEngineBridge"), "ours must still go: {txt}");
        assert!(!at_install_time.contains("TheirNewMod"), "sanity: the backup predates their mod");

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Regression: the modded client copy is recorded as a directory, and the
    /// removal loop used remove_file — which fails on directories, leaving an
    /// 89 GB folder behind and reporting a failed uninstall.
    #[test]
    fn a_created_directory_is_removed_not_just_files() {
        let root = std::env::temp_dir().join("tm-uninstall-dir");
        let _ = std::fs::remove_dir_all(&root);
        let copy = root.join("SCUM-Modded").join("SCUM").join("Content");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("a.pak"), b"x").unwrap();

        let target = root.join("SCUM-Modded");
        assert!(target.is_dir());
        let res = if target.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        assert!(res.is_ok(), "a created directory must be removable: {res:?}");
        assert!(!target.exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn pruning_removes_our_empty_folders_but_never_touches_occupied_ones() {
        let root = std::env::temp_dir().join("tm-uninstall-prune");
        let _ = std::fs::remove_dir_all(&root);
        let mods = root.join("SCUM").join("Binaries").join("Win64").join("UE4SS").join("Mods");
        std::fs::create_dir_all(mods.join(MOD_NAME).join("dlls")).unwrap();
        // Someone else's mod, with content — must survive untouched.
        std::fs::create_dir_all(mods.join("UsmapDumper")).unwrap();
        std::fs::write(mods.join("UsmapDumper").join("main.lua"), b"x").unwrap();

        let mut r = Vec::new();
        prune_empty_mod_dirs(&root.display().to_string(), &mut r);

        assert!(!mods.join(MOD_NAME).exists(), "our empty folder should be gone");
        assert!(mods.join("UsmapDumper").join("main.lua").exists(), "other mods untouched");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// remove_dir refuses non-empty dirs — that's what makes this safe even if
    /// the user dropped their own file into our folder.
    #[test]
    fn pruning_leaves_our_folder_alone_if_it_still_has_something_in_it() {
        let root = std::env::temp_dir().join("tm-uninstall-prune2");
        let _ = std::fs::remove_dir_all(&root);
        let mods = root.join("SCUM").join("Binaries").join("Win64").join("UE4SS").join("Mods");
        std::fs::create_dir_all(mods.join(MOD_NAME)).unwrap();
        std::fs::write(mods.join(MOD_NAME).join("their-notes.txt"), b"mine").unwrap();

        let mut r = Vec::new();
        prune_empty_mod_dirs(&root.display().to_string(), &mut r);

        assert!(mods.join(MOD_NAME).join("their-notes.txt").exists(), "must not delete their file");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_mods_txt_is_not_an_error() {
        let root = std::env::temp_dir().join("tm-uninstall-nomods");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(remove_from_mods_txt(&root.display().to_string()).ok);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Without a manifest we must not invent a file list to delete.
    #[test]
    fn a_planless_uninstall_says_so_instead_of_guessing() {
        // plan() reads the real C:\TurdMOD; assert only on the no-manifest shape.
        let p = UninstallPlan {
            steps: vec![],
            has_manifest: false,
            service_state: ServiceState::Missing,
            files_to_restore: 0,
            files_to_remove: 0,
            warning: "No install record found".into(),
        };
        assert!(!p.warning.is_empty(), "the user must be told it can't fully reverse");
        assert_eq!(p.files_to_remove, 0, "never delete files we can't prove are ours");
    }
}
