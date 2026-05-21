use std::sync::Arc;

use tauri::Manager;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod commands;
mod state;

use commands::*;
use state::AppState;

pub fn run() {
    // Log to file at C:\FreeIX\freeix.log
    let log_path = std::path::PathBuf::from("C:\\FreeIX\\freeix.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open log file");

    tracing_subscriber::registry()
        .with(
            EnvFilter::from_default_env()
                .add_directive("freeix=debug".parse().unwrap())
                .add_directive("info".parse().unwrap()),
        )
        .with(fmt::layer().with_writer(std::sync::Mutex::new(file)).with_ansi(false))
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
        .setup(move |app| {
            tracing::info!("FreeIX initialized successfully");

            // Auto-start protection if configured
            let config = app_state.config.read().clone();
            if config.auto_start {
                let state = app_state.clone();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tracing::info!("Auto-starting DNS protection...");
                    match commands::auto_start_protection(state, app_handle).await {
                        Ok(_) => tracing::info!("Auto-start protection enabled"),
                        Err(e) => tracing::warn!("Auto-start protection failed: {}", e),
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running FreeIX");
}
