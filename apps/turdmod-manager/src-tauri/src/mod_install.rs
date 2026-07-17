// Install / uninstall / list / verify SCUM mods.
//
// Install pipeline:
//   1. Resolve a signed download URL via mod_index.
//   2. Stream the artifact, hashing as it goes.
//   3. Compare to the API-provided sha256.
//   4. Extract into <mods_dir>/<slug>/ (clobber any prior install).
//   5. Drop a .turdmod-installed.json sidecar so list_installed works.

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::mod_index;

const SIDECAR_FILENAME: &str = ".turdmod-installed.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub slug: String,
    pub version: String,
    pub sha256: String,
    pub installed_at: i64,
    pub install_path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InstallProgress {
    Downloading { received: u64, total: Option<u64> },
    Verifying,
    Extracting,
    Done,
}

pub async fn install_mod<F>(
    slug: &str,
    mods_dir: &Path,
    auth_token: Option<&str>,
    progress: F,
) -> Result<InstalledMod>
where
    F: Fn(InstallProgress) + Send + 'static,
{
    let info = mod_index::get_download_url(slug, auth_token).await?;
    tracing::info!(slug, version = %info.version, "install: resolved download url");

    let resp = reqwest::get(&info.url)
        .await
        .with_context(|| format!("GET signed url for {}", slug))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("download {} returned {}", slug, status));
    }
    let total = resp.content_length();

    let mut hasher = Sha256::new();
    let mut buf: Vec<u8> = Vec::with_capacity(total.unwrap_or(1024 * 1024) as usize);
    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download chunk")?;
        hasher.update(&chunk);
        buf.write_all(&chunk).context("buffer write")?;
        received += chunk.len() as u64;
        progress(InstallProgress::Downloading { received, total });
    }

    progress(InstallProgress::Verifying);
    let computed = hex(&hasher.finalize());
    if !computed.eq_ignore_ascii_case(&info.sha256) {
        return Err(anyhow!(
            "Sha256Mismatch: expected {} got {}",
            info.sha256,
            computed
        ));
    }

    progress(InstallProgress::Extracting);
    let target = mods_dir.join(slug);
    if target.exists() {
        fs::remove_dir_all(&target)
            .with_context(|| format!("clobber existing install {}", target.display()))?;
    }
    fs::create_dir_all(&target).context("create slug dir")?;

    let reader = Cursor::new(buf);
    let mut zip = zip::ZipArchive::new(reader).context("open zip")?;
    extract_zip(&mut zip, &target)?;

    let installed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let size_bytes = info.size_bytes.unwrap_or(received);
    let record = InstalledMod {
        slug: slug.to_string(),
        version: info.version,
        sha256: computed,
        installed_at,
        install_path: target.clone(),
        size_bytes,
    };

    let sidecar = target.join(SIDECAR_FILENAME);
    let json = serde_json::to_vec_pretty(&record).context("serialize sidecar")?;
    fs::write(&sidecar, json)
        .with_context(|| format!("write sidecar {}", sidecar.display()))?;

    progress(InstallProgress::Done);
    Ok(record)
}

// Stdlib-only zip extractor. Skips zip-slip paths (entries that try to
// escape the target dir via .. or absolute paths).
fn extract_zip<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    target: &Path,
) -> Result<()> {
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("zip entry")?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => {
                tracing::warn!(name = entry.name(), "zip: skipping unsafe entry");
                continue;
            }
        };
        let out_path = target.join(&entry_path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&out_path)
            .with_context(|| format!("create {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out).context("zip extract copy")?;
    }
    Ok(())
}

pub fn uninstall_mod(slug: &str, mods_dir: &Path) -> Result<()> {
    let target = mods_dir.join(slug);
    if !target.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&target)
        .with_context(|| format!("remove {}", target.display()))?;
    Ok(())
}

pub fn list_installed(mods_dir: &Path) -> Result<Vec<InstalledMod>> {
    let mut out = Vec::new();
    if !mods_dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(mods_dir).with_context(|| format!("read {}", mods_dir.display()))? {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(err = %err, "list_installed: dir entry error");
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let sidecar = path.join(SIDECAR_FILENAME);
        if !sidecar.exists() {
            continue;
        }
        match fs::read(&sidecar).map_err(anyhow::Error::from).and_then(|b| {
            serde_json::from_slice::<InstalledMod>(&b).map_err(anyhow::Error::from)
        }) {
            Ok(rec) => out.push(rec),
            Err(err) => tracing::warn!(err = %err, sidecar = %sidecar.display(), "list_installed: bad sidecar"),
        }
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}
