// Modded client — build an ISOLATED copy of the game, never touch the Steam one.
//
// @ctx: the two-copy design is what keeps Steam's Play button safe. Vanilla
//   stays pristine (BattlEye on, official servers); the modded copy is what the
//   Launcher injects into. Modding the Steam install in place has already cost
//   one full steam://validate redownload — see reference_modded_client_isolated_copy.
//
// @inv: we NEVER ship game files. Everything here copies from the user's own
//   install. Redistributing SCUM content is the line we don't cross.
//
// ── Why hardlinks, and the sharp edge ──────────────────────────────────────
// Measured on a real install: 98.6% of the 89 GB is .pak/.ucas/.utoc/.sig —
// immutable content the game only ever reads. Hardlinking those costs zero
// bytes and is instant; the remaining ~1.2 GB is a real copy. 89 GB -> 1.2 GB.
//
// @brk: writing THROUGH a hardlink mutates the original — verified: a 10 MB
//   source truncated to 16 bytes when the link was written in place. That would
//   corrupt the user's vanilla install, the exact disaster this design prevents.
//   So: only provably-immutable extensions are ever linked, and any later
//   replacement must delete-then-create (verified safe) rather than write in
//   place. Adding a NEW file alongside links is safe (verified).
// @inv: hardlinks are same-volume only (verified: cross-volume is refused).

use crate::install_local::StepResult;
use crate::manifest::Manifest;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Extensions the game only reads. Anything not on this list gets a real copy,
/// because the game writes plenty inside its own folder (UE4SS.log, crash
/// dumps, imgui.ini, shader caches).
const IMMUTABLE_EXTS: &[&str] = &["pak", "ucas", "utoc", "sig"];

/// Folders left out of the modded copy entirely.
///
/// @inv: BattlEye must be OFF on the modded side. Not copying it is the
///   strongest form of that — the copy physically cannot start BE, so there's
///   no flag to get out of sync and nothing to accidentally re-enable. The
///   Steam install keeps its own BattlEye untouched, which is what lets the
///   Play button still work on official servers.
/// @dep: turdmod-loader/launcher::scum_install_has_battleye looks for a folder
///   named exactly this next to SCUM.exe.
const EXCLUDED_DIRS: &[&str] = &["BattlEye"];

fn is_excluded_dir(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| EXCLUDED_DIRS.iter().any(|x| x.eq_ignore_ascii_case(n)))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize)]
pub struct DriveInfo {
    /// "C:" etc.
    pub name: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
    /// Enough room for the real-copy portion (and the whole thing if we can't link).
    pub fits: bool,
    /// Same volume as the game, so paks can be hardlinked — near-instant, ~no space.
    pub can_hardlink: bool,
    /// Plain-language sizing for this choice.
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientPlan {
    pub source: String,
    pub total_bytes: u64,
    /// Portion that can be hardlinked when the destination shares the volume.
    pub linkable_bytes: u64,
    /// Portion that is always a real copy.
    pub copy_bytes: u64,
    pub file_count: usize,
    pub drives: Vec<DriveInfo>,
}

fn is_immutable(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMMUTABLE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

fn volume_of(p: &Path) -> Option<String> {
    p.components().next().and_then(|c| {
        let s = c.as_os_str().to_string_lossy().to_uppercase();
        // "C:" from "C:\..."
        s.split(':').next().map(|d| format!("{d}:"))
    })
}

fn human(bytes: u64) -> String {
    let gb = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

/// Walk the install and total up what would be linked vs copied.
fn measure(source: &Path) -> (u64, u64, usize) {
    let mut linkable = 0u64;
    let mut copy = 0u64;
    let mut count = 0usize;
    let mut stack = vec![source.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    if !is_excluded_dir(&p) {
                        stack.push(p);
                    }
                }
                Ok(t) if t.is_file() => {
                    let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                    count += 1;
                    if is_immutable(&p) {
                        linkable += len;
                    } else {
                        copy += len;
                    }
                }
                _ => {}
            }
        }
    }
    (linkable, copy, count)
}

