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

/// Why the game server stopped, read out of its own log.
///
/// @ctx: "the server isn't running — start it" is useless when the server DID
///   start and then quit on purpose. SCUM writes the reason to SCUM.log and
///   exits cleanly, so the process is simply gone by the time we look. Without
///   this the operator sees a green service, a red server, and no explanation.
/// @dep: SCUM writes "Requested application exit with the following error
///   message:" followed by the reason on the next Error line.
fn exit_reason_from_log(server_root: &str) -> Option<String> {
    let log = std::path::Path::new(server_root)
        .join("SCUM")
        .join("Saved")
        .join("Logs")
        .join("SCUM.log");
    let text = std::fs::read_to_string(&log).ok()?;

    // @ctx: scan the WHOLE file, not a tail window. SCUM.log is truncated per
    //   run, and UE's shutdown is verbose — a real failure sat ~700 lines above
    //   the end, so a 400-line tail missed it entirely. The only reason to
    //   bound this at all is a pathologically large log.
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(50_000);
    let lines = &lines[start..];

    let idx = lines
        .iter()
        .rposition(|l| l.contains("Requested application exit with the following error message"))?;
    // The reason is the next Error line.
    let reason = lines[idx + 1..]
        .iter()
        .take(4)
        .find_map(|l| l.split("Error:").nth(1))
        .map(|s| s.trim().to_string())?;
    Some(reason)
}

/// Turn a raw engine exit reason into something actionable.
fn advise_on_exit(reason: &str) -> String {
    if reason.contains("integrity compromised") || reason.contains("sig file") {
        let pak = reason
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        return format!(
            "SCUM started, then quit because a mod pak failed its signature check{}. \
             That's the game refusing to load a modified pak — not a TurdMOD problem. \
             Move that .pak and its .sig out of SCUM\\Content\\Paks (keep them somewhere safe), \
             then start the server again.",
            if pak.is_empty() { String::new() } else { format!(" ({pak})") }
        );
    }
    format!(
        "SCUM started, then quit on its own. It reported: \"{reason}\". \
         Fix that, then start the server again."
    )
}

/// Run all checks against a running install.
///
/// `server_root` is optional; when given, a stopped game server is diagnosed
/// from SCUM.log instead of being reported as a bare "not running".
pub async fn run(port: u16, token: &str, server_root: Option<&str>) -> VerifyReport {
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
                        // It may not have failed to start — it may have started
                        // and then quit. Those need completely different advice.
                        match server_root.and_then(exit_reason_from_log) {
                            Some(reason) => advise_on_exit(&reason),
                            None => "The service is up but SCUM isn't started. The Manager's Start button (or POST /server/start) will launch it.".into(),
                        }
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

    #[test]
    fn a_signature_failure_is_named_as_such_not_as_didnt_start() {
        let advice = advise_on_exit(
            "  Pak file or matching sig file integrity compromised: ../../../SCUM/Content/Paks/pakchunk0_s53-WindowsServer.pak",
        );
        assert!(advice.contains("signature check"), "{advice}");
        assert!(advice.contains("pakchunk0_s53-WindowsServer.pak"), "names the culprit: {advice}");
        assert!(advice.contains("not a TurdMOD problem"), "must not send them debugging us");
        assert!(!advice.contains("Start button"), "it DID start — don't tell them to start it");
    }

    #[test]
    fn an_unknown_exit_reason_is_quoted_verbatim() {
        let advice = advise_on_exit("Something we've never seen");
        assert!(advice.contains("Something we've never seen"), "{advice}");
    }

    #[test]
    fn no_log_means_no_invented_diagnosis() {
        assert!(exit_reason_from_log(r"Z:\definitely\not\here").is_none());
    }

    /// Regression: the first version scanned only the last 400 lines and missed
    /// a real failure that sat ~700 lines above the end of a 1043-line log,
    /// because UE's shutdown sequence is long and chatty.
    #[test]
    fn finds_the_reason_even_when_far_above_the_end_of_the_log() {
        let root = std::env::temp_dir().join("tm-setup-logscan-test");
        let logs = root.join("SCUM").join("Saved").join("Logs");
        std::fs::create_dir_all(&logs).unwrap();

        let mut log = String::from("LogInit: boot\n");
        log.push_str("LogSCUM: Error: Requested application exit with the following error message:\n");
        log.push_str("LogSCUM: Error:   Pak file or matching sig file integrity compromised: ../../../SCUM/Content/Paks/pakchunk0_s53-WindowsServer.pak\n");
        for i in 0..800 {
            log.push_str(&format!("LogModuleManager: Shutting down and abandoning module M{i}\n"));
        }
        log.push_str("LogExit: Exiting.\n");
        std::fs::write(logs.join("SCUM.log"), log).unwrap();

        let reason = exit_reason_from_log(&root.display().to_string())
            .expect("must find the reason despite 800 lines of shutdown noise");
        assert!(reason.contains("integrity compromised"), "{reason}");
        assert!(advise_on_exit(&reason).contains("pakchunk0_s53-WindowsServer.pak"));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn unreachable_service_fails_closed_with_guidance() {
        // Port 1 is never a TurdMOD service.
        let r = run(1, "tok", None).await;
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
