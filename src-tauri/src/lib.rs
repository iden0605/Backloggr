mod clipper;
mod commands;
mod db;
mod rawg;
mod tracker;

use clipper::{CaptureSlot, CaptureState, FocusLog};
use db::DbState;
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

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
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
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
            match app.notification().request_permission() {
                Ok(state) => eprintln!("clipper: notification permission: {state:?}"),
                Err(e) => eprintln!("clipper: failed to request notification permission: {e}"),
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_backlog,
            commands::search_rawg,
            commands::add_game,
            commands::update_game_status,
            commands::delete_game,
            commands::set_game_exe_name,
            commands::get_dashboard_stats,
            commands::get_currently_playing,
            commands::get_playtime_totals,
            commands::get_game_details,
            commands::chat_recommend,
            commands::get_dashboard_recommendations,
            clipper::get_clips,
            clipper::delete_clip,
            clipper::save_clip,
            clipper::get_clip_seconds,
            clipper::set_clip_seconds
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                clipper::stop(app_handle);
            }
        });
}
