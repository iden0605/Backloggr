mod clipper;
mod commands;
mod db;
// Native WASAPI system-audio capture for clips — Windows-only (macOS uses a loopback device).
#[cfg(target_os = "windows")]
mod loopback;
mod overlay;
mod rawg;
mod steam;
mod tracker;
mod tray;

use clipper::{CaptureSlot, CaptureState, FocusLog};
use db::DbState;
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

// Default "save the last N seconds" hotkey — hardcoded for now, same pattern as the RAWG key /
// worker URL, until Settings (Stage 8) grows a real capture-settings section to make it
// user-configurable.
const CLIP_HOTKEY: &str = "Alt+F9";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Registered first, per the plugin's docs: launching a second copy of the app must bail
        // out immediately (focusing the existing window instead) — a duplicate instance would
        // register a second global hotkey handler and spawn a second full-screen ffmpeg capture
        // that ignores the first instance's pause/scope decisions. (Observed for real when dev
        // rebuilds left zombie instances recording the whole screen nonstop.)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // show() as well as focus: with background mode the window may be hidden, and a
            // relaunch is the user saying "open the app".
            tray::show_main_window(app);
        }))
        // Launch-on-startup (Settings toggle → commands.rs get/set_autostart_enabled). Autostart
        // launches pass --hidden so the app boots straight to the tray: the whole point of
        // starting with the OS is background tracking, not a window over the desktop at login.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        clipper::save_clip_from_hotkey(app.clone());
                    }
                })
                .build(),
        )
        .manage(CaptureState(Mutex::new(CaptureSlot::idle())))
        .manage(FocusLog(Mutex::new(VecDeque::new())))
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::init(&app_data_dir);
            app.manage(DbState(Mutex::new(conn)));
            // A previous life of the app that died without its exit cleanup (crash, force-kill,
            // dev rebuild) may have left its capture ffmpeg running — kill it before anything
            // else records or reads the buffer.
            clipper::reap_orphan_capture(app.handle());
            // Capture only ever starts/stops from tracker.rs, at the same points it opens/closes
            // `sessions` rows — nothing records while no game is being played.
            tracker::start(app.handle().clone());

            if let Err(e) = app.global_shortcut().register(CLIP_HOTKEY) {
                eprintln!("clipper: failed to register hotkey {CLIP_HOTKEY}: {e}");
            }
            // Clip feedback shows through the in-game overlay window, not OS notifications —
            // macOS suppresses notification banners while a fullscreen app is frontmost, which
            // is precisely when clips get saved. Created here (hidden) because window creation
            // must happen on the main thread; toasts fire from the hotkey's async context.
            overlay::init(app.handle());

            if let Err(e) = tray::init(app.handle()) {
                eprintln!("tray: failed to create tray icon: {e}");
            }
            // Autostart launches pass --hidden (see the autostart plugin registration): start in
            // the tray, tracking in the background, without flashing a window at login.
            if std::env::args().any(|arg| arg == "--hidden") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        // Background mode: closing the main window hides it to the tray instead of quitting, so
        // playtime tracking and the clip hotkey keep working. Actually quitting goes through the
        // tray menu's Quit (app.exit → RunEvent::Exit → clipper::stop).
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library,
            commands::get_game_stats,
            commands::search_rawg,
            commands::add_game,
            commands::update_game_status,
            commands::delete_game,
            commands::set_game_exe_name,
            commands::get_dashboard_stats,
            commands::get_currently_playing,
            commands::get_game_details,
            commands::chat_recommend,
            commands::get_dashboard_recommendations,
            commands::list_chats,
            commands::get_chat,
            commands::save_chat,
            commands::delete_chat,
            clipper::get_clips,
            clipper::delete_clip,
            clipper::save_clip,
            clipper::get_clip_seconds,
            clipper::set_clip_seconds,
            clipper::get_mic_enabled,
            clipper::set_mic_enabled,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
            commands::fetch_steam_library,
            commands::import_steam_games,
            commands::get_steam_profile
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::Exit => clipper::stop(app_handle),
                // macOS: clicking the Dock icon while the window is hidden-to-tray must bring it
                // back — without this the Dock click does nothing and the app looks hung.
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => tray::show_main_window(app_handle),
                _ => {}
            }
        });
}
