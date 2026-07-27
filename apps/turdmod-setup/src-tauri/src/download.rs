// Fetch the Server Pack ourselves, so the wizard works from a bare exe.
//
// @ctx: Setup used to require the pack already extracted beside it — which is
//   why the pack (not the standalone exe) had to be the "Start here" download,
//   and why anyone grabbing just Setup.exe hit "Couldn't find
//   turdmod-service.exe". This closes that.
//
// @inv: extract defensively. A zip entry can name ../ paths and escape the
//   destination ("zip slip"); we resolve every entry against the target dir and
//   refuse anything that lands outside it. The pack is ours today, but this
//   code runs on whatever the network hands it.

use crate::install_local::StepResult;
use futures_util::StreamExt;
use serde::Serialize;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const PACK_URL: &str = "https://turdmod.com/releases/TurdMOD-Server-Pack-latest.zip";

#[derive(Debug, Clone, Serialize)]
pub struct DownloadResult {
    pub steps: Vec<StepResult>,
    /// Where the pack was extracted, when it worked.
    pub artifacts_dir: Option<String>,
}

/// Where downloaded packs land. Beside the running exe when writable (keeps
/// everything together), else the temp dir.
fn staging_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("turdmod-pack");
            if std::fs::create_dir_all(&candidate).is_ok() {
                // Prove it's actually writable — Program Files often isn't.
                let probe = candidate.join(".w");
                if std::fs::write(&probe, b"x").is_ok() {
                    let _ = std::fs::remove_file(&probe);
                    return candidate;
                }
            }
        }
    }
    std::env::temp_dir().join("turdmod-pack")
}

/// Reject entries that would escape the destination directory.
fn safe_join(root: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name);
    if rel.is_absolute() {
        return None;
    }
    let mut out = root.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(part) => out.push(part),
            // Anything else (.., root, prefix) is an escape attempt.
            Component::CurDir => {}
            _ => return None,
        }
    }
    out.starts_with(root).then_some(out)
}

pub async fn fetch_pack() -> DownloadResult {
    let mut steps = Vec::new();
    let dir = staging_dir();

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(600)).build() {
        Ok(c) => c,
        Err(e) => {
            steps.push(StepResult::fail("Download", format!("{e}")));
            return DownloadResult { steps, artifacts_dir: None };
        }
    };

    let resp = match client.get(PACK_URL).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            steps.push(StepResult::fail(
                "Download",
                format!("turdmod.com returned HTTP {} for the Server Pack", r.status()),
            ));
            return DownloadResult { steps, artifacts_dir: None };
        }
        Err(e) => {
            steps.push(StepResult::fail(
                "Download",
                format!("couldn't reach turdmod.com: {e}. Check your internet connection."),
            ));
            return DownloadResult { steps, artifacts_dir: None };
        }
    };

    let total = resp.content_length().unwrap_or(0);
    let zip_path = dir.join("server-pack.zip");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        steps.push(StepResult::fail("Download", format!("{}: {e}", dir.display())));
        return DownloadResult { steps, artifacts_dir: None };
    }

    let mut file = match std::fs::File::create(&zip_path) {
        Ok(f) => f,
        Err(e) => {
            steps.push(StepResult::fail("Download", format!("{}: {e}", zip_path.display())));
            return DownloadResult { steps, artifacts_dir: None };
        }
    };

    let mut written = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                if let Err(e) = file.write_all(&bytes) {
                    steps.push(StepResult::fail("Download", format!("writing the download: {e}")));
                    return DownloadResult { steps, artifacts_dir: None };
                }
                written += bytes.len() as u64;
            }
            Err(e) => {
                steps.push(StepResult::fail("Download", format!("the download was interrupted: {e}")));
                return DownloadResult { steps, artifacts_dir: None };
            }
        }
    }
    drop(file);

    // A truncated download that still extracts is worse than a clean failure.
    if total > 0 && written != total {
        let _ = std::fs::remove_file(&zip_path);
        steps.push(StepResult::fail(
            "Download",
            format!("incomplete: got {written} of {total} bytes. Try again."),
        ));
        return DownloadResult { steps, artifacts_dir: None };
    }

    steps.push(StepResult::ok(
        "Download",
        format!("{:.1} MB from turdmod.com", written as f64 / 1024.0 / 1024.0),
    ));

    match extract(&zip_path, &dir) {
        Ok(n) => steps.push(StepResult::ok("Extract", format!("{n} file(s) to {}", dir.display()))),
        Err(e) => {
            steps.push(StepResult::fail("Extract", e));
            return DownloadResult { steps, artifacts_dir: None };
        }
    }

    // Only claim success if what we needed is actually there.
    if !dir.join("turdmod-service.exe").exists() {
        steps.push(StepResult::fail(
            "Extract",
            "the pack didn't contain turdmod-service.exe — the download may be corrupt",
        ));
        return DownloadResult { steps, artifacts_dir: None };
    }

    let _ = std::fs::remove_file(&zip_path);
    DownloadResult { steps, artifacts_dir: Some(dir.display().to_string()) }
}

fn extract(zip_path: &Path, dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("not a readable zip: {e}"))?;
    let mut count = 0usize;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("entry {i}: {e}"))?;
        let Some(name) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            return Err(format!("refusing unsafe path in zip: {}", entry.name()));
        };
        let Some(out) = safe_join(dest, &name.to_string_lossy()) else {
            return Err(format!("refusing path that escapes the folder: {}", entry.name()));
        };

        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut w = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut w).map_err(|e| format!("{}: {e}", out.display()))?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_escape_the_destination() {
        let root = Path::new(r"C:\dest");
        assert!(safe_join(root, "a/b.txt").is_some());
        assert!(safe_join(root, "./a.txt").is_some());
        // Classic zip-slip shapes.
        assert!(safe_join(root, "../evil.exe").is_none());
        assert!(safe_join(root, "a/../../evil.exe").is_none());
        assert!(safe_join(root, r"C:\Windows\System32\evil.dll").is_none());
        assert!(safe_join(root, "/etc/passwd").is_none());
    }

    #[test]
    fn nested_paths_stay_under_the_root() {
        let root = Path::new(r"C:\dest");
        let p = safe_join(root, "UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll").unwrap();
        assert!(p.starts_with(root));
        assert!(p.ends_with("main.dll"));
    }

    #[test]
    fn extracts_a_real_zip_and_counts_files() {
        let d = std::env::temp_dir().join("tm-download-extract");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();

        let zip_path = d.join("t.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            z.start_file("turdmod-service.exe", opts).unwrap();
            z.write_all(b"svc").unwrap();
            z.start_file("UE4SS/UE4SS.dll", opts).unwrap();
            z.write_all(b"ue4ss").unwrap();
            z.finish().unwrap();
        }

        let out = d.join("out");
        let n = extract(&zip_path, &out).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(out.join("turdmod-service.exe")).unwrap(), b"svc");
        assert!(out.join("UE4SS").join("UE4SS.dll").is_file(), "nested entries land correctly");

        std::fs::remove_dir_all(&d).unwrap();
    }
}
