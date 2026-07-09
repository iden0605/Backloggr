// Steam Web API integration — Stage 10 (task 17): imports the user's full owned-games library
// (including games never launched on this machine), replacing guesswork with the authoritative
// list. The path-heuristic auto-add in tracker.rs stays for launch-time detection; this covers
// everything it structurally can't see.

use crate::rawg::http_client;
use serde::{Deserialize, Serialize};

// Baked in at compile time from the STEAM_API_KEY env var — unlike the RAWG key it is NOT
// committed to the repo: CI injects it from the GitHub repo secret of the same name
// (release.yml), and local dev reads it from the gitignored src-tauri/.cargo/config.toml
// [env] section. Missing at compile time = the import commands return a clear error.
const STEAM_API_KEY: &str = match option_env!("STEAM_API_KEY") {
    Some(key) => key,
    None => "",
};
const STEAM_API_BASE: &str = "https://api.steampowered.com";

fn api_key() -> Result<&'static str, String> {
    if STEAM_API_KEY.is_empty() {
        Err("Steam import isn't configured in this build (missing Steam Web API key).".into())
    } else {
        Ok(STEAM_API_KEY)
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SteamOwnedGame {
    pub app_id: i64,
    pub name: String,
    pub playtime_minutes: i64,
}

/// Accepts the identity forms a user actually has on hand — a full profile URL
/// (`…/profiles/<id64>` or `…/id/<vanity>`), a bare 17-digit steamID64, or a bare vanity
/// name — and resolves it to a steamID64, calling ResolveVanityURL only when needed.
pub async fn resolve_steam_id(input: &str) -> Result<String, String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Enter your Steam profile URL, vanity name, or steamID64.".into());
    }

    if let Some(rest) = trimmed.split("/profiles/").nth(1) {
        let id = rest.split('/').next().unwrap_or_default();
        if is_steam_id64(id) {
            return Ok(id.to_string());
        }
        return Err("That profile URL doesn't contain a valid steamID64.".into());
    }
    if let Some(rest) = trimmed.split("/id/").nth(1) {
        let vanity = rest.split('/').next().unwrap_or_default();
        return resolve_vanity(vanity).await;
    }
    if is_steam_id64(trimmed) {
        return Ok(trimmed.to_string());
    }
    resolve_vanity(trimmed).await
}

fn is_steam_id64(s: &str) -> bool {
    s.len() == 17 && s.chars().all(|c| c.is_ascii_digit())
}

#[derive(Deserialize)]
struct VanityEnvelope {
    response: VanityResponse,
}

#[derive(Deserialize)]
struct VanityResponse {
    success: i64,
    steamid: Option<String>,
}

async fn resolve_vanity(vanity: &str) -> Result<String, String> {
    if vanity.is_empty() {
        return Err("Enter your Steam profile URL, vanity name, or steamID64.".into());
    }
    let key = api_key()?;
    let url = format!("{STEAM_API_BASE}/ISteamUser/ResolveVanityURL/v1/");

    let response = http_client()
        .get(&url)
        .query(&[("key", key), ("vanityurl", vanity)])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Steam API returned status {}", response.status()));
    }

    let parsed: VanityEnvelope = response.json().await.map_err(|e| e.to_string())?;
    match parsed.response.steamid {
        Some(id) if parsed.response.success == 1 => Ok(id),
        _ => Err(format!("No Steam profile found for \"{vanity}\".")),
    }
}

#[derive(Deserialize)]
struct OwnedGamesEnvelope {
    response: OwnedGamesResponse,
}

// A private profile (or one with "Game details" hidden) comes back as a bare `{"response":{}}`
// with 200 OK — absent fields, not an error status — so everything here is optional and the
// caller-facing error explains the privacy setting.
#[derive(Deserialize)]
struct OwnedGamesResponse {
    games: Option<Vec<OwnedGameEntry>>,
}

#[derive(Deserialize)]
struct OwnedGameEntry {
    appid: i64,
    name: Option<String>,
    playtime_forever: Option<i64>,
}

/// The full owned library for a steamID64 — every purchased/claimed game, launched or not.
pub async fn get_owned_games(steam_id: &str) -> Result<Vec<SteamOwnedGame>, String> {
    let key = api_key()?;
    let url = format!("{STEAM_API_BASE}/IPlayerService/GetOwnedGames/v1/");

    let response = http_client()
        .get(&url)
        .query(&[
            ("key", key),
            ("steamid", steam_id),
            ("include_appinfo", "1"),
            ("include_played_free_games", "1"),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Steam API returned status {}", response.status()));
    }

    let parsed: OwnedGamesEnvelope = response.json().await.map_err(|e| e.to_string())?;
    let Some(games) = parsed.response.games else {
        return Err(
            "Steam returned no games — the profile's \"Game details\" privacy setting must be \
             Public for the library to be readable."
                .into(),
        );
    };

    Ok(games
        .into_iter()
        .filter_map(|g| {
            let name = g.name?;
            Some(SteamOwnedGame {
                app_id: g.appid,
                name,
                playtime_minutes: g.playtime_forever.unwrap_or(0),
            })
        })
        .collect())
}
