fn main() {
    // Embed Windows manifest for admin elevation
    #[cfg(target_os = "windows")]
    {
        let res = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("freeix.manifest"));
        let attrs = tauri_build::Attributes::new().windows_attributes(res);
        tauri_build::try_build(attrs).expect("failed to run tauri_build");
    }
    #[cfg(not(target_os = "windows"))]
    {
        tauri_build::build();
    }
}
