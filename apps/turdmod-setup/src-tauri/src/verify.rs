// Post-install verification — prove it actually works.
//
// Three checks, in dependency order. Each failure carries a plain-language
// diagnosis, because "check failed" is useless to someone who's already stuck.

use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub label: &'static str,
    pub ok: bool,
    pub detail: String,
    /// What to do about it. Empty when ok.
    pub fix: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub checks: Vec<Check>,
    pub all_ok: bool,
    pub summary: String,
}

async fn http(url: &str, token: Option<&str>, body: Option<serde_json::Value>) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut req = match &body {
        Some(_) => client.post(url),
        None => client.get(url),
    };
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    if let Some(b) = body {
        req = req.json(&b);
    }

    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
    }
    Ok(text)
}

/// Run all checks against a running install.
pub async fn run(port: u16, token: &str) -> VerifyReport {
    let base = format!("http://127.0.0.1:{port}");
    let mut checks = Vec::new();

    // 1. Service alive
    let service_ok = match http(&format!("{base}/health"), None, None).await {
        Ok(body) => {
            checks.push(Check {
                id: "service",
                label: "TurdMOD service responding",
                ok: true,
                detail: body.chars().take(160).collect(),
                fix: String::new(),
            });
            true
        }
        Err(e) => {
            checks.push(Check {
                id: "service",
                label: "TurdMOD service responding",
                ok: false,
                detail: e,
                fix: "The service isn't running. Open Services (services.msc) and start TurdMODService, or re-run the install as Administrator.".into(),
            });
            false
        }
    };

    // 2. Game server process — only meaningful if the service answers.
    let server_running = if service_ok {
        match http(&format!("{base}/status"), Some(token), None).await {
            Ok(body) => {
                let running = body.contains("\"server_running\":true");
                checks.push(Check {
                    id: "server",
                    label: "Game server running",
                    ok: running,
                    detail: body.chars().take(160).collect(),
                    fix: if running {
                        String::new()
                    } else {
                        "The service is up but SCUM isn't started. The Manager's Start button (or POST /server/start) will launch it.".into()
                    },
                });
                running
            }
            Err(e) => {
                let bad_token = e.contains("401") || e.contains("403");
                checks.push(Check {
                    id: "server",
                    label: "Game server running",
                    ok: false,
                    detail: e,
                    fix: if bad_token {
                        "The API rejected the token. Check that the token in C:\\TurdMOD\\service.json matches what Setup configured.".into()
                    } else {
                        "Couldn't read server status.".into()
                    },
                });
                false
            }
        }
    } else {
        checks.push(Check {
            id: "server",
            label: "Game server running",
            ok: false,
            detail: "skipped — service not responding".into(),
            fix: "Fix the service check first.".into(),
        });
        false
    };

    // 3. Engine bridge — the real proof that modding works.
    if server_running {
        let body = serde_json::json!({ "method": "ping" });
        match http(&format!("{base}/engine/rpc"), Some(token), Some(body)).await {
            Ok(text) if text.contains("\"pong\":true") => checks.push(Check {
                id: "bridge",
                label: "Engine bridge connected",
                ok: true,
                detail: "ping → pong".into(),
                fix: String::new(),
            }),
            Ok(text) => checks.push(Check {
                id: "bridge",
                label: "Engine bridge connected",
                ok: false,
                detail: text.chars().take(200).collect(),
                fix: "The server is up but the bridge didn't answer. Check that UE4SS loaded the mod — look for 'TurdMODEngineBridge' in UE4SS.log next to GameServer.exe.".into(),
            }),
            Err(e) => checks.push(Check {
                id: "bridge",
                label: "Engine bridge connected",
                ok: false,
                detail: e,
                fix: "The bridge pipe isn't answering. This is usually UE4SS not loading the mod — confirm main.dll and enabled.txt are in the TurdMODEngineBridge folder, then restart the server.".into(),
            }),
        }
    } else {
        checks.push(Check {
            id: "bridge",
            label: "Engine bridge connected",
            ok: false,
            detail: "skipped — game server not running".into(),
            fix: "Start the server first.".into(),
        });
    }

    let all_ok = checks.iter().all(|c| c.ok);
    let summary = if all_ok {
        "You're running TurdMOD. All checks passed.".to_string()
    } else {
        let first = checks.iter().find(|c| !c.ok).map(|c| c.label).unwrap_or("A check");
        format!("{first} didn't pass — see the fix below, or ask the assistant.")
    };

    VerifyReport { checks, all_ok, summary }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unreachable_service_fails_closed_with_guidance() {
        // Port 1 is never a TurdMOD service.
        let r = run(1, "tok").await;
        assert!(!r.all_ok);
        assert_eq!(r.checks.len(), 3, "always reports all three checks");
        // Every failure must tell the user what to do.
        for c in r.checks.iter().filter(|c| !c.ok) {
            assert!(!c.fix.is_empty(), "check {} needs a fix hint", c.id);
        }
        // Downstream checks should be skipped, not falsely green.
        assert!(r.checks.iter().all(|c| !c.ok));
    }
}
