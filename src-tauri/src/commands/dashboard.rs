// Dashboard stats + the live "currently playing" read.

use crate::db::DbState;
use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

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
    /// Owned games (everything except the wishlist) — the library size.
    pub games_in_library: i64,
    /// Distinct games with any tracked session — "how much of the library gets played".
    pub games_played_count: i64,
    pub games_played: Vec<GamePlaytime>,
    pub playtime_last_7_days: Vec<DailyPlaytime>,
    /// Rolling previous-7-days total (days -13..-7, localtime) — the "vs last week" delta's
    /// baseline. The current week's total is the sum of `playtime_last_7_days` client-side.
    pub prev_week_playtime_seconds: i64,
    /// Per-game playtime over the last 7 days, most-played first — feeds the fixed
    /// "Most played this week" card independently of the shelf's period selector.
    pub week_games: Vec<GamePlaytime>,
}

/// Maps a period selector to a SQL date-filter clause on `s.started_at`. Buckets are calendar
/// days in the user's LOCAL timezone (`'localtime'` uses the OS tz): `day` = today, `week`/
/// `month` = today plus the previous 6/29 days. Timestamps are stored as UTC — without the
/// `'localtime'` conversion, an evening session east of UTC lands on the wrong local day (e.g. a
/// 9am AEST session is 11pm UTC the previous day, which used to count toward yesterday).
fn period_filter(period: &str) -> Result<&'static str, String> {
    match period {
        "day" => Ok("date(s.started_at, 'localtime') = date('now', 'localtime')"),
        "week" => Ok("date(s.started_at, 'localtime') >= date('now', 'localtime', '-6 days')"),
        "month" => Ok("date(s.started_at, 'localtime') >= date('now', 'localtime', '-29 days')"),
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

    let games_in_library: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM games WHERE status != 'wishlist'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let games_played_count: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT game_id) FROM sessions",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let per_game_playtime = |filter: &str| -> Result<Vec<GamePlaytime>, String> {
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
        Ok(rows)
    };

    let games_played = per_game_playtime(filter)?;
    // Fixed rolling week, independent of the shelf's period selector.
    let week_games = per_game_playtime(period_filter("week")?)?;

    let prev_week_playtime_seconds: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(s.duration_seconds), 0) FROM sessions s
             WHERE s.duration_seconds IS NOT NULL
               AND date(s.started_at, 'localtime') >= date('now', 'localtime', '-13 days')
               AND date(s.started_at, 'localtime') < date('now', 'localtime', '-6 days')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let playtime_last_7_days = {
        // Same 'localtime' bucketing as period_filter — chart days must be the user's days.
        let mut stmt = conn
            .prepare(
                "SELECT date(started_at, 'localtime') AS d, SUM(duration_seconds)
                 FROM sessions
                 WHERE duration_seconds IS NOT NULL
                   AND date(started_at, 'localtime') >= date('now', 'localtime', '-6 days')
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
                        "SELECT date('now', 'localtime', ?1)",
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
        games_in_library,
        games_played_count,
        games_played,
        playtime_last_7_days,
        prev_week_playtime_seconds,
        week_games,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentlyPlaying {
    pub game_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub started_at: String,
    /// How many OTHER games also have an open session right now. Multiple games running at once
    /// is normal (launcher-spawned games, two games mid-swap) — the hero/nav shows the most
    /// recently launched one plus a "+N more" so the display isn't silently lying.
    pub also_playing: i64,
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
        "SELECT g.id, g.name, g.cover_url, s.started_at,
                (SELECT COUNT(*) - 1 FROM sessions WHERE ended_at IS NULL) AS also_playing
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
                also_playing: row.get::<_, i64>(4)?.max(0),
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}
