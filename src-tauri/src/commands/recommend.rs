// AI recommendations: the Shelby chat flow (`chat_recommend`) and the activity-seeded
// For You pipeline (`get_dashboard_recommendations` + "Load more"), both proxied through
// the Cloudflare Worker and verified/resolved against RAWG here. The governing pattern:
// the model PROPOSES (candidates, filters), code DECIDES (result-vs-question flow in the
// worker, fact verification against real RAWG data here).

use crate::db::DbState;
use crate::rawg::{self, RawgGameResult};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tauri::State;

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

    let status = response.status();
    if !status.is_success() {
        // The worker wraps its own failures (Groq unreachable, bad request) in a friendly
        // clarify-shaped JSON body — surface that message instead of a bare status code.
        let friendly = response
            .json::<WorkerReply>()
            .await
            .ok()
            .and_then(|reply| match reply {
                WorkerReply::Clarify { question, .. } => Some(question),
                _ => None,
            });
        return Err(friendly
            .unwrap_or_else(|| format!("Recommendation service returned status {status}")));
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

    // Best-effort log to `recommendations` — a failure here shouldn't fail the user-facing
    // reply. Capped at the newest 200 rows: nothing reads this table yet, and uncapped it
    // grows a row per chat turn forever.
    if let Ok(conn) = db.0.lock() {
        let response_json = serde_json::to_string(&result).unwrap_or_default();
        let _ = conn.execute(
            "INSERT INTO recommendations (prompt, response) VALUES (?1, ?2)",
            (&message, &response_json),
        );
        let _ = conn.execute(
            "DELETE FROM recommendations
             WHERE id NOT IN (SELECT id FROM recommendations ORDER BY id DESC LIMIT 200)",
            [],
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