/// All fixed drives with free/total bytes, in ONE call.
///
/// @ctx: wmic is gone on current Windows 11 — it returned nothing here, which
///   made the drive list silently empty. PowerShell's Get-PSDrive is present on
///   every target this app supports, and one invocation beats one per letter.
#[cfg(windows)]
fn all_drives() -> Vec<(String, u64, u64)> {
    use std::process::Command;
    let script = "Get-PSDrive -PSProvider FileSystem | Where-Object { $null -ne $_.Used } | \
                  ForEach-Object { \"$($_.Name)|$($_.Free)|$($_.Used)\" }";
    let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    else {
        return Vec::new();
    };

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().split('|');
            let name = parts.next()?.trim();
            let free: u64 = parts.next()?.trim().parse().ok()?;
            let used: u64 = parts.next()?.trim().parse().ok()?;
            if name.len() != 1 {
                return None; // skip named PSDrives that aren't disks
            }
            Some((format!("{}:", name.to_uppercase()), free, free + used))
        })
        .collect()
}

#[cfg(not(windows))]
fn all_drives() -> Vec<(String, u64, u64)> {
    Vec::new()
}

/// What it would take to build a modded copy, and where it could go.
pub fn plan(source: &str) -> ClientPlan {
    let src = PathBuf::from(source);
    let (linkable, copy, count) = measure(&src);
    let total = linkable + copy;
    let src_vol = volume_of(&src).unwrap_or_default();

    let mut drives = Vec::new();
    for (letter, free, size) in all_drives() {
        if size == 0 {
            continue;
        }
        let can_hardlink = letter == src_vol;
        // Same volume: only the non-linkable part costs space. Different
        // volume: the whole install has to be duplicated.
        let needed = if can_hardlink { copy } else { total };
        // Leave headroom so we don't fill the disk to the last byte.
        let fits = free > needed.saturating_add(2 * 1024 * 1024 * 1024);

        let note = if !fits {
            format!("needs {}, only {} free", human(needed), human(free))
        } else if can_hardlink {
            format!(
                "same drive as your game — uses only {} and takes seconds ({} shared, not duplicated)",
                human(copy),
                human(linkable)
            )
        } else {
            format!("full copy — {} used, and it'll take a while", human(needed))
        };

        drives.push(DriveInfo { name: letter, free_bytes: free, total_bytes: size, fits, can_hardlink, note });
    }

    ClientPlan {
        source: source.to_string(),
        total_bytes: total,
        linkable_bytes: linkable,
        copy_bytes: copy,
        file_count: count,
        drives,
    }
}

