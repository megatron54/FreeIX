use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App, Manager,
};

pub fn create_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let toggle = MenuItem::with_id(app, "toggle", "Disable Protection", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open FreeIX", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&toggle, &open, &quit])?;

    TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("FreeIX - DNS Ad Blocker")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                tracing::info!("Tray: toggle protection");
            }
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
