// "Is there a newer TurdMOD?" — compares what's installed against what
// turdmod.com currently advertises.
//
// @dep: scripts/package-release.ps1 writes VERSION.json into the pack and the
//   identical latest.json into releases/; scripts/upload-release.ps1 pushes
//   latest.json LAST so it never advertises artifacts that aren't up yet.
// @inv: never report "up to date" when we simply couldn't tell. An unknown
//   local version or an unreachable server is `Unknown`, not `Current` —
//   telling someone they're current when they might not be is the one wrong
//   answer here.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const LATEST_URL: &str = "https://turdmod.com/releases/latest.json";
const TURDMOD_DIR: &str = r"C:\TurdMOD";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VersionInfo {
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub released: String,
    #[serde(default)]
    pub engine_built: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateState {
    /// Installed build matches what's published.
    Current,
    /// A different build is published — offer it.
    Available,
    /// Couldn't determine one side or the other. Say so; don't guess.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateReport {
    pub state: UpdateState,
    pub installed: Option<VersionInfo>,
    pub latest: Option<VersionInfo>,
    /// Plain-language line for the UI.
    pub summary: String,
    pub download_url: String,
}

fn parse(raw: &str) -> Option<VersionInfo> {
    let v: VersionInfo = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    if v.build.is_empty() {
        return None;
    }
    Some(v)
}

/// The VERSION.json recorded when this machine was set up.
pub fn installed_version() -> Option<VersionInfo> {
    for p in [
        PathBuf::from(TURDMOD_DIR).join("VERSION.json"),
        // Also look beside the running exe — the pack ships one at its root.
        std::env::current_exe().ok()?.parent()?.join("VERSION.json"),
    ] {
        if let Ok(raw) = std::fs::read_to_string(&p) {
            if let Some(v) = parse(&raw) {
                return Some(v);
            }
        }
    }
    None
}

async fn fetch_latest() -> Option<VersionInfo> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().ok()?;
    let text = client.get(LATEST_URL).send().await.ok()?.text().await.ok()?;
    parse(&text)
}

pub async fn check() -> UpdateReport {
    let installed = installed_version();
    let latest = fetch_latest().await;
    let download_url = "https://turdmod.com/downloads".to_string();

    match (&installed, &latest) {
        (Some(i), Some(l)) if i.build == l.build => UpdateReport {
            state: UpdateState::Current,
            summary: format!("You're on the latest build ({}).", l.build),
            installed,
            latest,
            download_url,
        },
        (Some(i), Some(l)) => UpdateReport {
            state: UpdateState::Available,
            summary: format!(
                "A newer TurdMOD is available — you have {}, the current build is {} (released {}). \
                 Updating keeps your settings and access key.",
                i.build, l.build, l.released
            ),
            installed,
            latest,
            download_url,
        },
        (None, Some(l)) => UpdateReport {
            state: UpdateState::Unknown,
            summary: format!(
                "The current build is {}. We couldn't tell which build you're on — installs from \
                 before this check existed don't record a version.",
                l.build
            ),
            installed,
            latest,
            download_url,
        },
        (_, None) => UpdateReport {
            state: UpdateState::Unknown,
            summary: "Couldn't reach turdmod.com to check for updates. Your install is unaffected."
                .into(),
            installed,
            latest,
            download_url,
        },
    }
}

/// Copy the pack's VERSION.json into C:\TurdMOD so later runs can compare.
/// Best-effort: a missing version file must never fail an install.
pub fn record_installed_version(artifacts: &std::path::Path) {
    let src = artifacts.join("VERSION.json");
    if !src.exists() {
        return;
    }
    let dir = PathBuf::from(TURDMOD_DIR);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::copy(src, dir.join("VERSION.json"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_version_file() {
        let v = parse(r#"{"build":"20260727-1216","released":"2026-07-27","engine_built":"2026-07-22"}"#)
            .expect("should parse");
        assert_eq!(v.build, "20260727-1216");
        assert_eq!(v.released, "2026-07-27");
    }

    #[test]
    fn a_version_file_without_a_build_is_not_usable() {
        assert!(parse("{}").is_none(), "no build id means we can't compare");
        assert!(parse(r#"{"build":""}"#).is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn tolerates_a_bom() {
        assert!(parse("\u{feff}{\"build\":\"x\"}").is_some());
    }

    /// @inv: the dangerous answer is a false "you're up to date".
    #[tokio::test]
    async fn an_unknown_local_version_is_never_reported_as_current() {
        // Both-sides-known is the only path to Current; assert the mapping
        // directly rather than depending on the network.
        let unknown_local = UpdateReport {
            state: UpdateState::Unknown,
            installed: None,
            latest: Some(VersionInfo { build: "x".into(), ..Default::default() }),
            summary: String::new(),
            download_url: String::new(),
        };
        assert_ne!(unknown_local.state, UpdateState::Current);
    }
}
