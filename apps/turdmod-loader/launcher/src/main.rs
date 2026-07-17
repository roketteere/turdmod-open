//! turdmod-launcher — start SCUM.exe with the TurdMOD loader injected.
//!
//! Thin CLI over `turdmod_launcher_core`: parse flags, build a
//! `LaunchOptions`, call `launch`. The spawn-suspended + CreateRemoteThread
//! injection itself lives in the lib so the desktop launcher's Tauri
//! backend shares the exact same code path.
//!
//! This is the standard "spawn-suspended + inject + resume" pattern that
//! every UE4-mod loader uses (UE4SS, REFramework, etc.). It runs as the
//! game's parent process, so it requires no admin rights and BE doesn't
//! see anything because BE isn't running on private-BE-off servers.

use std::path::PathBuf;

use clap::Parser;
use turdmod_launcher_core::{launch, resolve_dll, resolve_scum, LaunchOptions, ServerTarget};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// Path to SCUM.exe. If omitted, tries SCUM_EXE env, then Steam install discovery.
    #[arg(long)]
    scum: Option<PathBuf>,

    /// Path to turdmod_loader.dll. If omitted, looks next to the launcher exe.
    #[arg(long)]
    dll: Option<PathBuf>,

    /// Extra DLLs to inject after the primary loader (repeatable).
    /// Used to load the decorator DLL alongside the kitchen-sink loader.
    /// Example: --extra-dll path\to\turdmod_rich_decorators.dll
    #[arg(long = "extra-dll", action = clap::ArgAction::Append)]
    extra_dlls: Vec<PathBuf>,

    /// Connect target host/ip for an allowlisted BE-off server. When set,
    /// the launcher appends `+connect <server>:<server-port>` and writes the
    /// server block into launch-mode.json.
    #[arg(long)]
    server: Option<String>,

    /// Port for --server (default 7042, SCUM's typical game port).
    #[arg(long, default_value_t = 7042)]
    server_port: u16,

    /// Human-readable name for --server (logged + recorded in launch-mode.json).
    #[arg(long, default_value = "manual")]
    server_name: String,

    /// Args forwarded to SCUM.exe (everything after `--`).
    #[arg(last = true)]
    game_args: Vec<String>,

    /// Skip the BattlEye / official-server pre-flight check. Off by default —
    /// don't enable unless you know exactly what you're doing.
    #[arg(long)]
    skip_safety_check: bool,
}

/// Branding banner. Kept in sync with companion / loader / guard.
// `concat!` joins each piece without applying line-continuation escape
// rules — using `"\n\\\n   ..."` (the natural way to break lines in a
// `const &str`) would eat every leading space on the next source line
// per the Rust reference §"String escapes". We need leading whitespace
// preserved on the inner banner rows so the T-block's middle column
// stays vertically aligned. concat! avoids the trap entirely.
const BANNER: &str = concat!(
    "\n",
    "████████╗██╗   ██╗██████╗ ██████╗ ███╗   ███╗ ██████╗ ██████╗\n",
    "╚══██╔══╝██║   ██║██╔══██╗██╔══██╗████╗ ████║██╔═══██╗██╔══██╗\n",
    "   ██║   ██║   ██║██████╔╝██║  ██║██╔████╔██║██║   ██║██║  ██║\n",
    "   ██║   ██║   ██║██╔══██╗██║  ██║██║╚██╔╝██║██║   ██║██║  ██║\n",
    "   ██║   ╚██████╔╝██║  ██║██████╔╝██║ ╚═╝ ██║╚██████╔╝██████╔╝\n",
    "   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═════╝\n",
    "\n",
    "                  >>> TurdMOD is running! <<<\n",
    "                 launcher v0.1.0 — DLL injector\n",
    "                  github.com/roketteere/scummymap\n",
);

fn main() {
    eprintln!("{BANNER}");
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("turdmod-launcher: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let scum_exe = resolve_scum(cli.scum.as_deref())?;
    let dll = resolve_dll(cli.dll.as_deref())?;

    // A manually-passed --server is trusted as BE-off (battle_eye=false):
    // the CLI is the dev/power-user path. The desktop launcher gets its
    // servers from the allowlist endpoint and never offers a BE-on one.
    let server = cli.server.as_ref().map(|host| ServerTarget {
        id: format!("cli:{host}"),
        name: cli.server_name.clone(),
        ip: host.clone(),
        port: cli.server_port,
        battle_eye: false,
    });

    eprintln!("turdmod-launcher v{}", env!("CARGO_PKG_VERSION"));

    let opts = LaunchOptions {
        scum_exe,
        dll,
        extra_dlls: cli.extra_dlls,
        game_args: cli.game_args,
        server,
        skip_safety_check: cli.skip_safety_check,
    };

    let pid = launch(&opts, &mut |line| eprintln!("  {line}"))?;
    eprintln!("  launched pid={pid}");
    Ok(())
}
