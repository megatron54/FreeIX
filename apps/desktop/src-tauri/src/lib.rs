use std::sync::Arc;

use tauri::{Manager, menu::{Menu, MenuItem}, tray::TrayIconBuilder, image::Image};
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
        .setup(move |app| {
            // Create tray menu
            let quit = MenuItem::with_id(app, "quit", "Quit FreeIX", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            // Build tray icon
            let icon = app.default_window_icon().cloned().unwrap_or_else(|| Image::new_owned(vec![0; 32*32*4], 32, 32));
            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("FreeIX - DNS Ad Blocker")
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            tracing::info!("FreeIX initialized with system tray");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running FreeIX");
}
