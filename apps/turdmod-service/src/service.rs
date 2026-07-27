// Windows Service registration + SCM event loop.
// Supports multi-instance via --instance flag in binPath.

use crate::config::Config;
use crate::engine::{self, new_state};
use crate::server::run_server;
use crate::shutdown::Shutdown;
use std::ffi::OsString;
use tokio::runtime::Runtime;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    Result as WsResult,
};

define_windows_service!(ffi_service_main, service_main);

pub fn service_name(instance: &str) -> String {
    match instance {
        "default" => "TurdMODService".into(),
        id => format!("TurdMOD-{}", id),
    }
}

pub fn dispatch(instance: &str) -> WsResult<()> {
    service_dispatcher::start(&service_name(instance), ffi_service_main)
}

fn service_main(args: Vec<OsString>) {
    // Parse --instance from SCM args (passed via binPath= extra arguments)
    let args_str: Vec<String> = args.iter().filter_map(|a| a.to_str().map(String::from)).collect();
    let instance = args_str.windows(2)
        .find(|w| w[0] == "--instance")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "default".into());

    if let Err(e) = run_service(&instance) {
        tracing::error!("service error: {}", e);
    }
}

fn run_service(instance: &str) -> anyhow::Result<()> {
    let cfg = Config::load_or_default(instance);
    // Same as run_console — the service path must install owner identity too,
    // or a service-mode start silently loses owner recognition.
    crate::owner::init(cfg.owner_steam_ids.clone(), cfg.owner_name.clone());
    if crate::owner::is_unconfigured() {
        tracing::warn!("no owner_steam_ids/owner_name in service.json — owner-gated mods will ignore everyone");
    }
    crate::restore_campaign::set_instance(&cfg.instance_id);
    let state = new_state();
    let (shutdown, shutdown_rx) = Shutdown::new();

    let state_for_stop = std::sync::Arc::clone(&state);
    let shutdown_for_stop = shutdown.clone();

    let event_handler = move |ctrl| -> ServiceControlHandlerResult {
        if ctrl == ServiceControl::Stop {
            // Kill SCUMServer
            let s = state_for_stop.lock().unwrap();
            if let Some(pid) = s.pid {
                std::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .spawn()
                    .ok();
            }
            drop(s);

            // Signal all async tasks + axum to shut down
            shutdown_for_stop.trigger();
            ServiceControlHandlerResult::NoError
        } else {
            ServiceControlHandlerResult::NotImplemented
        }
    };

    let svc_name = service_name(instance);
    let status_handle = service_control_handler::register(&svc_name, event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::from_secs(15),
        process_id: None,
    })?;

    let rt = Runtime::new()?;
    rt.block_on(async {
        let mon_cfg = cfg.clone();
        let mon_state = std::sync::Arc::clone(&state);
        tokio::spawn(engine::monitor_loop(mon_cfg, mon_state, shutdown_rx.clone()));
        tokio::spawn(crate::welcome::watch_loop(shutdown_rx.clone()));

        let ev_tx = crate::events::start(std::sync::Arc::clone(&state), shutdown_rx.clone());
        // Permissions: create the shared state, feed it to the command interpreter
        // (tier enforcement) AND the !perm admin loop. @inv: this service path —
        // not just main.rs — must spawn both, or perms are dead in production.
        let perms = crate::permissions::start();
        // map snapshot first — legacy map-mods + /map/players both need it.
        let map_snap = crate::map_tracker::start_tracker(cfg.phantom_population);
        // Registry spine — the SAME single source of truth as console (registry::registered()).
        // This is the structural fix for the main.rs/service.rs drift: migrated mods light up in
        // production automatically, no second spawn list to forget.
        let world = crate::world::WorldCache::new();
        crate::world::spawn_world_poll(world.clone());
        crate::sysmon::spawn_sysmon();
        crate::banner::spawn_keeper();
        tokio::spawn(crate::registry::run_registered_mods(
            ev_tx.clone(),
            crate::registry::ModCtx { perms: perms.clone(), world, map: map_snap.clone() },
        ));
        // Every legacy (not-yet-trait) mod — the SAME run_all the console path calls (no drift).
        crate::legacy::run_all(&crate::legacy::LegacyCtx { ev_tx: ev_tx.clone(), perms: perms.clone(), map: map_snap.clone() });

        // Recurring restart scheduler — same as the console path (no drift).
        tokio::spawn(crate::schedule::run_scheduler(cfg.clone()));

        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: std::time::Duration::default(),
                process_id: None,
            })
            .ok();

        tracing::info!("service running — instance={}, port={}", instance, cfg.port);

        if let Err(e) = run_server(cfg, state, map_snap, perms.clone(), shutdown_rx).await {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    tracing::info!("service stopped");
    Ok(())
}

pub fn install(exe_path: &str, instance: &str) -> anyhow::Result<()> {
    let svc_name = service_name(instance);
    let display = match instance {
        "default" => "TurdMOD Engine Service".into(),
        id => format!("TurdMOD Engine Service ({})", id),
    };
    let bin_path = if instance == "default" {
        format!("\"{}\"", exe_path)
    } else {
        format!("\"{}\" --instance {}", exe_path, instance)
    };

    let out = std::process::Command::new("sc")
        .args([
            "create", &svc_name,
            "binPath=", &bin_path,
            "DisplayName=", &display,
            "start=", "auto",
        ])
        .output()?;
    tracing::info!("sc create: {} {}",
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}

pub fn uninstall(instance: &str) -> anyhow::Result<()> {
    let svc_name = service_name(instance);
    let out = std::process::Command::new("sc")
        .args(["delete", &svc_name])
        .output()?;
    tracing::info!("sc delete: {}", String::from_utf8_lossy(&out.stdout).trim());
    Ok(())
}
