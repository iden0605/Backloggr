// App/OS-level settings commands: launch-on-startup, the clip hotkey, and uninstall.

use crate::db::{settings, DbState};
use tauri::State;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

/// Uninstall-entry display names that count as "this app": the current product name plus the
/// pre-rename one (v0.5.0 and earlier installed as "Game Backlog").
#[cfg(target_os = "windows")]
const UNINSTALL_DISPLAY_NAMES: &[&str] = &["Backloggr", "Game Backlog"];

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

/// One-time cleanup after the "Game Backlog" → "Backloggr" product rename: launch-on-startup
/// was registered under the old name in HKCU\...\Run, pointing at an exe path the MSI upgrade
/// removed — and the autostart plugin (keyed on the new name) can't see it, so the user's
/// setting would silently break. If the stale entry exists, drop it and re-register under the
/// new name. Called from lib.rs setup on every launch; a no-op once migrated.
#[cfg(target_os = "windows")]
pub fn migrate_renamed_autostart(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    // `create` (not `open`) — the Run key needs write access to remove the stale value.
    let Ok(run) =
        windows_registry::CURRENT_USER.create("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    else {
        return;
    };
    if run.get_string("Game Backlog").is_ok() {
        let _ = run.remove_value("Game Backlog");
        let _ = app.autolaunch().enable();
    }
}

/// Finds this app's Windows uninstall entry. NSIS names its key after the product; MSI keys
/// are product-code GUIDs, so entries are also matched by DisplayName. Checked in both HKCU
/// (NSIS default currentUser mode) and HKLM (MSI, perMachine NSIS), plus the WOW6432Node view.
#[cfg(target_os = "windows")]
fn find_uninstall_string() -> Option<String> {
    use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE};
    const UNINSTALL_PATHS: &[&str] = &[
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ];
    let roots: [&Key; 2] = [CURRENT_USER, LOCAL_MACHINE];
    for root in roots {
        for path in UNINSTALL_PATHS {
            let Ok(key) = root.open(path) else { continue };
            for name in UNINSTALL_DISPLAY_NAMES {
                if let Ok(cmd) = key.open(name).and_then(|e| e.get_string("UninstallString")) {
                    return Some(cmd);
                }
            }
            let Ok(subkeys) = key.keys() else { continue };
            for sub in subkeys {
                let Ok(entry) = key.open(&sub) else { continue };
                let Ok(display) = entry.get_string("DisplayName") else { continue };
                if UNINSTALL_DISPLAY_NAMES.contains(&display.as_str()) {
                    if let Ok(cmd) = entry.get_string("UninstallString") {
                        return Some(cmd);
                    }
                }
            }
        }
    }
    None
}

/// Launches the app's own uninstaller and quits. Removes only the installed application —
/// the app-data dir (library DB, settings, clips) is deliberately untouched: neither the MSI
/// nor the NSIS uninstaller (deleteAppDataOnUninstall unset) reaches into it, so a reinstall
/// picks the library back up.
#[tauri::command]
pub fn uninstall_app(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let uninstall = find_uninstall_string().ok_or_else(|| {
            "Couldn't find the uninstaller — remove Backloggr from Windows Settings → Apps instead."
                .to_string()
        })?;

        // MSI entries record "MsiExec.exe /I{GUID}" (/I is modify/repair) — extract the
        // product GUID and run a proper /x uninstall instead of trusting the recorded verb.
        let mut cmd = if uninstall.to_lowercase().contains("msiexec") {
            let guid = uninstall
                .find('{')
                .and_then(|start| {
                    uninstall[start..]
                        .find('}')
                        .map(|end| &uninstall[start..start + end + 1])
                })
                .ok_or_else(|| format!("Unrecognized MSI uninstall entry: {uninstall}"))?;
            let mut c = std::process::Command::new("msiexec.exe");
            c.args(["/x", guid]);
            c
        } else {
            // NSIS: a (usually quoted) path to uninstall.exe, possibly with trailing args.
            let trimmed = uninstall.trim();
            let (program, rest) = match trimmed.strip_prefix('"') {
                Some(stripped) => match stripped.split_once('"') {
                    Some((p, r)) => (p.to_string(), r.trim().to_string()),
                    None => (stripped.to_string(), String::new()),
                },
                None => match trimmed.split_once(' ') {
                    Some((p, r)) => (p.to_string(), r.trim().to_string()),
                    None => (trimmed.to_string(), String::new()),
                },
            };
            let mut c = std::process::Command::new(program);
            if !rest.is_empty() {
                c.args(rest.split_whitespace());
            }
            c
        };
        // No hide_console here — the uninstaller's own UI (msiexec progress / NSIS wizard)
        // is exactly what should appear.
        cmd.spawn()
            .map_err(|e| format!("Couldn't launch the uninstaller: {e}"))?;

        // Quit so the app's files aren't in use by the time the uninstaller reaches them.
        // The child is its own process — exiting doesn't take it down with us.
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            app.exit(0);
        });
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err(
            "Uninstalling from Settings is only available on Windows — quit Backloggr and move it out of Applications instead."
                .to_string(),
        )
    }
}
