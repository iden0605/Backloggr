use crate::db::DbState;
use crate::rawg::{self, RawgGameDetail, RawgGameResult};
use crate::steam;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
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

#[tauri::command]
pub async fn get_game_details(rawg_id: i64) -> Result<RawgGameDetail, String> {
    rawg::get_game_details(rawg_id).await
}

// Bundled the same way as RAWG_API_KEY in rawg.rs — never exposed in the frontend bundle or a
// Settings field the user has to fill in themselves.
const WORKER_URL: &str = "https://proxy.backloggr.workers.dev";

#[derive(Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// A resolved RAWG game plus the AI's short per-game "why this fits" note (chat results only —
/// dashboard suggestions carry no per-game reason). `Deserialize` + `default` on `reason` keep
/// the dashboard cache backward-compatible: rows written before this field existed still load.
#[derive(Serialize, Deserialize, Clone)]
pub struct RecommendedGame {
    #[serde(flatten)]
    pub game: RawgGameResult,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatRecommendResponse {
    #[serde(rename = "clarify")]
    Clarify {
        question: String,
        options: Option<Vec<String>>,
        multi_select: bool,
        // Real size of the AI's current candidate pool (None when unknown, e.g. fallbacks) —
        // shown in the UI as honest narrowing progress, never a fabricated number.
        candidate_count: Option<u32>,
    },
    #[serde(rename = "results")]
    Results {
        reasoning: String,
        games: Vec<RecommendedGame>,
    },
}

#[derive(Deserialize)]
struct WorkerTitle {
    title: String,
    #[serde(default)]
    reason: Option<String>,
}

/// The worker replies with one of two shapes (see `proxy/src/index.ts`'s `handleChat`); this
/// mirrors that contract so `chat_recommend` can match on it directly instead of hand-parsing.
/// `titles` are specific game names the model knows of (each with a short per-game fit note) —
/// resolved individually via `rawg::resolve_titles_with_reasons` rather than a single keyword
/// search, for real result variety. `options` lets a clarifying question offer quick-pick
/// choices instead of requiring free text every time; `candidate_count` is the actual size of
/// the model's remaining candidate pool while narrowing.
#[derive(Deserialize)]
#[serde(untagged)]
enum WorkerReply {
    Search {
        titles: Vec<WorkerTitle>,
        reasoning: String,
        // Structured filters the player asked for (release window, explicit genre, platform) —
        // the model proposes them, but they're enforced HERE against each resolved game's real
        // RAWG facts, since the model's own knowledge of dates/genres/platforms is unreliable.
        #[serde(default, rename = "minYear")]
        min_year: Option<i32>,
        #[serde(default, rename = "maxYear")]
        max_year: Option<i32>,
        #[serde(default, rename = "requiredGenres")]
        required_genres: Option<Vec<String>>,
        #[serde(default, rename = "requiredPlatforms")]
        required_platforms: Option<Vec<String>>,
    },
    Clarify {
        question: String,
        #[serde(default)]
        options: Option<Vec<String>>,
        #[serde(default, rename = "multiSelect")]
        multi_select: bool,
        #[serde(default, rename = "candidateCount")]
        candidate_count: Option<u32>,
    },
}

const MAX_RECOMMENDATIONS: usize = 8;

/// Games released within this many years count as "recent" and get surfaced before older
/// picks in recommendation results — a soft priority, not a filter: older games still show
/// when there aren't enough recent ones (or when the player explicitly asked for old games).
const RECENT_RELEASE_YEARS: i32 = 7;

/// Stable-partitions recommendations so games released within `RECENT_RELEASE_YEARS` come
/// first, preserving the model's own ranking within each group. Unknown release dates sort
/// with the older group.
fn prioritize_recent(games: &mut [RecommendedGame]) {
    use chrono::Datelike;
    let cutoff = chrono::Utc::now().year() - RECENT_RELEASE_YEARS;
    games.sort_by_key(|g| match rawg::release_year(&g.game) {
        Some(year) if year >= cutoff => 0u8,
        _ => 1,
    });
}

/// Case-insensitive "contains any" check of a comma-joined RAWG field ("Action, RPG, Indie" /
/// "PC, Nintendo Switch") against a required-values list. An empty list means the constraint
/// wasn't asked for (pass); a game missing the field entirely can't be verified (fail) —
/// consistent with the release-window policy in `chat_recommend`.
fn field_matches_any(field: &Option<String>, wanted: &[String]) -> bool {
    if wanted.is_empty() {
        return true;
    }
    match field {
        Some(value) => {
            let value = value.to_lowercase();
            wanted.iter().any(|w| value.contains(&w.to_lowercase()))
        }
        None => false,
    }
}

/// Fetches every game name in the library (any status, including dropped — re-recommending a
/// game the player abandoned reads just as fake as one they own) for the worker's exclusion
/// list. Collected into an owned Vec so the mutex guard drops before any `.await`.
fn owned_game_names(db: &DbState) -> Result<Vec<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT name FROM games")
        .map_err(|e| e.to_string())?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(names)
}

