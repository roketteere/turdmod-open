//! turdmod-guard daemon.
//!
//! Subscribes to the companion's event stream, runs each event through
//! the detector registry, forwards Flag verdicts to the backend (and the
//! local log) for admin attention.
//!
//! Two run modes:
//!
//!   * `live` — connect to the companion over HTTP/WS and stream events
//!     in real-time.
//!   * `replay <ndjson-file>` — read a captured stream from disk. Used
//!     for testing detectors against historical data.

#![allow(clippy::needless_return)]

mod detectors;

use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;
use clap::{Parser, Subcommand};
use detectors::{Detector, DetectorReport, Event, Verdict};
use once_cell::sync::Lazy;

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Backend URL to forward flagged reports to. If unset, reports stay local.
    #[arg(long, env = "TURDMOD_GUARD_BACKEND")]
    backend: Option<String>,

    /// Where to append a local audit log (NDJSON of every report).
    #[arg(long, default_value = "guard-reports.ndjson")]
    log_file: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Replay a captured event stream (NDJSON, one Event per line).
    Replay {
        file: PathBuf,
    },
    /// Connect to a running companion and stream live events. (Stub.)
    Live {
        #[arg(long, default_value = "http://127.0.0.1:8911/events")]
        companion_url: String,
    },
}

static REGISTRY: Lazy<Mutex<Vec<Box<dyn Detector>>>> =
    Lazy::new(|| Mutex::new(detectors::default_registry()));

/// Branding banner. Kept in sync with companion/src/index.ts:BANNER and
/// loader/src/lib.rs:BANNER so every TurdMOD process logs the same
/// "TurdMOD is running" moment.
const BANNER: &str = "\n\
████████╗██╗   ██╗██████╗ ██████╗ ███╗   ███╗ ██████╗ ██████╗\n\
╚══██╔══╝██║   ██║██╔══██╗██╔══██╗████╗ ████║██╔═══██╗██╔══██╗\n\
   ██║   ██║   ██║██████╔╝██║  ██║██╔████╔██║██║   ██║██║  ██║\n\
   ██║   ██║   ██║██╔══██╗██║  ██║██║╚██╔╝██║██║   ██║██║  ██║\n\
   ██║   ╚██████╔╝██║  ██║██████╔╝██║ ╚═╝ ██║╚██████╔╝██████╔╝\n\
   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═════╝\n\
\n\
                  >>> TurdMOD is running! <<<\n\
                 guard v0.1.0 — anti-cheat daemon\n\
                  github.com/roketteere/scummymap\n";

fn main() {
    eprintln!("{BANNER}");
    let cli = Cli::parse();
    eprintln!("turdmod-guard v{}", env!("CARGO_PKG_VERSION"));

    match cli.cmd {
        Cmd::Replay { ref file } => {
            if let Err(e) = run_replay(file, cli.backend.as_deref(), &cli.log_file) {
                eprintln!("replay failed: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Live { ref companion_url } => {
            eprintln!("live mode not yet implemented — companion needs an /events endpoint first ({companion_url})");
            eprintln!("for now, capture the companion's stdout to NDJSON and use `replay`.");
            std::process::exit(1);
        }
    }
}

fn run_replay(file: &PathBuf, backend: Option<&str>, log_path: &PathBuf) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(std::fs::File::open(file).map_err(|e| e.to_string())?);
    let mut counts = (0usize, 0usize, 0usize); // ok, warn, flag
    let mut log_out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| e.to_string())?;

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: Event = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip non-Event line: {e} <{line}>");
                continue;
            }
        };
        let mut reg = REGISTRY.lock().unwrap();
        for det in reg.iter_mut() {
            let v = det.evaluate(&event);
            match &v {
                Verdict::Ok => counts.0 += 1,
                Verdict::Warn { .. } => counts.1 += 1,
                Verdict::Flag { .. } => counts.2 += 1,
            }
            if matches!(v, Verdict::Warn { .. } | Verdict::Flag { .. }) {
                let report = DetectorReport {
                    detector: det.name().to_string(),
                    ts: Utc::now(),
                    verdict: v.clone(),
                    steam: extract_steam(&event),
                    player: extract_player(&event),
                };
                emit_report(&report, backend, &mut log_out);
            }
        }
    }
    eprintln!(
        "replay done: ok={} warn={} flag={}",
        counts.0, counts.1, counts.2
    );
    Ok(())
}

fn emit_report(report: &DetectorReport, backend: Option<&str>, log: &mut std::fs::File) {
    let json = match serde_json::to_string(report) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("serialize report failed: {e}");
            return;
        }
    };
    use std::io::Write;
    let _ = writeln!(log, "{json}");
    eprintln!("[{}] {}", report.detector, json);
    if let Some(url) = backend {
        // Best-effort POST. Failures are logged but not fatal.
        let result = ureq::post(url).send_string(&json);
        if let Err(e) = result {
            eprintln!("backend post failed: {e}");
        }
    }
}

fn extract_steam(event: &Event) -> Option<String> {
    match event {
        Event::Login { steam, .. } | Event::Logout { steam, .. } | Event::Admin { steam, .. } => {
            Some(steam.clone())
        }
        Event::Kill { killer_steam, .. } => killer_steam.clone(),
        Event::Chat { steam, .. } => steam.clone(),
        Event::Vehicle { owner, .. } => owner.clone(),
    }
}

fn extract_player(event: &Event) -> Option<String> {
    match event {
        Event::Login { player, .. } | Event::Logout { player, .. } | Event::Admin { player, .. } => {
            Some(player.clone())
        }
        Event::Kill { killer, .. } => killer.clone(),
        Event::Chat { player, .. } => Some(player.clone()),
        _ => None,
    }
}
