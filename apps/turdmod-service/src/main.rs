// turdmod-service — Windows Service + HTTP API for SCUM server management.
// 61 concurrent mod modules, 94 bridge handlers, 100+ chat commands.
// Console mode: --console. Install: --install. Multi-instance: --instance <name>.

mod achievements;
mod owner;
mod admin_files;
mod admin_log;
mod afk;
mod airdrop;
mod analytics;
mod announcements;
mod auto_announce;
mod restart_banner;
mod spa;
mod phantom;
mod hitmebaby;
mod nightvote;
mod registry;
mod world;
mod dims;
mod sysmon;
mod banner;
mod passive_control;
mod weather_cycle;
mod legacy;
mod auction;
mod auth;
mod backup;
mod schedule;
mod restore_campaign;
mod banking;
mod base_protection;
mod boss_fight;
mod bridge;
mod chat_cmds;
mod chat_filter;
mod chat_log;
mod checkpoint;
mod combat_log;
mod prestige;
mod factions;
mod overlay;
mod companions;
mod config;
mod death_recap;
mod death_notes;
mod discord_verify;
mod bounty_board;
mod bounty_hunter;
mod building_zones;
mod convoy;
mod clans;
mod duels;
mod economy;
mod elevated;
mod event_scheduler;
mod engine;
mod fast_travel;
mod fishing_tournament;
mod events;
mod objective_events;
mod gambling;
mod game_events;
mod god_mode;
mod horde;
mod inject;
mod jail;
mod kits;
mod leaderboard;
mod login_streak;
mod lottery;
mod loot_multiplier;
mod map_markers;
mod map_tracker;
mod mechman;
mod metabolism;
mod mod_list;
mod npc_contracts;
mod quests;
mod radio;
mod reputation;
mod rules;
mod scoreboard;
mod spawn_loadout;
// supply_drops: PARKED 2026-06-10 — bogus grid announcements (made-up 8x8 grid ≠ SCUM's map).
// Replaced by the real airdrop feature (IDEAS.md). Code kept for reference.
// mod supply_drops;
mod team_balance;
mod title_system;
mod trading;
mod weather_alerts;
mod npc;
mod ollama;
mod player_db;
mod safe_zones;
mod pvp_zones;
mod scavenger_hunt;
mod skill_boost;
mod smoker_time;
mod trader_refill;
// tow: PARKED 2026-06-10 — teleport-based follow-tow fought the towed vehicle's physics
// (bounce + collision damage + a server crash). Needs the physics-safe rebuild: disable
// the towed vehicle's SetSimulatePhysics + rigid-attach, then re-enable on !untow. Code +
// the bridge `towStep` handler are kept as the WIP foundation. See IDEAS.md.
// mod tow;
mod vehicle_interactions;
// vehicle_ownership: DEPRECATED 2026-06-10 — collided with chat_cmds (duplicate !register etc.,
// different JSON schema). Unregistered from the registry; module no longer compiled.
// mod vehicle_ownership;
mod zilla_protection;
mod perf_monitor;
mod permissions;
mod pipe_rpc;
mod player_shops;
mod racing;
mod raid_alerts;
mod referral;
mod rcon;
mod rcon_be;
mod router;
mod scumdb;
mod scummap_push;
mod scheduler;
mod spawn_protection;
mod server;
mod service;
mod shutdown;
mod teleport;
mod vac_screening;
mod territory;
mod vehicle_registry;
mod vehicle_durability;
mod pvp_rotation;
mod vehicle_repo;
mod tree_preserve;
mod vehicles;
mod voting;
mod warzone;
mod welcome;

use config::Config;
use engine::new_state;
use tracing_subscriber::{fmt, EnvFilter};

fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    let instance = args.windows(2)
        .find(|w| w[0] == "--instance")
        .map(|w| w[1].as_str())
        .unwrap_or("default");

    if args.iter().any(|a| a == "--console") {
        run_console(instance)
    } else if args.iter().any(|a| a == "--install") {
        let exe = std::env::current_exe()?;
        service::install(exe.to_str().unwrap_or("turdmod-service.exe"), instance)?;
        println!("Service installed: {}", service::service_name(instance));
        Ok(())
    } else if args.iter().any(|a| a == "--uninstall") {
        service::uninstall(instance)?;
        println!("Service removed: {}", service::service_name(instance));
        Ok(())
    } else {
        service::dispatch(instance).map_err(|e| anyhow::anyhow!("service dispatch: {}", e))
    }
}

