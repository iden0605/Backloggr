// RAWG API integration — Stage 2 (game search & metadata), extended in Stage 5 with a
// per-game detail lookup (description/Metacritic/developer/publisher) for the expand UI.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const RAWG_API_KEY: &str = "a7786494c20247e79a871ac14e016552";
const RAWG_BASE_URL: &str = "https://api.rawg.io/api";

/// Shared HTTP client — reqwest clients hold a connection pool, so constructing one per request
/// (the previous pattern) threw away keep-alive connections between the sequential lookups in
/// `resolve_titles_with_reasons` and every other call. Also used by `commands.rs` for worker
/// requests.
pub(crate) fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Deserialize)]
struct RawgSearchResponse {
    results: Vec<RawgResult>,
}

#[derive(Deserialize)]
struct RawgResult {
    id: i64,
    slug: String,
    name: String,
    background_image: Option<String>,
    released: Option<String>,
    genres: Vec<RawgGenre>,
    platforms: Option<Vec<RawgPlatformEntry>>,
}

#[derive(Deserialize)]
struct RawgGenre {
    name: String,
}

#[derive(Deserialize)]
struct RawgPlatformEntry {
    platform: RawgPlatform,
}

#[derive(Deserialize)]
struct RawgPlatform {
    name: String,
}

#[derive(Deserialize)]
struct RawgCompany {
    name: String,
}

