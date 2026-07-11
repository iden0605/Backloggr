// App/OS-level settings commands (currently just launch-on-startup).

/// Whether the app is registered to launch at login/startup. State lives in the OS itself
/// (LaunchAgent plist on macOS, registry Run key on Windows) via tauri-plugin-autostart — not in
/// the settings table, so an externally-removed entry reads back correctly as disabled.
#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(|e| e.to_string())
    } else {
        autolaunch.disable().map_err(|e| e.to_string())
    }
}
