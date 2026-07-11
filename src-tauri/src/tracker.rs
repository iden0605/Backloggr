use crate::db::{games, sessions, DbState};
use crate::rawg;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, System, UpdateKind};
use tauri::{AppHandle, Emitter, Manager};

const POLL_INTERVAL_SECS: u64 = 5;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SessionStarted {
    game_id: i64,
    session_id: i64,
    started_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SessionEnded {
    game_id: i64,
    session_id: i64,
    duration_seconds: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameAutoAdded {
    game_id: i64,
    name: String,
}

/// Strip a trailing `.exe` (case-insensitive) so Windows' suffixed process names
/// compare equal to the extension-less names sysinfo reports on Mac. Also used by
/// `clipper::find_window_title_for_exe` to match a window's owning process against the stored
/// exe_name regardless of suffix.
pub(crate) fn normalize_exe_name(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn running_exe_names(sys: &mut System) -> HashSet<String> {
    // Only names + exe paths are ever read — skip the per-process CPU/memory/disk/user stats a
    // bare refresh_processes() collects, which measurably cut this 5s poll's own CPU cost.
    sys.refresh_processes_specifics(
        ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet),
    );
    sys.processes()
        .values()
        .map(|p| normalize_exe_name(&p.name().to_string()))
        .collect()
}

/// Directory names that mark "the next path component is a game's install folder" across the
/// major PC storefronts/launchers. Matched case-insensitively against path components, not raw
/// substrings, so we don't misfire on something like a user folder named "Origin Games Notes".
const LIBRARY_MARKERS: &[&str] = &[
    "common", // steamapps/common/<Game>
    "epic games",
    "gog games",
    "battle.net",
    "riot games",
];

/// Executable-name fragments (matched against the normalized, `.exe`-less, lowercased name) that
/// mark a process as launcher/storefront plumbing rather than a game, even though it lives under
/// a storefront library folder. Live-testing example: `RiotClientServices.exe` (the always-running
/// Riot client under `Riot Games\Riot Client\`) got auto-added and RAWG-matched to an unrelated
/// game ("GRITO GRIOT"). These never open a session or a library row.
const NON_GAME_EXE_PATTERNS: &[&str] = &[
    // Riot plumbing: the client itself, its UX/render helpers, and the Vanguard anti-cheat.
    "riotclient",
    "leagueclient", // League's launcher — the game itself is "League of Legends.exe"
    "vanguard",
    "vgtray",
    "vgc",
    // Storefront clients/launchers that can sit inside library-marker folders.
    "epicgameslauncher",
    "epicwebhelper",
    "epiconlineservices",
    "galaxyclient",
    "battle.net",
    "agent", // Battle.net's background updater ("Agent.exe")
    "steamwebhelper",
    "gameoverlayui",
    // Generic helper/service processes games and launchers ship alongside the real exe.
    "launcher",
    "crashhandler",
    "crashpad",
    "crashreport",
    "crashsender",
    "webhelper",
    "easyanticheat",
    "battleye",
    "beservice",
    "anticheat",
    "overlay",
    "updater",
    "installer",
    "uninstall",
    "setup",
    "redist",
    "dxsetup",
    "vcredist",
    "helper",
    "service",
];

/// Install-folder names (the path component right after a library marker) that hold launcher
/// infrastructure, not games — e.g. `Riot Games\Riot Client\`, `Epic Games\Launcher\`.
const NON_GAME_INSTALL_FOLDERS: &[&str] = &[
    "riot client",
    "launcher",
    "epic online services",
    "directxredist",
    "_commonredist",
    "tools",
];

/// Whether a normalized exe name looks like launcher/anti-cheat/helper plumbing rather than an
/// actual game — see `NON_GAME_EXE_PATTERNS`.
fn is_non_game_exe(exe_norm: &str) -> bool {
    NON_GAME_EXE_PATTERNS.iter().any(|p| exe_norm.contains(p))
}

/// Best-effort guess at a human-readable game name from its install path, e.g.
/// `.../steamapps/common/Dave the Diver/DaveTheDiver.app` -> `"Dave the Diver"`. Only ever used
/// to seed a RAWG search for a game we don't already know about — never trusted as final data.
fn guess_game_name_from_path(path: &Path) -> Option<String> {
    let components: Vec<String> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .map(|s| s.to_string())
        .collect();

    for (i, component) in components.iter().enumerate() {
        let lower = component.to_lowercase();
        if LIBRARY_MARKERS.contains(&lower.as_str()) {
            if let Some(next) = components.get(i + 1) {
                // The next component must be a game's install DIRECTORY. An exe sitting directly
                // inside the marker folder (e.g. `Battle.net\Battle.net.exe`) is launcher
                // plumbing, not a game install.
                if i + 1 == components.len() - 1 {
                    return None;
                }
                if NON_GAME_INSTALL_FOLDERS.contains(&next.to_lowercase().as_str()) {
                    return None;
                }
                return Some(next.clone());
            }
        }
    }
    None
}

/// Turns a raw filesystem name into something RAWG's search can actually match, e.g.
/// `"BloonsTD6"` -> `"Bloons TD 6"` (RAWG lists it as "Bloons TD 6" — searching the unsplit
/// folder name returns nothing). Splits on lowercase→uppercase boundaries, acronym→word
/// boundaries ("TDGame" -> "TD Game"), and letter↔digit boundaries; underscores/dashes/dots
/// become spaces. Idempotent on names that already have spaces (e.g. "Dave the Diver").
fn humanize_name(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut result = String::with_capacity(raw.len() + 4);

    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' || c == '.' {
            if !result.is_empty() && !result.ends_with(' ') {
                result.push(' ');
            }
            continue;
        }

        if i > 0 {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            let boundary = (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_alphabetic() && c.is_numeric())
                || (prev.is_numeric() && c.is_alphabetic())
                || (prev.is_uppercase() && c.is_uppercase() && next.is_some_and(|n| n.is_lowercase()));
            if boundary && !result.ends_with(' ') {
                result.push(' ');
            }
        }
        result.push(c);
    }

    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Scans currently running processes for games installed under a known storefront/launcher
/// directory (Steam, Epic, GOG, Battle.net, Riot) that aren't already linked to a backlog entry.
/// Returns `(raw_exe_name, guessed_display_name)` pairs, deduplicated by normalized exe name.
fn detect_unregistered_games(
    sys: &System,
    tracked_exe_names: &HashSet<String>,
) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();

    for process in sys.processes().values() {
        let raw_name = process.name().to_string();
        let normalized = normalize_exe_name(&raw_name);
        if is_non_game_exe(&normalized) {
            continue;
        }
        if tracked_exe_names.contains(&normalized) || !seen.insert(normalized) {
            continue;
        }
        if let Some(display_name) = process.exe().and_then(guess_game_name_from_path) {
            found.push((raw_name, display_name));
        }
    }

    found
}

