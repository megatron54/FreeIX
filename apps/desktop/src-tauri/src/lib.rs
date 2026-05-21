use std::sync::Arc;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

mod commands;
mod state;

use commands::*;
use state::AppState;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("freeix=debug".parse().unwrap())
                .add_directive("info".parse().unwrap()),
        )
        .init();

    tracing::info!("FreeIX Desktop starting");

    let app_state = Arc::new(AppState::new());

    tauri::Builder::default()
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            toggle_protection,
            get_status,
            get_stats,
            get_config,
            update_config,
            add_whitelist,
            remove_whitelist,
            add_blacklist,
            remove_blacklist,
            get_whitelist,
            get_blacklist,
            get_dns_providers,
            set_dns_provider,
            get_logs,
            get_top_blocked,
            update_blocklists,
        ])
        .setup(move |_app| {
            tracing::info!("FreeIX initialized successfully");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running FreeIX");
}
