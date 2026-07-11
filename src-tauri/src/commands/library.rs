// Library CRUD + per-game stats: the games table's command surface (add/search/mark/delete,
// exe linking) and the Library detail page's stat tiles/sparkline.

use crate::db::games::{self, LibraryGame, Status};
use crate::db::DbState;
use crate::rawg::{self, RawgGameDetail, RawgGameResult};
use serde::Serialize;
use tauri::State;

/// The Library page's one read: every game with its session aggregates, so activity states
/// (played / never played) and the last-played sort come from a single query.
#[tauri::command]
pub fn get_library(db: State<DbState>) -> Result<Vec<LibraryGame>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    games::library(&conn).map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStats {
    pub total_seconds: i64,
    pub last_played_at: Option<String>,
    pub session_count: i64,
    /// Mean length of finished sessions only — an open session has no duration yet.
    pub avg_session_seconds: i64,
    /// Playtime bucketed into rolling 7-day windows, oldest first, index 7 = the last 7 days.
    pub weekly_seconds: Vec<i64>,
}

/// Per-game stats for the Library detail page: headline tiles + the 8-week trend sparkline.
#[tauri::command]
pub fn get_game_stats(db: State<DbState>, id: i64) -> Result<GameStats, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let stats = games::playtime_stats(&conn, id).map_err(|e| e.to_string())?;

    let mut weekly_seconds = vec![0i64; 8];
    let mut stmt = conn
        .prepare(
            "SELECT CAST((julianday('now') - julianday(started_at)) / 7 AS INTEGER),
                    SUM(duration_seconds)
             FROM sessions
             WHERE game_id = ?1 AND duration_seconds IS NOT NULL
               AND julianday('now') - julianday(started_at) < 56
             GROUP BY 1",
        )
        .map_err(|e| e.to_string())?;
    let buckets = stmt
        .query_map([id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    for bucket in buckets {
        let (weeks_ago, seconds) = bucket.map_err(|e| e.to_string())?;
        if (0..8).contains(&weeks_ago) {
            weekly_seconds[(7 - weeks_ago) as usize] = seconds;
        }
    }

    Ok(GameStats {
        total_seconds: stats.total_seconds,
        last_played_at: stats.last_played_at,
        session_count: stats.session_count,
        avg_session_seconds: if stats.finished_count > 0 {
            stats.total_seconds / stats.finished_count
        } else {
            0
        },
        weekly_seconds,
    })
}

#[tauri::command]
pub async fn search_rawg(query: String) -> Result<Vec<RawgGameResult>, String> {
    rawg::search_games(&query).await
}

#[tauri::command]
pub fn add_game(
    db: State<DbState>,
    rawg_id: i64,
    name: String,
    cover_url: Option<String>,
    genre: Option<String>,
    platform: Option<String>,
) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    games::add_by_rawg(
        &conn,
        rawg_id,
        &name,
        cover_url.as_deref(),
        genre.as_deref(),
        platform.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_game_status(db: State<DbState>, id: i64, status: String) -> Result<(), String> {
    let status = Status::parse(&status).ok_or_else(|| format!("invalid status: {status}"))?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    games::set_status(&conn, id, status).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_game(db: State<DbState>, id: i64) -> Result<(), String> {
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    games::delete_cascading(&mut conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_game_exe_name(db: State<DbState>, id: i64, exe_name: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    games::set_exe_name(&conn, id, exe_name.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_game_details(rawg_id: i64) -> Result<RawgGameDetail, String> {
    rawg::get_game_details(rawg_id).await
}