/// Looks up `guessed_name` on RAWG (best-effort — a miss or API failure just means the game gets
/// added with the guessed name and no cover art rather than blocking tracking), then creates or
/// links a backlog entry for it and immediately opens its first session. Runs the network call
/// *before* touching the DB so the connection mutex is never held across an `.await`. Returns
/// the game's row id so the caller can exempt it from the stale-session sweep this poll.
async fn auto_register_and_track(
    app: &AppHandle,
    active: &mut HashMap<i64, i64>,
    exe_name: &str,
    guessed_name: &str,
) -> Option<i64> {
    let search_query = humanize_name(guessed_name);
    let display_fallback = if search_query.is_empty() {
        guessed_name.to_string()
    } else {
        search_query.clone()
    };

    let rawg_match = rawg::best_match(&search_query).await;

    let db = app.state::<DbState>();
    let conn = match db.0.lock() {
        Ok(c) => c,
        Err(_) => return None,
    };

    // The upsert's conflict branch also applies the wishlist → library rule: running a game
    // means you own it, and this path is the FIRST launch for any wishlisted game without an
    // exe link (the tracked-games loop only handles games that already have one).
    let game = match &rawg_match {
        Some(m) => games::upsert_auto_registered(
            &conn,
            Some(m.rawg_id),
            &m.name,
            m.cover_url.as_deref(),
            m.genre.as_deref(),
            m.platform.as_deref(),
            exe_name,
        ),
        None => games::upsert_auto_registered(
            &conn, None, &display_fallback, None, None, None, exe_name,
        ),
    };

    let Ok((game_id, name)) = game else { return None };

    // The RAWG match can resolve to a game that's ALREADY being tracked under a different exe
    // name (an update renamed the executable, or a second exe under the same install matched
    // the same rawg_id). Opening a second session would orphan the first in `active` — keep
    // the original session and let the updated exe_name take over from the next poll.
    if active.contains_key(&game_id) {
        return Some(game_id);
    }

    let _ = app.emit("game-auto-added", GameAutoAdded { game_id, name });

    if let Ok((session_id, started_at)) = sessions::open(&conn, game_id) {
        active.insert(game_id, session_id);
        let _ = app.emit(
            "session-started",
            SessionStarted { game_id, session_id, started_at },
        );
    }
    Some(game_id)
}

