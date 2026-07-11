// Library CRUD + per-game stats: the games table's command surface (add/search/mark/delete,
// exe linking) and the Library detail page's stat tiles/sparkline.

use crate::db::DbState;
use crate::rawg::{self, RawgGameDetail, RawgGameResult};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub rawg_id: Option<i64>,
    pub name: String,
    pub cover_url: Option<String>,
    pub genre: Option<String>,
    pub platform: Option<String>,
    pub status: String,
    pub rating: Option<i64>,
    pub notes: Option<String>,
    pub exe_name: Option<String>,
    pub added_at: String,
    pub completed_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGame {
    #[serde(flatten)]
    pub game: Game,
    pub total_seconds: i64,
    /// Best-known end of the most recent session (falls back through the heartbeat to the
    /// session start for a still-open session). NULL = never played.
    pub last_played_at: Option<String>,
    pub session_count: i64,
}

/// The Library page's one read: every game with its session aggregates, so activity states
/// (played / never played) and the last-played sort come from a single query.
#[tauri::command]
pub fn get_library(db: State<DbState>) -> Result<Vec<LibraryGame>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.rawg_id, g.name, g.cover_url, g.genre, g.platform, g.status,
                    g.rating, g.notes, g.exe_name, g.added_at, g.completed_at,
                    COALESCE(SUM(s.duration_seconds), 0),
                    MAX(COALESCE(s.ended_at, s.last_seen_at, s.started_at)),
                    COUNT(s.id)
             FROM games g LEFT JOIN sessions s ON s.game_id = g.id
             GROUP BY g.id ORDER BY g.added_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let games = stmt
        .query_map([], |row| {
            Ok(LibraryGame {
                game: Game {
                    id: row.get(0)?,
                    rawg_id: row.get(1)?,
                    name: row.get(2)?,
                    cover_url: row.get(3)?,
                    genre: row.get(4)?,
                    platform: row.get(5)?,
                    status: row.get(6)?,
                    rating: row.get(7)?,
                    notes: row.get(8)?,
                    exe_name: row.get(9)?,
                    added_at: row.get(10)?,
                    completed_at: row.get(11)?,
                },
                total_seconds: row.get(12)?,
                last_played_at: row.get(13)?,
                session_count: row.get(14)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(games)
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

    let (total_seconds, last_played_at, session_count, finished_count): (i64, Option<String>, i64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds), 0),
                    MAX(COALESCE(ended_at, last_seen_at, started_at)),
                    COUNT(id),
                    COUNT(duration_seconds)
             FROM sessions WHERE game_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| e.to_string())?;

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
        total_seconds,
        last_played_at,
        session_count,
        avg_session_seconds: if finished_count > 0 { total_seconds / finished_count } else { 0 },
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
    conn.execute(
        "INSERT INTO games (rawg_id, name, cover_url, genre, platform) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(rawg_id) DO NOTHING",
        (&rawg_id, &name, &cover_url, &genre, &platform),
    )
    .map_err(|e| e.to_string())?;

    conn.query_row(
        "SELECT id FROM games WHERE rawg_id = ?1",
        [rawg_id],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_game_status(db: State<DbState>, id: i64, status: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let completed_at_clause = if status == "completed" {
        "completed_at = CURRENT_TIMESTAMP"
    } else {
        "completed_at = NULL"
    };
    conn.execute(
        &format!("UPDATE games SET status = ?1, {completed_at_clause} WHERE id = ?2"),
        rusqlite::params![status, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_game(db: State<DbState>, id: i64) -> Result<(), String> {
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    // `sessions.game_id`/`clips.game_id` reference `games(id)` and `foreign_keys = ON`, so any
    // game with tracked playtime or saved clips must have those rows cleared first or the
    // delete fails with a foreign key constraint error.
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM sessions WHERE game_id = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM clips WHERE game_id = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM games WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_game_exe_name(db: State<DbState>, id: i64, exe_name: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE games SET exe_name = ?1 WHERE id = ?2",
        rusqlite::params![exe_name, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_game_details(rawg_id: i64) -> Result<RawgGameDetail, String> {
    rawg::get_game_details(rawg_id).await
}
