// App/OS-level settings commands: launch-on-startup and the clip hotkey.

use crate::db::{settings, DbState};
use tauri::State;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

/// Fallback when the user has never picked a hotkey. lib.rs registers the persisted (or
/// this default) hotkey at startup; `set_clip_hotkey` swaps the registration at runtime.
pub const DEFAULT_CLIP_HOTKEY: &str = "Alt+F9";
const CLIP_HOTKEY_KEY: &str = "clip_hotkey";

/// The persisted clip hotkey — what the running registration SHOULD be. Also read by
/// lib.rs at startup and by every piece of UI copy that names the hotkey.
#[tauri::command]
pub fn get_clip_hotkey(db: State<DbState>) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(settings::get(&conn, CLIP_HOTKEY_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| DEFAULT_CLIP_HOTKEY.to_string()))
}

/// Swaps the live registration to `hotkey` and persists it. The new combo is registered
/// BEFORE the old one is dropped: if the OS rejects it (already taken by another app,
/// unparseable), the error surfaces to Settings and the old hotkey keeps working.
#[tauri::command]
pub fn set_clip_hotkey(
    app: tauri::AppHandle,
    db: State<DbState>,
    hotkey: String,
) -> Result<(), String> {
    let current = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        settings::get(&conn, CLIP_HOTKEY_KEY)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| DEFAULT_CLIP_HOTKEY.to_string())
    };
    if hotkey == current {
        return Ok(());
    }

    let shortcuts = app.global_shortcut();
    shortcuts
        .register(hotkey.as_str())
        .map_err(|e| format!("Couldn't register {hotkey}: {e}"))?;
    // Best-effort: a stale old registration only means two live hotkeys until restart.
    let _ = shortcuts.unregister(current.as_str());

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    settings::set(&conn, CLIP_HOTKEY_KEY, &hotkey).map_err(|e| e.to_string())
}

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