/// Forwards the chat turn to the bundled Worker URL, and — whenever the worker's code-side
/// narrowing rule decides the candidate pool is focused enough — resolves the named titles
/// against RAWG itself, so the RAWG API key never has to leave this binary. `questions_asked`
/// counts clarifying rounds for the current ask (the worker caps the loop); the player's own
/// library is sent as an exclusion list so nothing they already have comes back.
#[tauri::command]
pub async fn chat_recommend(
    db: State<'_, DbState>,
    message: String,
    history: Vec<ChatMessage>,
    questions_asked: u32,
) -> Result<ChatRecommendResponse, String> {
    let excluded = owned_game_names(&db)?;

    let response = rawg::http_client()
        .post(format!("{WORKER_URL}/chat"))
        .json(&serde_json::json!({
            "message": message,
            "history": history,
            "questionsAsked": questions_asked,
            "excluded": excluded,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!(
            "Recommendation service returned status {}",
            response.status()
        ));
    }

    let reply: WorkerReply = response.json().await.map_err(|e| e.to_string())?;

    let result = match reply {
        WorkerReply::Clarify {
            question,
            options,
            multi_select,
            candidate_count,
        } => ChatRecommendResponse::Clarify {
            question,
            options,
            multi_select,
            candidate_count,
        },
        WorkerReply::Search {
            titles,
            reasoning,
            min_year,
            max_year,
            required_genres,
            required_platforms,
        } => {
            let pairs: Vec<(String, Option<String>)> =
                titles.into_iter().map(|t| (t.title, t.reason)).collect();
            let genres = required_genres.unwrap_or_default();
            let platforms = required_platforms.unwrap_or_default();
            // Resolve every candidate (the worker sends spares beyond the 8 that will show):
            // filter failures get dropped and recent releases float to the front below, so
            // capping before either step would waste candidates.
            let mut games: Vec<RecommendedGame> = rawg::resolve_titles_with_reasons(&pairs, pairs.len())
                .await
                .into_iter()
                .filter(|(game, _)| {
                    // Verify each asked-for constraint against the game's real RAWG facts.
                    // A game RAWG lacks the fact for can't be verified — dropping it beats
                    // showing a 2014 game for a "2024+" ask (same policy for genre/platform).
                    let year_ok = if min_year.is_some() || max_year.is_some() {
                        match rawg::release_year(game) {
                            Some(year) => {
                                min_year.is_none_or(|min| year >= min)
                                    && max_year.is_none_or(|max| year <= max)
                            }
                            None => false,
                        }
                    } else {
                        true
                    };
                    year_ok
                        && field_matches_any(&game.genre, &genres)
                        && field_matches_any(&game.platform, &platforms)
                })
                .map(|(game, reason)| RecommendedGame { game, reason })
                .collect();
            prioritize_recent(&mut games);
            // The Groq model barely knows 2024+ releases, so a release-window ask can come
            // back short after verification. Top up from RAWG's own date-range discovery
            // (popular releases in the window, genre-filtered) — real new games the model
            // structurally can't name. Model picks keep the front; fills trail.
            if games.len() < MAX_RECOMMENDATIONS {
                if let Some(min) = min_year {
                    let mut seen: std::collections::HashSet<i64> =
                        games.iter().map(|g| g.game.rawg_id).collect();
                    let excluded_lower: std::collections::HashSet<String> =
                        excluded.iter().map(|n| n.to_lowercase()).collect();
                    for game in rawg::discover_recent(min, max_year, &genres).await {
                        if games.len() >= MAX_RECOMMENDATIONS {
                            break;
                        }
                        if !seen.insert(game.rawg_id)
                            || excluded_lower.contains(&game.name.to_lowercase())
                            || !field_matches_any(&game.platform, &platforms)
                        {
                            continue;
                        }
                        games.push(RecommendedGame {
                            game,
                            reason: Some("Popular new release in your timeframe".to_string()),
                        });
                    }
                }
            }
            games.truncate(MAX_RECOMMENDATIONS);
            ChatRecommendResponse::Results { reasoning, games }
        }
    };

    // Best-effort log to `recommendations` — a failure here shouldn't fail the user-facing reply.
    if let Ok(conn) = db.0.lock() {
        let response_json = serde_json::to_string(&result).unwrap_or_default();
        let _ = conn.execute(
            "INSERT INTO recommendations (prompt, response) VALUES (?1, ?2)",
            (&message, &response_json),
        );
    }

    Ok(result)
}

/// One taste-profile entry sent to the worker's /suggest prompt. `weight` is this game's
/// log-dampened share of total playtime as a percent — log so a 300-hour Valorant habit
/// pulls recommendations toward FPS without drowning out a 10-hour cozy game entirely.
/// None when the library has no playtime yet (fresh installs fall back to recently-added
/// games, weighted equally by omission).
struct FavoriteSignal {
    name: String,
    genre: Option<String>,
    weight: Option<u32>,
}

struct GenreSignal {
    backlog_count: i64,
    top_genre: Option<String>,
    top_genre_playtime_seconds: i64,
    favorites: Vec<FavoriteSignal>,
}

/// Picks the genre the user has spent the most time playing (via each game's *primary* — first
/// listed — genre) and a playtime-weighted taste profile to seed the dashboard's AI suggestion
/// prompt: the top 15 games by playtime with log-dampened share weights, or (for a fresh
/// backlog with no playtime yet) the 3 most recently added, so the widget isn't empty on day one.
fn compute_genre_signal(conn: &rusqlite::Connection) -> Result<GenreSignal, String> {
    struct Row {
        name: String,
        genre: Option<String>,
        added_at: String,
        total: i64,
    }

    let mut stmt = conn
        .prepare(
            "SELECT g.name, g.genre, g.added_at, COALESCE(SUM(s.duration_seconds), 0) AS total
             FROM games g
             LEFT JOIN sessions s ON s.game_id = g.id AND s.duration_seconds IS NOT NULL
             GROUP BY g.id",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Row {
                name: row.get(0)?,
                genre: row.get(1)?,
                added_at: row.get(2)?,
                total: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let backlog_count = rows.len() as i64;

    let mut genre_playtime: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for row in &rows {
        if let Some(primary) = row.genre.as_deref().and_then(|g| g.split(", ").next()) {
            *genre_playtime.entry(primary.to_string()).or_insert(0) += row.total;
        }
    }
    let (top_genre, top_genre_playtime_seconds) = genre_playtime
        .into_iter()
        // Tie-break on genre name (Reverse → alphabetically first wins): plain max over a
        // HashMap breaks ties by iteration order, which is randomized per instance — an
        // all-zero-playtime library (fresh import, nothing played) got a different "top
        // genre" on every call, regenerating the recommendations cache on every visit.
        .max_by_key(|(genre, total)| (*total, std::cmp::Reverse(genre.clone())))
        .map(|(g, t)| (Some(g), t))
        .unwrap_or((None, 0));

    let mut played: Vec<&Row> = rows.iter().filter(|r| r.total > 0).collect();
    played.sort_by(|a, b| b.total.cmp(&a.total));
    let favorites = if !played.is_empty() {
        // Weight = ln(1 + hours), normalized to percent shares. Log keeps the balance the
        // taste profile needs: 300h/100h/10h of play becomes roughly 45/36/19 rather than
        // the raw 73/24/3 — the dominant game leads, nothing gets erased.
        let top: Vec<&Row> = played.into_iter().take(15).collect();
        let logs: Vec<f64> = top
            .iter()
            .map(|r| (1.0 + r.total as f64 / 3600.0).ln())
            .collect();
        let log_sum: f64 = logs.iter().sum();
        top.iter()
            .zip(&logs)
            .map(|(r, log_weight)| FavoriteSignal {
                name: r.name.clone(),
                genre: r.genre.clone(),
                weight: Some((log_weight / log_sum * 100.0).round().max(1.0) as u32),
            })
            .collect()
    } else {
        let mut by_added: Vec<&Row> = rows.iter().collect();
        by_added.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        by_added
            .into_iter()
            .take(3)
            .map(|r| FavoriteSignal {
                name: r.name.clone(),
                genre: r.genre.clone(),
                weight: None,
            })
            .collect()
    };

    Ok(GenreSignal {
        backlog_count,
        top_genre,
        top_genre_playtime_seconds,
        favorites,
    })
}

const DASHBOARD_RECS_REFRESH_SECONDS: i64 = 5 * 3600;
const DASHBOARD_RECS_PLAYTIME_SHIFT_SECONDS: i64 = 2 * 3600;

/// Games picked from the player's backlog/playtime history rather than a chat prompt — shown as
/// a Dashboard widget. Only re-queries the AI when something meaningful changed since the last
/// generation (backlog size, top-played genre, or a big jump in that genre's playtime) or 5
/// hours have passed, so opening the Dashboard repeatedly doesn't burn an AI call every time.
#[tauri::command]
pub async fn get_dashboard_recommendations(
    db: State<'_, DbState>,
) -> Result<ChatRecommendResponse, String> {
    let signal = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        compute_genre_signal(&conn)?
    };

    if signal.backlog_count == 0 {
        return Ok(ChatRecommendResponse::Results {
            reasoning: "Add some games to your backlog to get personalized recommendations."
                .to_string(),
            games: vec![],
        });
    }

    struct Cached {
        generated_at: i64,
        backlog_count: i64,
        top_genre: Option<String>,
        top_genre_playtime_seconds: i64,
        reasoning: String,
        games_json: String,
    }

    let cached = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT generated_at, backlog_count, top_genre, top_genre_playtime_seconds, reasoning, games_json
             FROM dashboard_recommendations_cache WHERE id = 1",
            [],
            |row| {
                Ok(Cached {
                    generated_at: row.get(0)?,
                    backlog_count: row.get(1)?,
                    top_genre: row.get(2)?,
                    top_genre_playtime_seconds: row.get(3)?,
                    reasoning: row.get(4)?,
                    games_json: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
    };

    let now = chrono::Utc::now().timestamp();
    let should_regenerate = match &cached {
        None => true,
        Some(c) => {
            now - c.generated_at > DASHBOARD_RECS_REFRESH_SECONDS
                || c.backlog_count != signal.backlog_count
                || c.top_genre != signal.top_genre
                || (signal.top_genre_playtime_seconds - c.top_genre_playtime_seconds)
                    >= DASHBOARD_RECS_PLAYTIME_SHIFT_SECONDS
        }
    };

    if !should_regenerate {
        if let Some(c) = cached {
            let games: Vec<RecommendedGame> = serde_json::from_str(&c.games_json).unwrap_or_default();
            return Ok(ChatRecommendResponse::Results {
                reasoning: c.reasoning,
                games,
            });
        }
    }

    let excluded = owned_game_names(&db)?;
    let (reasoning, games) =
        fetch_suggestions(&signal.favorites, &excluded, signal.top_genre.as_deref()).await?;

    if let Ok(conn) = db.0.lock() {
        let games_json = serde_json::to_string(&games).unwrap_or_default();
        let _ = conn.execute(
            "INSERT INTO dashboard_recommendations_cache
                (id, generated_at, backlog_count, top_genre, top_genre_playtime_seconds, reasoning, games_json)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                generated_at = excluded.generated_at,
                backlog_count = excluded.backlog_count,
                top_genre = excluded.top_genre,
                top_genre_playtime_seconds = excluded.top_genre_playtime_seconds,
                reasoning = excluded.reasoning,
                games_json = excluded.games_json",
            (
                now,
                signal.backlog_count,
                &signal.top_genre,
                signal.top_genre_playtime_seconds,
                &reasoning,
                &games_json,
            ),
        );
    }

    Ok(ChatRecommendResponse::Results { reasoning, games })
}

/// POSTs a favorites list to the worker's `/suggest` endpoint, resolves every returned title
/// against RAWG, and applies the shared recency sort + cap — the fetch half of
/// `get_dashboard_recommendations`, shared with the uncached "load more" path.
async fn fetch_suggestions(
    favorites: &[FavoriteSignal],
    excluded: &[String],
    top_genre: Option<&str>,
) -> Result<(String, Vec<RecommendedGame>), String> {
    let favorites_json: Vec<serde_json::Value> = favorites
        .iter()
        .map(|f| serde_json::json!({ "name": f.name, "genre": f.genre, "weight": f.weight }))
        .collect();

    let response = rawg::http_client()
        .post(format!("{WORKER_URL}/suggest"))
        .json(&serde_json::json!({ "games": favorites_json, "excluded": excluded }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!(
            "Recommendation service returned status {}",
            response.status()
        ));
    }

    #[derive(Deserialize)]
    struct SuggestReply {
        titles: Vec<String>,
        reasoning: String,
    }

    let reply: SuggestReply = response.json().await.map_err(|e| e.to_string())?;
    // Resolve everything the model named (it sends more than the 8 shown), float recent
    // releases to the front, then cap — same recency policy as chat results.
    let title_count = reply.titles.len();
    let mut games: Vec<RecommendedGame> = rawg::resolve_titles(&reply.titles, title_count)
        .await
        .into_iter()
        .map(|game| RecommendedGame { game, reason: None })
        .collect();

    // Blend in up to 2 genuinely-new releases (last ~2 years, RAWG date-range discovery)
    // from the player's top-played genre. The Groq model can't name games past its
    // knowledge cutoff no matter how hard the prompt leans recent — this is where truly
    // new titles enter the For You set.
    if let Some(genre) = top_genre {
        use chrono::Datelike;
        let min_year = chrono::Utc::now().year() - 1;
        let seen: std::collections::HashSet<i64> =
            games.iter().map(|g| g.game.rawg_id).collect();
        let excluded_lower: std::collections::HashSet<String> =
            excluded.iter().map(|n| n.to_lowercase()).collect();
        let fresh: Vec<RecommendedGame> =
            rawg::discover_recent(min_year, None, &[genre.to_string()])
                .await
                .into_iter()
                .filter(|g| {
                    !seen.contains(&g.rawg_id) && !excluded_lower.contains(&g.name.to_lowercase())
                })
                .take(2)
                .map(|game| RecommendedGame { game, reason: None })
                .collect();
        games.extend(fresh);
    }

    prioritize_recent(&mut games);
    games.truncate(MAX_RECOMMENDATIONS);
    Ok((reply.reasoning, games))
}

/// "Load more" for the For You grid (task 21): the same activity-seeded `/suggest` ask, but
/// with everything already on screen excluded alongside the library so a fresh batch comes
/// back. Deliberately uncached — it only runs on an explicit click, and the cached base set
/// in `dashboard_recommendations_cache` stays untouched.
#[tauri::command]
pub async fn get_more_dashboard_recommendations(
    db: State<'_, DbState>,
    shown: Vec<String>,
) -> Result<ChatRecommendResponse, String> {
    let signal = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        compute_genre_signal(&conn)?
    };

    if signal.backlog_count == 0 {
        return Ok(ChatRecommendResponse::Results {
            reasoning: String::new(),
            games: vec![],
        });
    }

    let mut excluded = owned_game_names(&db)?;
    excluded.extend(shown);
    let (reasoning, games) =
        fetch_suggestions(&signal.favorites, &excluded, signal.top_genre.as_deref()).await?;
    Ok(ChatRecommendResponse::Results { reasoning, games })
}

// ---- Ask AI chat history (chat_conversations) ----
// Turns are an opaque JSON blob of the frontend's ChatTurn shape — Rust stores and
// returns it verbatim, the frontend owns (de)serialization.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversation {
    pub id: i64,
    pub title: String,
    pub turns_json: String,
    pub questions_asked: i64,
}

#[tauri::command]
pub fn list_chats(db: State<DbState>) -> Result<Vec<ChatSummary>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, updated_at FROM chat_conversations
             ORDER BY updated_at DESC, id DESC",
        )
        .map_err(|e| e.to_string())?;
    let chats = stmt
        .query_map([], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(chats)
}

