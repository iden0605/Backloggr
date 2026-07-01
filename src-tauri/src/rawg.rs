// RAWG API integration — Stage 2 (game search & metadata).

use serde::{Deserialize, Serialize};

const RAWG_API_KEY: &str = "a7786494c20247e79a871ac14e016552";
const RAWG_BASE_URL: &str = "https://api.rawg.io/api";

#[derive(Deserialize)]
struct RawgSearchResponse {
    results: Vec<RawgResult>,
}

#[derive(Deserialize)]
struct RawgResult {
    id: i64,
    name: String,
    background_image: Option<String>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawgGameResult {
    pub rawg_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub genre: Option<String>,
    pub platform: Option<String>,
}

pub async fn search_games(query: &str) -> Result<Vec<RawgGameResult>, String> {
    let client = reqwest::Client::new();
    let url = format!("{RAWG_BASE_URL}/games");

    let response = client
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
        .map(|r| {
            let genre = r
                .genres
                .into_iter()
                .map(|g| g.name)
                .collect::<Vec<_>>()
                .join(", ");
            RawgGameResult {
                rawg_id: r.id,
                name: r.name,
                cover_url: r.background_image,
                genre: if genre.is_empty() { None } else { Some(genre) },
                platform: r.platforms.map(|platforms| {
                    platforms
                        .into_iter()
                        .map(|p| p.platform.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                }),
            }
        })
        .collect();

    Ok(games)
}