/// Resolves sessions left open (`ended_at IS NULL`) by a previous run that never shut down
/// cleanly — app crash/force-quit, or the whole OS restarting/losing power while a game was
/// running. Runs once at tracker startup, before the poll loop.
///
/// For each dangling session: if the game's process is still running right now, adopt it back
/// into `active` so tracking resumes under the *original* `started_at` instead of splitting into
/// a second session. Otherwise close it out using the last heartbeat (`last_seen_at`) as a
/// best-effort end time — much closer to the truth than "0 duration" (using started_at) or
/// "however long the machine was off/app was closed" (using now) — and flag it
/// `ended_estimated` so consumers know the duration wasn't measured at a real exit event.
///
/// A game removed from the backlog while a session was still open has no `exe_name` to check
/// (LEFT JOIN misses it), so it can never be "adopted" — it's always closed out immediately.
fn reconcile_dangling_sessions(
    app: &AppHandle,
    conn: &rusqlite::Connection,
    running: &HashSet<String>,
    active: &mut HashMap<i64, i64>,
) {
    let Ok(dangling) = sessions::dangling(conn) else { return };

    // Only the most recent dangling row per game is eligible for adoption (rows come back
    // newest-first per game); any older ones — leftover from repeated crashes before a
    // clean run ever happened — are stale and always get closed.
    let mut seen_games: HashSet<i64> = HashSet::new();

    for d in dangling {
        let is_first_for_game = seen_games.insert(d.game_id);
        let is_running = is_first_for_game
            && d.exe_name
                .as_deref()
                .map(|e| running.contains(&normalize_exe_name(e)))
                .unwrap_or(false);

        if is_running {
            active.insert(d.game_id, d.session_id);
            continue;
        }

        if let Ok(Some(duration_seconds)) = sessions::close_estimated(conn, d.session_id) {
            let _ = app.emit(
                "session-ended",
                SessionEnded {
                    game_id: d.game_id,
                    session_id: d.session_id,
                    duration_seconds,
                },
            );
        }
    }
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // System::new() — not new_all(), which also collects CPU/memory/disk snapshots this
        // tracker never reads; processes are refreshed per poll in running_exe_names.
        let mut sys = System::new();
        // game_id -> session_id for sessions this tracker is actively tracking (either opened
        // by this run, or re-adopted from a dangling session at startup).
        let mut active: HashMap<i64, i64> = HashMap::new();

        {
            // Adopted-session exe name, resolved while the connection is held — capture sync
            // happens after the lock drops.
            let mut startup_exe: Option<String> = None;
            let running = running_exe_names(&mut sys);
            let db = app.state::<DbState>();
            let conn = db.0.lock();
            if let Ok(conn) = conn {
                reconcile_dangling_sessions(&app, &conn, &running, &mut active);
                if let Some(&game_id) = active.keys().next() {
                    startup_exe = games::exe_name(&conn, game_id).ok().flatten();
                }
            }
            // A game was already running when the app launched (adopted above) — start capture
            // for it now rather than waiting for the first poll tick.
            if !active.is_empty() {
                crate::clipper::ensure_capture(&app, startup_exe.as_deref());
            }
        }

        loop {
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            let running = running_exe_names(&mut sys);

            let tracked_games: Vec<(i64, String)> = {
                let db = app.state::<DbState>();
                let conn = match db.0.lock() {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                match games::tracked_exes(&conn) {
                    Ok(rows) => rows.into_iter().map(|t| (t.id, t.exe_name)).collect(),
                    Err(_) => continue,
                }
            };

            // Auto-discover games installed via a known storefront that were launched but never
            // manually added — the network lookup happens before any DB lock is taken. Their exe
            // names are remembered for this poll's capture sync below, since `tracked_games` was
            // fetched before they were registered.
            let tracked_exe_names: HashSet<String> = tracked_games
                .iter()
                .map(|(_, exe)| normalize_exe_name(exe))
                .collect();
            let mut newly_registered_exe: Option<String> = None;
            let mut registered_this_poll: HashSet<i64> = HashSet::new();
            for (exe_name, guessed_name) in detect_unregistered_games(&sys, &tracked_exe_names) {
                if let Some(game_id) =
                    auto_register_and_track(&app, &mut active, &exe_name, &guessed_name).await
                {
                    registered_this_poll.insert(game_id);
                }
                newly_registered_exe = Some(exe_name);
            }

            {
                let db = app.state::<DbState>();
                let Ok(conn) = db.0.lock() else { continue };

                // A game can leave `tracked_games` while its session is still open — deleted
                // from the library mid-session, or its exe link cleared. The loop below only
                // visits tracked games, so without this sweep the stale `active` entry lingers
                // forever: capture keeps recording with no game running, and an unlinked
                // game's session row never closes. Games auto-registered THIS poll are exempt
                // (they were registered after `tracked_games` was fetched).
                let tracked_ids: HashSet<i64> =
                    tracked_games.iter().map(|(id, _)| *id).collect();
                let stale: Vec<(i64, i64)> = active
                    .iter()
                    .filter(|(game_id, _)| {
                        !tracked_ids.contains(game_id) && !registered_this_poll.contains(game_id)
                    })
                    .map(|(g, s)| (*g, *s))
                    .collect();
                for (game_id, session_id) in stale {
                    active.remove(&game_id);
                    // Deleted games take their session rows with them (delete_game's
                    // transaction), so the guarded close simply misses then; an unlinked
                    // game's open row closes out at its last heartbeat.
                    if let Ok(Some(duration_seconds)) = sessions::close_orphaned(&conn, session_id) {
                        let _ = app.emit(
                            "session-ended",
                            SessionEnded { game_id, session_id, duration_seconds },
                        );
                    }
                }

                for (game_id, exe_name) in &tracked_games {
                    let is_running = running.contains(&normalize_exe_name(exe_name));
                    let already_tracking = active.contains_key(game_id);

                    if is_running && already_tracking {
                        // Heartbeat: if this process dies without a clean shutdown, the next
                        // startup's reconciliation pass closes the session here, not at started_at.
                        if let Some(session_id) = active.get(game_id) {
                            let _ = sessions::heartbeat(&conn, *session_id);
                        }
                    } else if is_running && !already_tracking {
                        // Library model: "playing" is derived from the open session, never
                        // stored. The only status a launch changes is wishlist → library:
                        // actually running a game means you own it. Manual marks
                        // (`completed`/`dropped`) stay put — replaying doesn't un-mark them.
                        let _ = games::promote_wishlist_to_library(&conn, *game_id);

                        if let Ok((session_id, started_at)) = sessions::open(&conn, *game_id) {
                            active.insert(*game_id, session_id);
                            let _ = app.emit(
                                "session-started",
                                SessionStarted { game_id: *game_id, session_id, started_at },
                            );
                        }
                    } else if !is_running && already_tracking {
                        if let Some(session_id) = active.remove(game_id) {
                            if let Ok(Some(duration_seconds)) = sessions::close_on_exit(&conn, session_id) {
                                let _ = app.emit(
                                    "session-ended",
                                    SessionEnded { game_id: *game_id, session_id, duration_seconds },
                                );
                            }
                        }
                    }
                }
            }

            // Capture follows "any session is active", reconciled once per poll (after the DB
            // lock drops — spawning/killing ffmpeg shouldn't block other DB users). This one call
            // site covers start-on-launch, stop-when-the-last-game-quits, crash restarts, and
            // Windows window-scope upgrades — see clipper::ensure_capture. Capture targets one
            // window at a time, so a second game quitting never cuts off a still-playing first.
            if active.is_empty() {
                crate::clipper::stop(&app);
            } else {
                let exe = tracked_games
                    .iter()
                    .find(|(id, _)| active.contains_key(id))
                    .map(|(_, e)| e.as_str())
                    .or(newly_registered_exe.as_deref());
                // Focus sampling/pausing lives in clipper's own 1s watcher (spawned by
                // ensure_capture for non-window-scoped captures) — the 5s poll here is too
                // coarse to catch quick alt-tabs.
                crate::clipper::ensure_capture(&app, exe);
            }
        }
    });
}