#[tauri::command]
pub fn get_chat(db: State<DbState>, id: i64) -> Result<ChatConversation, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, title, turns_json, questions_asked FROM chat_conversations WHERE id = ?1",
        [id],
        |row| {
            Ok(ChatConversation {
                id: row.get(0)?,
                title: row.get(1)?,
                turns_json: row.get(2)?,
                questions_asked: row.get(3)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// Upsert a conversation after each completed exchange; returns the row id so the
/// frontend can adopt it after the first save of a new chat. An id that no longer
/// exists (deleted from the history panel mid-conversation) falls through to insert.
#[tauri::command]
pub fn save_chat(
    db: State<DbState>,
    id: Option<i64>,
    title: String,
    turns_json: String,
    questions_asked: i64,
) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    if let Some(id) = id {
        let updated = conn
            .execute(
                "UPDATE chat_conversations
                 SET turns_json = ?1, questions_asked = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?3",
                rusqlite::params![turns_json, questions_asked, id],
            )
            .map_err(|e| e.to_string())?;
        if updated > 0 {
            return Ok(id);
        }
    }
    conn.execute(
        "INSERT INTO chat_conversations (title, turns_json, questions_asked) VALUES (?1, ?2, ?3)",
        rusqlite::params![title, turns_json, questions_asked],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_chat(db: State<DbState>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_conversations WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
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

// ---- Steam library import (Stage 10, task 17) ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamLibraryGame {
    pub app_id: i64,
    pub name: String,
    pub playtime_minutes: i64,
    pub in_backlog: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamLibrary {
    pub steam_id: String,
    pub games: Vec<SteamLibraryGame>,
}

/// Resolves the pasted profile (URL / vanity / steamID64) and returns the full owned library —
/// step one of the two-step import: the frontend shows this list for review/deselection before
/// anything touches the backlog. Games already present (matched by `steam_appid` from a prior
/// import, or by name) are flagged so the UI can gray them out. The raw input is remembered in
/// `settings` so the field prefills next time.
#[tauri::command]
pub async fn fetch_steam_library(
    db: State<'_, DbState>,
    profile: String,
) -> Result<SteamLibrary, String> {
    let steam_id = steam::resolve_steam_id(&profile).await?;
    let mut owned = steam::get_owned_games(&steam_id).await?;
    // Most-played first reads as "my library", and the never-launched long tail — the games this
    // import exists to capture — groups alphabetically at the bottom where it's easy to skim.
    owned.sort_by(|a, b| {
        b.playtime_minutes
            .cmp(&a.playtime_minutes)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('steam_profile', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [profile.trim()],
    )
    .map_err(|e| e.to_string())?;

    let existing_appids: std::collections::HashSet<i64> = conn
        .prepare("SELECT steam_appid FROM games WHERE steam_appid IS NOT NULL")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    let existing_names: std::collections::HashSet<String> = conn
        .prepare("SELECT lower(name) FROM games")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let games = owned
        .into_iter()
        .map(|g| {
            let in_backlog = existing_appids.contains(&g.app_id)
                || existing_names.contains(&g.name.to_lowercase());
            SteamLibraryGame {
                app_id: g.app_id,
                name: g.name,
                playtime_minutes: g.playtime_minutes,
                in_backlog,
            }
        })
        .collect();

    Ok(SteamLibrary { steam_id, games })
}

/// The last profile input a successful fetch used — prefills the Settings field.
#[tauri::command]
pub fn get_steam_profile(db: State<DbState>) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'steam_profile'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SteamImportPick {
    pub app_id: i64,
    pub name: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SteamImportProgress {
    pub done: usize,
    pub total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamImportSummary {
    pub imported: usize,
    pub linked: usize,
    pub skipped: usize,
}

/// Step two: imports the selected games as `backlog` entries, best-effort enriched via RAWG
/// (cover/genre/platform — a miss just means a bare entry, same as the tracker's auto-add).
/// Lookups run in small concurrent chunks so a several-hundred-game library doesn't take one
/// serial round-trip each, with a `steam-import-progress` event per chunk for the UI's bar.
/// The DB mutex is only taken between chunks, never across an `.await`.
#[tauri::command]
pub async fn import_steam_games(
    app: tauri::AppHandle,
    db: State<'_, DbState>,
    games: Vec<SteamImportPick>,
) -> Result<SteamImportSummary, String> {
    use tauri::Emitter;

    const LOOKUP_CHUNK: usize = 8;
    let total = games.len();
    let mut done = 0usize;
    let mut summary = SteamImportSummary { imported: 0, linked: 0, skipped: 0 };

    for chunk in games.chunks(LOOKUP_CHUNK) {
        let handles: Vec<_> = chunk
            .iter()
            .cloned()
            .map(|pick| {
                tauri::async_runtime::spawn(async move {
                    let rawg_match = rawg::best_match(&pick.name).await;
                    (pick, rawg_match)
                })
            })
            .collect();
        let mut resolved = Vec::new();
        for handle in handles {
            if let Ok(pair) = handle.await {
                resolved.push(pair);
            }
        }

        {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            for (pick, rawg_match) in resolved {
                import_one_steam_game(&conn, &pick, rawg_match.as_ref(), &mut summary)
                    .map_err(|e| e.to_string())?;
            }
        }

        done += chunk.len();
        let _ = app.emit("steam-import-progress", SteamImportProgress { done, total });
    }

    // Single refresh signal for the Backlog view — per-game events (game-auto-added style)
    // would fire hundreds of notices for a big library.
    let _ = app.emit("steam-import-done", ());
    Ok(summary)
}

/// One game's dedupe-or-insert: already imported (by appid) → skip; RAWG match already in the
/// backlog (added by hand or by the tracker) → just link its `steam_appid`; otherwise insert a
/// new `backlog` row. A RAWG miss falls back to a name-matched link or a bare named entry.
fn import_one_steam_game(
    conn: &rusqlite::Connection,
    pick: &SteamImportPick,
    rawg_match: Option<&RawgGameResult>,
    summary: &mut SteamImportSummary,
) -> Result<(), rusqlite::Error> {
    let already: Option<i64> = conn
        .query_row(
            "SELECT id FROM games WHERE steam_appid = ?1",
            [pick.app_id],
            |row| row.get(0),
        )
        .optional()?;
    if already.is_some() {
        summary.skipped += 1;
        return Ok(());
    }

    let existing: Option<i64> = match rawg_match {
        Some(m) => conn
            .query_row(
                "SELECT id FROM games WHERE rawg_id = ?1 OR lower(name) = lower(?2)",
                rusqlite::params![m.rawg_id, pick.name],
                |row| row.get(0),
            )
            .optional()?,
        None => conn
            .query_row(
                "SELECT id FROM games WHERE lower(name) = lower(?1)",
                [&pick.name],
                |row| row.get(0),
            )
            .optional()?,
    };

    if let Some(id) = existing {
        conn.execute(
            "UPDATE games SET steam_appid = ?1 WHERE id = ?2",
            rusqlite::params![pick.app_id, id],
        )?;
        summary.linked += 1;
        return Ok(());
    }

    match rawg_match {
        Some(m) => conn.execute(
            "INSERT INTO games (rawg_id, name, cover_url, genre, platform, status, steam_appid)
             VALUES (?1, ?2, ?3, ?4, ?5, 'backlog', ?6)",
            rusqlite::params![m.rawg_id, m.name, m.cover_url, m.genre, m.platform, pick.app_id],
        )?,
        None => conn.execute(
            "INSERT INTO games (name, status, steam_appid) VALUES (?1, 'backlog', ?2)",
            rusqlite::params![pick.name, pick.app_id],
        )?,
    };
    summary.imported += 1;
    Ok(())
}
