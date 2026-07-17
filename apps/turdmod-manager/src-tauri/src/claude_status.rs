// Claude-connected indicator. Watches the Claude Code session-log
// directory (`~/.claude/projects/*/<session>.jsonl`) and reports the
// freshest activity timestamp. The .jsonl file's LastWriteTime updates
// on every tool call Claude makes, so it's a free heartbeat — no code
// in the Claude side required.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const ONLINE_WINDOW_MS: u64 = 60_000; // 1 minute
const IDLE_WINDOW_MS: u64 = 300_000; // 5 minutes

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeStatus {
    /// "online" | "idle" | "offline"
    pub state: String,
    pub last_update_ms: u64,
    pub idle_for_ms: u64,
    pub project_dir: Option<String>,
    pub session_id: Option<String>,
    pub log_path: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn claude_projects_dir() -> Option<PathBuf> {
    let p = home_dir()?.join(".claude").join("projects");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn find_latest_session() -> Option<(PathBuf, SystemTime, String, String)> {
    let dir = claude_projects_dir()?;
    let mut latest: Option<(PathBuf, SystemTime, String, String)> = None;

    let project_iter = std::fs::read_dir(&dir).ok()?;
    for project_entry in project_iter.flatten() {
        let Ok(ft) = project_entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let project_name = project_entry.file_name().to_string_lossy().into_owned();
        let project_path = project_entry.path();

        let Ok(file_iter) = std::fs::read_dir(&project_path) else {
            continue;
        };
        for entry in file_iter.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            match &latest {
                None => latest = Some((path, mtime, project_name.clone(), session_id)),
                Some((_, prev_mtime, _, _)) if mtime > *prev_mtime => {
                    latest = Some((path, mtime, project_name.clone(), session_id));
                }
                _ => {}
            }
        }
    }
    latest
}

#[tauri::command]
pub fn claude_status() -> ClaudeStatus {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    match find_latest_session() {
        Some((path, mtime, project_dir, session_id)) => {
            let last_update_ms = mtime
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let idle_for_ms = now_ms.saturating_sub(last_update_ms);

            let state = if idle_for_ms < ONLINE_WINDOW_MS {
                "online"
            } else if idle_for_ms < IDLE_WINDOW_MS {
                "idle"
            } else {
                "offline"
            }
            .to_string();

            ClaudeStatus {
                state,
                last_update_ms,
                idle_for_ms,
                project_dir: Some(project_dir),
                session_id: Some(session_id),
                log_path: Some(path.to_string_lossy().into_owned()),
            }
        }
        None => ClaudeStatus {
            state: "offline".to_string(),
            last_update_ms: 0,
            idle_for_ms: u64::MAX,
            project_dir: None,
            session_id: None,
            log_path: None,
        },
    }
}
