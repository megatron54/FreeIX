use std::sync::Arc;

use tauri::Manager;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod commands;
mod state;

use commands::*;
use state::AppState;

const DNS_ACTIVE_FLAG: &str = "C:\\FreeIX\\dns-active.flag";

/// Restore DNS settings to system defaults (resets all active adapters).
fn restore_dns_on_exit() {
    tracing::info!("Restoring DNS settings...");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("powershell")
            .args(&[
                "-NoProfile", "-WindowStyle", "Hidden", "-Command",
                "Start-Process powershell -ArgumentList '-NoProfile -WindowStyle Hidden -Command Get-NetAdapter | Where-Object { $_.Status -eq ''Up'' } | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses }' -Verb RunAs -Wait -WindowStyle Hidden"
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    let _ = std::fs::remove_file(DNS_ACTIVE_FLAG);
}

/// If a previous session crashed while DNS was redirected, restore it now.
fn recover_from_crash() {
    if std::path::Path::new(DNS_ACTIVE_FLAG).exists() {
        tracing::warn!("Detected dns-active.flag from previous crash, restoring DNS...");
        restore_dns_on_exit();
    }
}

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
                .add_directive("freeix=info".parse().unwrap())
                .add_directive("freeix_dns_engine=info".parse().unwrap())
                .add_directive("warn".parse().unwrap()),
        )
        .with(fmt::layer().with_writer(std::sync::Mutex::new(file)).with_ansi(false))
        .init();

    tracing::info!("FreeIX Desktop starting");

    // Recover DNS if previous session crashed while protection was active
    recover_from_crash();

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
            is_setup_complete,
            complete_setup,
            set_system_dns_to_local,
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

            // Spawn blocklist auto-update task (every 24 hours)
            let state_for_update = app_state.clone();
            tauri::async_runtime::spawn(async move {
                // Wait 5 minutes before first update (let app stabilize)
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                loop {
                    tracing::info!("Auto-updating blocklists...");
                    match commands::update_blocklists_inner(&state_for_update).await {
                        Ok(count) => tracing::info!(count, "Blocklists auto-updated"),
                        Err(e) => tracing::warn!("Blocklist auto-update failed: {}", e),
                    }
                    // Sleep 24 hours
                    tokio::time::sleep(std::time::Duration::from_secs(86400)).await;
                }
            });

            // Write flag file indicating DNS is being managed
            let _ = std::fs::create_dir_all("C:\\FreeIX");
            let _ = std::fs::write(DNS_ACTIVE_FLAG, "127.0.0.1");

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error building FreeIX")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                tracing::info!("FreeIX shutting down, restoring DNS...");
                restore_dns_on_exit();
            }
        });
}
