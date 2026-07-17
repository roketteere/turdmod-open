// TurdMOD Manager — Tauri 2 application entry point.
//
// Layout:
//   commands             Mod install / uninstall / list IPC.
//   mod_install          Download → verify SHA-256 → extract → place into mods dir.
//   mod_index            Talks to https://turdmod.com/api/mods.
//   scum_paths           Auto-detect SCUM install + mods dir.
//   server / server_commands
//                        SFTP / FTP / RCON connectors for remote dedicated servers.
//   account / account_commands
//                        OAuth desktop flow + OS-keychain token storage.

use tauri::Manager;

mod account;
mod account_commands;
mod admin_commands;
mod admin_files;
mod assist;
mod claude_status;
mod commands;
mod companion;
mod companion_commands;
mod dump;
mod dump_commands;
mod engine;
mod inject;
mod engine_commands;
mod engine_events;
mod engine_rpc;
mod loader_rpc;
mod mod_index;
mod ollama_pool;
mod mod_install;
mod remote;
mod remote_commands;
mod tunnel;
mod mod_state;
mod mod_state_commands;
mod scum_paths;
mod server;
mod server_commands;
mod settings;
mod settings_commands;
mod snapshot;
mod snapshot_commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,turdmod_manager_lib=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::manager_smoke_test,
            commands::manager_smoke_test_full,
            commands::manager_detect_scum,
            commands::manager_list_detected_installs,
            commands::manager_get_active_target,
            commands::manager_set_active_target,
            commands::manager_set_install_path,
            commands::manager_get_install_path,
            commands::manager_list_mods,
            commands::manager_get_mod,
            commands::manager_install_mod,
            commands::manager_uninstall_mod,
            commands::manager_list_installed,
            commands::manager_write_text_file,
            commands::manager_append_text_file,
            commands::manager_read_text_file,
            commands::manager_find_scumdump_database,
            commands::manager_engine_rpc_log_path,
            commands::manager_engine_rpc_log_tail,
            commands::manager_open_in_default_app,
            commands::manager_file_meta,
            account_commands::manager_account_status,
            account_commands::manager_account_sign_in_start,
            account_commands::manager_account_paste_token,
            account_commands::manager_account_sign_out,
            server_commands::manager_server_add,
            server_commands::manager_server_remove,
            server_commands::manager_server_list,
            server_commands::manager_server_test,
            server_commands::manager_server_upload_mod,
            server_commands::manager_server_remove_remote_mod,
            server_commands::manager_server_list_remote_mods,
            server_commands::manager_server_tail_log,
            server_commands::manager_server_rcon,
            server_commands::manager_server_known_logs,
            companion_commands::companion_start,
            companion_commands::companion_stop,
            companion_commands::companion_restart,
            companion_commands::companion_get_status,
            engine_commands::engine_install,
            engine_commands::engine_start,
            engine_commands::engine_stop,
            engine_commands::engine_get_status,
            engine_commands::engine_rpc,
            loader_rpc::loader_rpc,
            settings_commands::settings_get_all,
            settings_commands::settings_set_partial,
            settings_commands::settings_test_logs_dir,
            settings_commands::settings_test_rcon,
            settings_commands::manager_detect_node,
            mod_state_commands::manager_get_mod_state,
            mod_state_commands::manager_set_mod_frozen,
            mod_state_commands::manager_read_mod_config,
            mod_state_commands::manager_write_mod_config,
            mod_state_commands::manager_get_mod_manifest,
            admin_commands::manager_server_read_admin_file,
            admin_commands::manager_server_write_admin_file,
            admin_commands::manager_server_parse_server_settings,
            admin_commands::manager_server_save_server_settings_partial,
            ollama_pool::ollama_pool_list_endpoints,
            ollama_pool::ollama_pool_endpoints_file_path,
            ollama_pool::ollama_pool_health,
            ollama_pool::ollama_pool_dispatch,
            claude_status::claude_status,
            dump_commands::dump_status,
            dump_commands::dump_check_updates,
            dump_commands::dump_run_phase,
            dump_commands::dump_run_all,
            dump_commands::dump_open_folder,
            dump_commands::dump_extract_aes,
            dump_commands::dump_archive_entries,
            dump_commands::dump_diff_summary,
            dump_commands::dump_list_builds,
            dump_commands::dump_inject_dumper7,
            dump_commands::dump_inject_dll,
            assist::assist_gpu_info,
            assist::assist_list_models,
            assist::assist_pull_model,
            assist::assist_chat,
            assist::assist_summarize_diff,
            assist::assist_explain_phase_log,
            snapshot_commands::snapshot_install,
            snapshot_commands::snapshot_list,
            remote_commands::remote_get_config,
            remote_commands::remote_set_config,
            remote_commands::remote_health,
            remote_commands::remote_status,
            remote_commands::remote_server_start,
            remote_commands::remote_server_stop,
            remote_commands::remote_server_restart,
            remote_commands::remote_server_status,
            remote_commands::remote_server_battleye,
            remote_commands::remote_admin_file_get,
            remote_commands::remote_admin_file_set,
            remote_commands::remote_data_file_get,
            remote_commands::remote_data_file_set,
            remote_commands::remote_vehicles_detail,
            remote_commands::remote_vehicles_repo,
            remote_commands::remote_permissions_get,
            remote_commands::remote_permissions_set,
            remote_commands::tunnel_ensure,
            remote_commands::remote_server_schedule_get,
            remote_commands::remote_server_schedule_set,
            remote_commands::remote_elevated_get,
            remote_commands::remote_elevated_set,
            remote_commands::remote_loot_files,
            remote_commands::remote_loot_file_get,
            remote_commands::remote_loot_file_set,
            remote_commands::remote_server_logs,
            remote_commands::read_remote_text_file,
            remote_commands::remote_engine_rpc,
            remote_commands::remote_scumpilot_status,
            remote_commands::remote_scumpilot_pause,
            remote_commands::remote_scumpilot_resume,
            remote_commands::monitor_mods,
            remote_commands::monitor_activity,
            remote_commands::monitor_system,
            remote_commands::monitor_players,
            remote_commands::control_mod,
            remote_commands::scumdb_tables,
            remote_commands::scumdb_schema,
            remote_commands::scumdb_rows,
            remote_commands::scumdb_player_card,
            remote_commands::scumdb_player_inventory,
            remote_commands::scumdb_set_skill,
            remote_commands::scumdb_set_attribute,
            remote_commands::scumdb_map_vehicles,
            remote_commands::scumdb_last_positions,
            remote_commands::scumdb_map_zones,
            remote_commands::scumdb_traders,
            remote_commands::tunnel_get_config,
            remote_commands::tunnel_set_config,
            remote_commands::tunnel_start,
            remote_commands::tunnel_stop,
            remote_commands::tunnel_status,
        ])
        .setup(|app| {
            tracing::info!(
                "TurdMOD Manager booting — version {}",
                env!("CARGO_PKG_VERSION")
            );
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.open_devtools();
            }
            app.manage(companion::CompanionState::new());
            app.manage(engine::EngineState::new());
            app.manage(settings::SettingsState);

            // Auto-start the remote server monitoring tunnel if remote is configured + enabled, so the Live
            // Monitor's "remote server" view works the moment the app opens — persistence without a system-
            // level SSH task (the manager only needs the tunnel while it's running). Best-effort.
            if crate::remote::RemoteConfig::load().is_some() {
                match crate::tunnel::start(&crate::tunnel::TunnelConfig::load()) {
                    Ok(_) => tracing::info!("remote server monitoring tunnel auto-started"),
                    Err(e) => tracing::warn!("tunnel auto-start failed: {}", e),
                }
            }

            // Spawn the long-lived engine event listener. Reads the
            // bridge's named-pipe event firehose and rebroadcasts every
            // event to the React UI as Tauri `engine://event`. The
            // listener auto-reconnects whenever the engine isn't running
            // (pipe.txt missing or pipe closed), so Manager boot order
            // doesn't matter.
            engine_events::spawn_listener(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