#[tokio::main]
async fn run_console(instance: &str) -> anyhow::Result<()> {
    let cfg = Config::load_or_default(instance);
    // @inv: install owner identity BEFORE any mod starts — owner-gated mods
    //   match nobody until this runs, which is the safe direction but useless.
    owner::init(cfg.owner_steam_ids.clone(), cfg.owner_name.clone());
    if owner::is_unconfigured() {
        tracing::warn!("no owner_steam_ids/owner_name in service.json — owner-gated mods will ignore everyone");
    }
    // Resolve the SCUM.db path process-wide so mods that call scumdb::db_path(None)
    // (chat_cmds profile/lock lookups, etc.) read the configured DB, not the hardcoded default.
    scumdb::set_db_path(&cfg.scumdb_path);
    restore_campaign::set_instance(&cfg.instance_id);
    // Resolve the banner channel (Notifications.json) + set its silent resting state so it only
    // fires when WE write a banner (scheduled or ad-hoc). @dep banner::fire / write_scheduled.
    if let Some(np) = restart_banner::notif_path(&cfg) {
        banner::set_notif_path(np);
        banner::write_quiet_default();
    }
    let state = new_state();
    let (_shutdown, shutdown_rx) = shutdown::Shutdown::new();

    // Ctrl+C fires shutdown
    let shutdown_tx = _shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Ctrl+C — shutting down");
        shutdown_tx.trigger();
    });

    let mon_cfg = cfg.clone();
    let mon_state = std::sync::Arc::clone(&state);
    tokio::spawn(engine::monitor_loop(mon_cfg, mon_state, shutdown_rx.clone()));
    tokio::spawn(welcome::watch_loop(shutdown_rx.clone()));

    // MOTD kill moved to engine.rs start_server() — runs BEFORE server boots

    let ev_tx = events::start(std::sync::Arc::clone(&state), shutdown_rx.clone());
    // Permissions state is created early so the command interpreter can enforce
    // the tier ladder ("who can use what"). Same handle feeds permissions_loop.
    let perms = permissions::start();
    // router is now a dispatch function called by chat_cmds (the interpreter),
    // not a standalone event loop — no spawn needed.
    // restart_scheduler intentionally NOT spawned: the box's TurdMODRestart Windows task is the
    // SOLE restart driver (3h, OOM-leak mitigation). One mechanism only.
    // map snapshot first — legacy map-mods + /map/players both need it (passed to run_server below).
    let map_snap = map_tracker::start_tracker(cfg.phantom_population);
    // Registry spine: live world perception + the single registered-mod router (hitmebaby
    // migrated; more mods move here in waves). Replaces hitmebaby's standalone loop, and is
    // driven IDENTICALLY by the service path below — one source of truth, no spawn-list drift.
    let world = world::WorldCache::new();
    world::spawn_world_poll(world.clone());
    sysmon::spawn_sysmon();
    banner::spawn_keeper();
    tokio::spawn(registry::run_registered_mods(
        ev_tx.clone(),
        registry::ModCtx { perms: perms.clone(), world, map: map_snap.clone() },
    ));
    // Every legacy (not-yet-trait) mod — ONE source of truth shared identically with the service
    // path (legacy::run_all). Ends the main.rs/service.rs spawn-list drift for ALL mods.
    legacy::run_all(&legacy::LegacyCtx { ev_tx: ev_tx.clone(), perms: perms.clone(), map: map_snap.clone() });

    // Recurring restart scheduler — interval-based, fires via /server/restart.
    tokio::spawn(schedule::run_scheduler(cfg.clone()));

    tracing::info!("console mode — instance={}, port={}", instance, cfg.port);
    server::run_server(cfg, state, map_snap, perms.clone(), shutdown_rx).await
}