/// Build the isolated modded copy at `dest`.
///
/// @inv: hardlink only IMMUTABLE_EXTS, and only when dest shares the source
///   volume. Everything else is a real copy, so anything the game writes is
///   private to this copy and can never reach the vanilla install.
pub fn create_modded_copy(source: &str, dest: &str, mf: &mut Manifest) -> Vec<StepResult> {
    let src = PathBuf::from(source);
    let dst = PathBuf::from(dest);
    let mut r = Vec::new();

    if !src.join("SCUM").join("Binaries").join("Win64").join("SCUM.exe").exists() {
        r.push(StepResult::fail("Modded copy", format!("{source} doesn't look like a SCUM client install")));
        return r;
    }
    if dst.exists() && std::fs::read_dir(&dst).map(|mut d| d.next().is_some()).unwrap_or(false) {
        r.push(StepResult::fail(
            "Modded copy",
            format!("{dest} already exists and isn't empty — pick another folder or remove it first"),
        ));
        return r;
    }

    let same_volume = volume_of(&src) == volume_of(&dst);
    let mut linked = 0usize;
    let mut copied = 0usize;
    let mut bytes_copied = 0u64;
    let mut failures = 0usize;
    let mut skipped_battleye = false;
    let mut first_error = String::new();

    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(rel) = p.strip_prefix(&src) else { continue };
            let target = dst.join(rel);

            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    // BattlEye never makes it into the modded copy.
                    if is_excluded_dir(&p) {
                        skipped_battleye = true;
                        continue;
                    }
                    let _ = std::fs::create_dir_all(&target);
                    stack.push(p);
                }
                Ok(t) if t.is_file() => {
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let use_link = same_volume && is_immutable(&p);
                    let res = if use_link {
                        std::fs::hard_link(&p, &target).map(|_| { linked += 1; })
                    } else {
                        std::fs::copy(&p, &target).map(|n| {
                            copied += 1;
                            bytes_copied += n;
                        })
                    };
                    if let Err(err) = res {
                        failures += 1;
                        if first_error.is_empty() {
                            first_error = format!("{}: {err}", rel.display());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // The copy is ours end to end — record the root so uninstall can remove it.
    mf.entries.push(crate::manifest::Entry {
        path: dst.display().to_string(),
        action: crate::manifest::Action::Created,
        backup: None,
    });

    if failures > 0 {
        r.push(StepResult::fail(
            "Modded copy",
            format!("{failures} file(s) failed — first was {first_error}"),
        ));
    } else {
        r.push(StepResult::ok(
            "Modded copy",
            format!(
                "{dest} — {linked} file(s) shared with your Steam install (no extra space), {copied} copied ({})",
                human(bytes_copied)
            ),
        ));
        if skipped_battleye {
            r.push(StepResult::ok(
                "BattlEye",
                "left out of the modded copy — it can't run there. Your Steam install keeps its own, so official servers still work.",
            ));
        }
    }
    r
}

/// Tell the Launcher where the modded copy lives.
///
/// @dep: apps/turdmod-loader/launcher/src/lib.rs::resolve_scum reads this
///   before falling back to its built-in candidate list.
/// @inv: without this the Launcher would fall through to the Steam install,
///   which defeats the whole point of building an isolated copy.
pub fn write_client_config(modded_root: &str) -> StepResult {
    let Ok(local) = std::env::var("LOCALAPPDATA") else {
        return StepResult::ok("Launcher config", "no LOCALAPPDATA — skipped");
    };
    let dir = PathBuf::from(local).join("TurdMOD");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return StepResult::ok("Launcher config", format!("couldn't create {}: {e}", dir.display()));
    }
    let path = dir.join("client.json");
    let body = serde_json::json!({
        "modded_root": modded_root,
        "scum_exe": PathBuf::from(modded_root)
            .join("SCUM").join("Binaries").join("Win64").join("SCUM.exe")
            .display().to_string(),
    });
    match serde_json::to_string_pretty(&body).map(|s| std::fs::write(&path, s)) {
        Ok(Ok(())) => StepResult::ok("Launcher config", format!("{} — the Launcher will use this copy", path.display())),
        _ => StepResult::ok("Launcher config", format!("couldn't write {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_read_only_content_is_ever_hardlinked() {
        for ext in ["pak", "ucas", "utoc", "sig", "PAK"] {
            assert!(is_immutable(Path::new(&format!("a.{ext}"))), "{ext} should link");
        }
        // The game writes all of these inside its own folder — linking them
        // would let the modded copy corrupt the vanilla install.
        for ext in ["log", "ini", "dmp", "exe", "dll", "json", "sav"] {
            assert!(!is_immutable(Path::new(&format!("a.{ext}"))), "{ext} must be a real copy");
        }
        assert!(!is_immutable(Path::new("no_extension")));
    }

    /// @inv: BattlEye must never reach the modded copy — and must never be
    ///   removed from the Steam install, which is what keeps official servers
    ///   playable.
    #[test]
    fn battleye_is_left_out_of_the_copy_and_left_alone_in_the_source() {
        let d = std::env::temp_dir().join("tm-client-be");
        let _ = std::fs::remove_dir_all(&d);
        let w = d.join("src").join("SCUM").join("Binaries").join("Win64");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("SCUM.exe"), b"exe").unwrap();
        let be = w.join("BattlEye");
        std::fs::create_dir_all(&be).unwrap();
        std::fs::write(be.join("BEClient_x64.dll"), b"be").unwrap();

        let dest = d.join("dest");
        let mut mf = Manifest::new_in("t", d.clone());
        let r = create_modded_copy(&d.join("src").display().to_string(), &dest.display().to_string(), &mut mf);
        assert!(r[0].ok, "{}", r[0].detail);

        assert!(dest.join("SCUM").join("Binaries").join("Win64").join("SCUM.exe").is_file());
        assert!(
            !dest.join("SCUM").join("Binaries").join("Win64").join("BattlEye").exists(),
            "BattlEye must not exist in the modded copy"
        );
        assert!(be.join("BEClient_x64.dll").is_file(), "the Steam install keeps its BattlEye");
        assert!(r.iter().any(|s| s.step == "BattlEye"), "and we say we did it");

        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn excluded_dir_matching_is_case_insensitive_and_exact() {
        assert!(is_excluded_dir(Path::new(r"C:\g\BattlEye")));
        assert!(is_excluded_dir(Path::new(r"C:\g\battleye")));
        assert!(!is_excluded_dir(Path::new(r"C:\g\BattlEyeNotes")), "must be the folder, not a prefix");
        assert!(!is_excluded_dir(Path::new(r"C:\g\Binaries")));
    }

    #[test]
    fn volume_is_parsed_from_a_windows_path() {
        assert_eq!(volume_of(Path::new(r"C:\Games\SCUM")).as_deref(), Some("C:"));
        assert_eq!(volume_of(Path::new(r"f:\SCUM-Modded")).as_deref(), Some("F:"));
    }

    #[test]
    fn refuses_a_source_that_isnt_a_scum_client() {
        let d = std::env::temp_dir().join("tm-client-badsrc");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut mf = Manifest::new_in("t", d.clone());
        let r = create_modded_copy(&d.display().to_string(), &d.join("out").display().to_string(), &mut mf);
        assert!(!r[0].ok);
        assert!(r[0].detail.contains("doesn't look like"), "{}", r[0].detail);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_a_non_empty_destination() {
        let d = std::env::temp_dir().join("tm-client-nonempty");
        let _ = std::fs::remove_dir_all(&d);
        let src = d.join("src");
        std::fs::create_dir_all(src.join("SCUM").join("Binaries").join("Win64")).unwrap();
        std::fs::write(src.join("SCUM").join("Binaries").join("Win64").join("SCUM.exe"), b"x").unwrap();
        let dest = d.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("something-of-theirs.txt"), b"important").unwrap();

        let mut mf = Manifest::new_in("t", d.clone());
        let r = create_modded_copy(&src.display().to_string(), &dest.display().to_string(), &mut mf);
        assert!(!r[0].ok, "must not clobber an existing folder");
        assert!(dest.join("something-of-theirs.txt").exists(), "their file must be untouched");

        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The real thing, in miniature: same-volume copy links the pak and copies
    /// the rest, and the link must not alias when later replaced correctly.
    #[test]
    fn links_content_and_copies_everything_else() {
        let d = std::env::temp_dir().join("tm-client-copy");
        let _ = std::fs::remove_dir_all(&d);
        let w = d.join("src").join("SCUM").join("Binaries").join("Win64");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("SCUM.exe"), b"exe-bytes").unwrap();
        let paks = d.join("src").join("SCUM").join("Content").join("Paks");
        std::fs::create_dir_all(&paks).unwrap();
        std::fs::write(paks.join("big.pak"), b"pak-bytes").unwrap();
        std::fs::write(w.join("UE4SS.log"), b"log-bytes").unwrap();

        let dest = d.join("dest");
        let mut mf = Manifest::new_in("t", d.clone());
        let r = create_modded_copy(&d.join("src").display().to_string(), &dest.display().to_string(), &mut mf);
        assert!(r[0].ok, "{}", r[0].detail);

        let dest_pak = dest.join("SCUM").join("Content").join("Paks").join("big.pak");
        let dest_exe = dest.join("SCUM").join("Binaries").join("Win64").join("SCUM.exe");
        assert!(dest_pak.exists() && dest_exe.exists());
        assert_eq!(std::fs::read(&dest_pak).unwrap(), b"pak-bytes");

        // Replacing a linked pak the SAFE way must not touch the source.
        std::fs::remove_file(&dest_pak).unwrap();
        std::fs::write(&dest_pak, b"modded-pak").unwrap();
        assert_eq!(
            std::fs::read(paks.join("big.pak")).unwrap(),
            b"pak-bytes",
            "delete-then-create must leave the vanilla pak alone"
        );

        // The copy root is recorded so uninstall can remove it.
        assert!(mf.entries.iter().any(|e| e.path == dest.display().to_string()));

        std::fs::remove_dir_all(&d).unwrap();
    }
}
