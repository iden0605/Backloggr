mod clipper;
mod commands;
mod db;
mod rawg;
mod tracker;

use db::DbState;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::init(&app_data_dir);
            app.manage(DbState(Mutex::new(conn)));
            tracker::start(app.handle().clone());
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
            commands::get_playtime_totals
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
