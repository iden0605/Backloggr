use crate::db::DbState;
use crate::rawg::{self, RawgGameResult};
use rusqlite::OptionalExtension;
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

#[tauri::command]
pub fn get_backlog(db: State<DbState>) -> Result<Vec<Game>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, rawg_id, name, cover_url, genre, platform, status, rating, notes, exe_name, added_at, completed_at FROM games ORDER BY added_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let games = stmt
        .query_map([], |row| {
            Ok(Game {
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
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(games)
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTotalPlaytime {
    pub game_id: i64,
    pub total_seconds: i64,
}

/// All-time total playtime per game — used by the Backlog view to show playtime inline without
/// pulling in the rest of `get_dashboard_stats`'s period-scoped/chart data it doesn't need.
#[tauri::command]
pub fn get_playtime_totals(db: State<DbState>) -> Result<Vec<GameTotalPlaytime>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT game_id, SUM(duration_seconds) FROM sessions
             WHERE duration_seconds IS NOT NULL GROUP BY game_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(GameTotalPlaytime {
                game_id: row.get(0)?,
                total_seconds: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePlaytime {
    pub game_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub total_seconds: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPlaytime {
    pub date: String,
    pub total_seconds: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub total_playtime_seconds: i64,
    pub games_completed: i64,
    pub games_in_backlog: i64,
    pub games_playing: i64,
    pub games_played: Vec<GamePlaytime>,
    pub playtime_last_7_days: Vec<DailyPlaytime>,
}

/// Maps a period selector to a SQL date-filter clause on `s.started_at`. `day` and `week` are
/// rolling windows (last 24h / last 7 days), not calendar-aligned, to match "how much have I
/// played recently" rather than "since Monday".
fn period_filter(period: &str) -> Result<&'static str, String> {
    match period {
        "day" => Ok("date(s.started_at) = date('now')"),
        "week" => Ok("date(s.started_at) >= date('now', '-6 days')"),
        "month" => Ok("date(s.started_at) >= date('now', '-29 days')"),
        "all" => Ok("1 = 1"),
        other => Err(format!("invalid period: {other}")),
    }
}

#[tauri::command]
pub fn get_dashboard_stats(db: State<DbState>, period: String) -> Result<DashboardStats, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let filter = period_filter(&period)?;

    let total_playtime_seconds: i64 = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(s.duration_seconds), 0) FROM sessions s
                 WHERE s.duration_seconds IS NOT NULL AND {filter}"
            ),
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let games_completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM games WHERE status = 'completed'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let games_in_backlog: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM games WHERE status = 'backlog'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let games_playing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM games WHERE status = 'playing'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let games_played = {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT g.id, g.name, g.cover_url, SUM(s.duration_seconds) AS total
                 FROM sessions s JOIN games g ON g.id = s.game_id
                 WHERE s.duration_seconds IS NOT NULL AND {filter}
                 GROUP BY g.id ORDER BY total DESC"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GamePlaytime {
                    game_id: row.get(0)?,
                    name: row.get(1)?,
                    cover_url: row.get(2)?,
                    total_seconds: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    let playtime_last_7_days = {
        let mut stmt = conn
            .prepare(
                "SELECT date(started_at) AS d, SUM(duration_seconds)
                 FROM sessions
                 WHERE duration_seconds IS NOT NULL AND date(started_at) >= date('now', '-6 days')
                 GROUP BY d ORDER BY d ASC",
            )
            .map_err(|e| e.to_string())?;
        let by_day: std::collections::HashMap<String, i64> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        (0..7)
            .rev()
            .map(|offset| {
                let date: String = conn
                    .query_row(
                        "SELECT date('now', ?1)",
                        [format!("-{offset} days")],
                        |row| row.get(0),
                    )
                    .unwrap_or_default();
                let total_seconds = *by_day.get(&date).unwrap_or(&0);
                DailyPlaytime { date, total_seconds }
            })
            .collect()
    };

    Ok(DashboardStats {
        total_playtime_seconds,
        games_completed,
        games_in_backlog,
        games_playing,
        games_played,
        playtime_last_7_days,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentlyPlaying {
    pub game_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub started_at: String,
}

/// Reads the live "currently playing" state straight from the DB (the open session with no
/// `ended_at`) rather than relying only on `session-started`/`session-ended` events, so the
/// Dashboard shows the right thing immediately on load — including when the tracker resumed an
/// already-running game via `tracker::reconcile_dangling_sessions` before the frontend had a
/// chance to attach its event listeners.
#[tauri::command]
pub fn get_currently_playing(db: State<DbState>) -> Result<Option<CurrentlyPlaying>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT g.id, g.name, g.cover_url, s.started_at
         FROM sessions s JOIN games g ON g.id = s.game_id
         WHERE s.ended_at IS NULL
         ORDER BY s.started_at DESC LIMIT 1",
        [],
        |row| {
            Ok(CurrentlyPlaying {
                game_id: row.get(0)?,
                name: row.get(1)?,
                cover_url: row.get(2)?,
                started_at: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}
