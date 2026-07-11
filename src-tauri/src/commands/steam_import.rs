// Steam library import (Stage 10, task 17): fetch the full owned library for review, then
// import the picked games as backlog entries, RAWG-enriched in concurrent chunks.

use crate::db::DbState;
use crate::rawg::{self, RawgGameResult};
use crate::steam;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tauri::State;

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
    crate::db::settings::set(&conn, "steam_profile", profile.trim()).map_err(|e| e.to_string())?;

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
    crate::db::settings::get(&conn, "steam_profile").map_err(|e| e.to_string())
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
