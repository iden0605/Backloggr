// System tray icon (Stage 7). The app's real home once background mode exists: closing the main
// window hides it instead of quitting (see lib.rs's on_window_event), so the tracker keeps
// logging sessions and the clip hotkey keeps working with no window open — the tray is then the
// only always-visible handle to reopen the window or actually quit. Quit goes through
// app.exit(0), which fires RunEvent::Exit and lets the capture ffmpeg shut down cleanly.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

/// Reopen the main window from the background: unhide, unminimize, focus.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Build the tray icon. Called once from setup; the returned tray lives for the app's lifetime
/// (tauri keeps it registered internally by id).
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open backloggr", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(
            app.default_window_icon()
                .expect("app bundle has window icons configured")
                .clone(),
        )
        .tooltip("backloggr")
        .menu(&menu)
        // Left-click opens the app (the Medal/Discord convention); the menu stays on right-click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