/// Shape returned by RAWG's `/games/{id}` detail endpoint — a superset of the search result
/// fields, but the search endpoint doesn't reliably return `description_raw`/`metacritic`/
/// `developers`/`publishers`/`website`, so this is fetched separately, on demand, when a card
/// is expanded rather than on every search result.
#[derive(Deserialize)]
struct RawgDetailResult {
    id: i64,
    slug: String,
    name: String,
    background_image: Option<String>,
    description_raw: Option<String>,
    metacritic: Option<i64>,
    genres: Vec<RawgGenre>,
    platforms: Option<Vec<RawgPlatformEntry>>,
    developers: Vec<RawgCompany>,
    publishers: Vec<RawgCompany>,
    website: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RawgGameResult {
    pub rawg_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub genre: Option<String>,
    pub platform: Option<String>,
    pub rawg_url: String,
    /// RAWG's release date (`YYYY-MM-DD`) — the authoritative source for release-window
    /// filtering of AI recommendations (the model's own date knowledge is unreliable).
    /// `default` keeps pre-existing dashboard cache rows deserializing.
    #[serde(default)]
    pub released: Option<String>,
}

/// Release year parsed from RAWG's `YYYY-MM-DD` date, `None` when RAWG has no date.
pub fn release_year(game: &RawgGameResult) -> Option<i32> {
    game.released.as_deref()?.get(..4)?.parse().ok()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawgGameDetail {
    pub rawg_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub genre: Option<String>,
    pub platform: Option<String>,
    pub rawg_url: String,
    pub description: Option<String>,
    pub metacritic_score: Option<i64>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub website_url: Option<String>,
}

fn join_names(names: Vec<String>) -> Option<String> {
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

fn join_platforms(platforms: Option<Vec<RawgPlatformEntry>>) -> Option<String> {
    platforms.map(|platforms| {
        platforms
            .into_iter()
            .map(|p| p.platform.name)
            .collect::<Vec<_>>()
            .join(", ")
    })
}

pub async fn search_games(query: &str) -> Result<Vec<RawgGameResult>, String> {
    let url = format!("{RAWG_BASE_URL}/games");

    let response = http_client()
        .get(&url)
        .query(&[("key", RAWG_API_KEY), ("search", query), ("page_size", "20")])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("RAWG API returned status {}", response.status()));
    }

    let parsed: RawgSearchResponse = response.json().await.map_err(|e| e.to_string())?;

    let games = parsed
        .results
        .into_iter()
        .map(|r| RawgGameResult {
            rawg_id: r.id,
            name: r.name,
            cover_url: r.background_image,
            genre: join_names(r.genres.into_iter().map(|g| g.name).collect()),
            platform: join_platforms(r.platforms),
            rawg_url: format!("https://rawg.io/games/{}", r.slug),
            released: r.released,
        })
        .collect();

    Ok(games)
}

/// Best single result for a title we believe is a real game name (a storefront folder name or a
/// Steam library entry), preferring an exact case-insensitive name match over RAWG's relevance
/// ordering rather than trusting it blindly — used by the tracker's auto-add and the Steam
/// library import.
pub async fn best_match(title: &str) -> Option<RawgGameResult> {
    let results = search_games(title).await.ok()?;
    let exact = results
        .iter()
        .position(|r| r.name.eq_ignore_ascii_case(title));
    match exact {
        Some(i) => results.into_iter().nth(i),
        None => results.into_iter().next(),
    }
}

/// Best single match for a specific title, used to resolve a game name the AI named (as opposed
/// to `search_games`'s broader keyword search used for the user-facing Search page).
async fn search_best_match(title: &str) -> Option<RawgGameResult> {
    let url = format!("{RAWG_BASE_URL}/games");

    let response = http_client()
        .get(&url)
        .query(&[("key", RAWG_API_KEY), ("search", title), ("page_size", "1")])
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let parsed: RawgSearchResponse = response.json().await.ok()?;
    let r = parsed.results.into_iter().next()?;

    Some(RawgGameResult {
        rawg_id: r.id,
        name: r.name,
        cover_url: r.background_image,
        genre: join_names(r.genres.into_iter().map(|g| g.name).collect()),
        platform: join_platforms(r.platforms),
        rawg_url: format!("https://rawg.io/games/{}", r.slug),
        released: r.released,
    })
}

/// Resolves a list of specific game titles (named by the AI, one per suggestion) into RAWG
/// results, deduplicating by `rawg_id` and capping at `max` — this is how chat/dashboard
/// recommendations get real variety instead of RAWG's keyword search surfacing a wall of
/// same-title reskins for a single generic query.
pub async fn resolve_titles(titles: &[String], max: usize) -> Vec<RawgGameResult> {
    let pairs: Vec<(String, Option<String>)> =
        titles.iter().map(|t| (t.clone(), None)).collect();
    resolve_titles_with_reasons(&pairs, max)
        .await
        .into_iter()
        .map(|(game, _)| game)
        .collect()
}

/// Same as `resolve_titles`, but keeps each title's AI-written "why this fits" note paired with
/// its resolved RAWG result, so chat results can show a per-game reason on the card.
///
/// Lookups run concurrently (they used to run one-by-one, adding several seconds of serial
/// round-trips to every result set), but results are collected in the AI's original order so
/// its best suggestions still win the dedup/cap.
pub async fn resolve_titles_with_reasons(
    titles: &[(String, Option<String>)],
    max: usize,
) -> Vec<(RawgGameResult, Option<String>)> {
    let handles: Vec<_> = titles
        .iter()
        .cloned()
        .map(|(title, reason)| {
            tauri::async_runtime::spawn(async move { (search_best_match(&title).await, reason) })
        })
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut games = Vec::new();

    for handle in handles {
        if games.len() >= max {
            break;
        }
        if let Ok((Some(game), reason)) = handle.await {
            if seen.insert(game.rawg_id) {
                games.push((game, reason));
            }
        }
    }

    games
}

pub async fn get_game_details(rawg_id: i64) -> Result<RawgGameDetail, String> {
    let url = format!("{RAWG_BASE_URL}/games/{rawg_id}");

    let response = http_client()
        .get(&url)
        .query(&[("key", RAWG_API_KEY)])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("RAWG API returned status {}", response.status()));
    }

    let r: RawgDetailResult = response.json().await.map_err(|e| e.to_string())?;

    Ok(RawgGameDetail {
        rawg_id: r.id,
        name: r.name,
        cover_url: r.background_image,
        genre: join_names(r.genres.into_iter().map(|g| g.name).collect()),
        platform: join_platforms(r.platforms),
        rawg_url: format!("https://rawg.io/games/{}", r.slug),
        description: r.description_raw.filter(|d| !d.is_empty()),
        metacritic_score: r.metacritic,
        developer: join_names(r.developers.into_iter().map(|d| d.name).collect()),
        publisher: join_names(r.publishers.into_iter().map(|p| p.name).collect()),
        website_url: r.website.filter(|w| !w.is_empty()),
    })
}
