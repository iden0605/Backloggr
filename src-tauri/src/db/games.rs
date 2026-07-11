//! The games table: canonical row structs, status semantics, and the game⨝session
//! aggregate reads. Activity ("playing now" / played / never played) is derived from
//! sessions and there is deliberately no verb to store it — `promote_wishlist_to_library`
//! is the only automatic status transition that exists.

use rusqlite::{Connection, OptionalExtension, Result};
use serde::Serialize;

/// Best-known end of the most recent session: falls back through the heartbeat to the
/// session start for a still-open session. NULL = never played. THE last-played invariant —
/// every query that needs it interpolates this.
const LAST_PLAYED: &str = "MAX(COALESCE(s.ended_at, s.last_seen_at, s.started_at))";

/// Typed status. `backlog` = plain in-library (v2 model), `completed`/`dropped` are the two
/// manual marks, `wishlist` is the separate tab. Legacy `playing` rows are normalized to
/// `backlog` at init; `parse` mirrors that for defensive reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Backlog,
    Completed,
    Dropped,
    Wishlist,
}

impl Status {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Status::Backlog => "backlog",
            Status::Completed => "completed",
            Status::Dropped => "dropped",
            Status::Wishlist => "wishlist",
        }
    }

    /// None for a string that isn't a valid status (frontend contract violation).
    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "backlog" | "playing" => Some(Status::Backlog), // 'playing' is legacy
            "completed" => Some(Status::Completed),
            "dropped" => Some(Status::Dropped),
            "wishlist" => Some(Status::Wishlist),
            _ => None,
        }
    }
}

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
    /// See LAST_PLAYED. NULL = never played.
    pub last_played_at: Option<String>,
    pub session_count: i64,
}

/// Raw per-game session aggregates; the command layer derives avg-session and pairs it
/// with its bespoke sparkline query.
pub struct PlaytimeStats {
    pub total_seconds: i64,
    pub last_played_at: Option<String>,
    pub session_count: i64,
    /// Sessions with a measured duration — denominator for "avg session length".
    pub finished_count: i64,
}

pub struct TrackedExe {
    pub id: i64,
    pub exe_name: String,
}

/// The Library page's one read: every game with its session aggregates, added-desc.
pub fn library(conn: &Connection) -> Result<Vec<LibraryGame>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT g.id, g.rawg_id, g.name, g.cover_url, g.genre, g.platform, g.status,
                g.rating, g.notes, g.exe_name, g.added_at, g.completed_at,
                COALESCE(SUM(s.duration_seconds), 0),
                {LAST_PLAYED},
                COUNT(s.id)
         FROM games g LEFT JOIN sessions s ON s.game_id = g.id
         GROUP BY g.id ORDER BY g.added_at DESC"
    ))?;
    let rows = stmt.query_map([], |row| {
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
    })?;
    rows.collect()
}

/// Headline aggregates for one game (detail-page stat tiles).
pub fn playtime_stats(conn: &Connection, id: i64) -> Result<PlaytimeStats> {
    conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(s.duration_seconds), 0),
                    {LAST_PLAYED},
                    COUNT(s.id),
                    COUNT(s.duration_seconds)
             FROM sessions s WHERE s.game_id = ?1"
        ),
        [id],
        |row| {
            Ok(PlaytimeStats {
                total_seconds: row.get(0)?,
                last_played_at: row.get(1)?,
                session_count: row.get(2)?,
                finished_count: row.get(3)?,
            })
        },
    )
}

/// Every game the tracker polls for: a non-empty exe link.
pub fn tracked_exes(conn: &Connection) -> Result<Vec<TrackedExe>> {
    let mut stmt =
        conn.prepare("SELECT id, exe_name FROM games WHERE exe_name IS NOT NULL AND exe_name != ''")?;
    let rows = stmt.query_map([], |row| {
        Ok(TrackedExe { id: row.get(0)?, exe_name: row.get(1)? })
    })?;
    rows.collect()
}

pub fn exe_name(conn: &Connection, id: i64) -> Result<Option<String>> {
    conn.query_row("SELECT exe_name FROM games WHERE id = ?1", [id], |row| row.get(0))
        .optional()
        .map(Option::flatten)
}

/// Running a game means you own it: wishlist → library on first launch. The ONLY automatic
/// status transition; manual marks (completed/dropped) are never touched.
pub fn promote_wishlist_to_library(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE games SET status = 'backlog' WHERE id = ?1 AND status = 'wishlist'",
        [id],
    )?;
    Ok(())
}

/// Manual status mark. Owns the completed_at side effect: set on completed, cleared otherwise.
pub fn set_status(conn: &Connection, id: i64, status: Status) -> Result<()> {
    let completed_at = if status == Status::Completed {
        "completed_at = CURRENT_TIMESTAMP"
    } else {
        "completed_at = NULL"
    };
    conn.execute(
        &format!("UPDATE games SET status = ?1, {completed_at} WHERE id = ?2"),
        rusqlite::params![status.as_sql(), id],
    )?;
    Ok(())
}

pub fn set_exe_name(conn: &Connection, id: i64, exe_name: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE games SET exe_name = ?1 WHERE id = ?2",
        rusqlite::params![exe_name, id],
    )?;
    Ok(())
}

/// Manual add from search: insert-or-ignore by rawg_id, returning the row id either way.
pub fn add_by_rawg(
    conn: &Connection,
    rawg_id: i64,
    name: &str,
    cover_url: Option<&str>,
    genre: Option<&str>,
    platform: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO games (rawg_id, name, cover_url, genre, platform) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(rawg_id) DO NOTHING",
        rusqlite::params![rawg_id, name, cover_url, genre, platform],
    )?;
    conn.query_row("SELECT id FROM games WHERE rawg_id = ?1", [rawg_id], |row| row.get(0))
}

/// The tracker's auto-register upsert. With a rawg_id: insert as in-library, or on conflict
/// take over the exe link and apply wishlist → library (this path IS the first launch for a
/// wishlisted game without an exe link). Without one: plain insert under the guessed name.
/// Returns (id, name) — name may differ from the input on the conflict path.
pub fn upsert_auto_registered(
    conn: &Connection,
    rawg_id: Option<i64>,
    name: &str,
    cover_url: Option<&str>,
    genre: Option<&str>,
    platform: Option<&str>,
    exe_name: &str,
) -> Result<(i64, String)> {
    match rawg_id {
        Some(rawg_id) => conn.query_row(
            "INSERT INTO games (rawg_id, name, cover_url, genre, platform, status, exe_name)
             VALUES (?1, ?2, ?3, ?4, ?5, 'backlog', ?6)
             ON CONFLICT(rawg_id) DO UPDATE SET
                exe_name = excluded.exe_name,
                status = CASE WHEN games.status = 'wishlist' THEN 'backlog' ELSE games.status END
             RETURNING id, name",
            rusqlite::params![rawg_id, name, cover_url, genre, platform, exe_name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ),
        None => conn.query_row(
            "INSERT INTO games (name, status, exe_name) VALUES (?1, 'backlog', ?2) RETURNING id, name",
            rusqlite::params![name, exe_name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ),
    }
}

/// Deletes a game and its FK dependents (sessions, clips) in one transaction, dependents
/// first — `foreign_keys = ON` rejects the delete otherwise. The pattern for any future
/// delete of a referenced row.
pub fn delete_cascading(conn: &mut Connection, id: i64) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM sessions WHERE game_id = ?1", [id])?;
    tx.execute("DELETE FROM clips WHERE game_id = ?1", [id])?;
    tx.execute("DELETE FROM games WHERE id = ?1", [id])?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{sessions, test_conn};

    fn insert_game(conn: &Connection, name: &str) -> i64 {
        conn.query_row(
            "INSERT INTO games (name) VALUES (?1) RETURNING id",
            [name],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn last_played_prefers_ended_then_heartbeat_then_started() {
        let conn = test_conn();
        let id = insert_game(&conn, "Celeste");

        // Closed session: last_played = ended_at.
        conn.execute(
            "INSERT INTO sessions (game_id, started_at, ended_at, last_seen_at, duration_seconds)
             VALUES (?1, '2026-01-01 10:00:00', '2026-01-01 12:00:00', '2026-01-01 11:59:00', 7200)",
            [id],
        )
        .unwrap();
        let stats = playtime_stats(&conn, id).unwrap();
        assert_eq!(stats.last_played_at.as_deref(), Some("2026-01-01 12:00:00"));

        // Open session with a later heartbeat: heartbeat wins over the closed session's end.
        conn.execute(
            "INSERT INTO sessions (game_id, started_at, last_seen_at)
             VALUES (?1, '2026-01-02 09:00:00', '2026-01-02 09:30:00')",
            [id],
        )
        .unwrap();
        let stats = playtime_stats(&conn, id).unwrap();
        assert_eq!(stats.last_played_at.as_deref(), Some("2026-01-02 09:30:00"));

        // Open session with no heartbeat at all: started_at is the floor.
        let bare = insert_game(&conn, "Bare");
        conn.execute(
            "INSERT INTO sessions (game_id, started_at) VALUES (?1, '2026-01-03 08:00:00')",
            [bare],
        )
        .unwrap();
        let stats = playtime_stats(&conn, bare).unwrap();
        assert_eq!(stats.last_played_at.as_deref(), Some("2026-01-03 08:00:00"));
    }

    #[test]
    fn library_aggregates_and_never_played_nulls() {
        let conn = test_conn();
        let played = insert_game(&conn, "Played");
        let never = insert_game(&conn, "Never");
        conn.execute(
            "INSERT INTO sessions (game_id, started_at, ended_at, duration_seconds)
             VALUES (?1, '2026-01-01 10:00:00', '2026-01-01 11:00:00', 3600),
                    (?1, '2026-01-02 10:00:00', '2026-01-02 10:30:00', 1800)",
            [played],
        )
        .unwrap();

        let rows = library(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        let by_name = |n: &str| rows.iter().find(|g| g.game.name == n).unwrap();
        let p = by_name("Played");
        assert_eq!(p.total_seconds, 5400);
        assert_eq!(p.session_count, 2);
        assert_eq!(p.last_played_at.as_deref(), Some("2026-01-02 10:30:00"));
        let n = by_name("Never");
        assert_eq!(n.total_seconds, 0);
        assert_eq!(n.session_count, 0);
        assert_eq!(n.last_played_at, None);
    }

    #[test]
    fn playtime_stats_finished_count_excludes_open_sessions() {
        let conn = test_conn();
        let id = insert_game(&conn, "Hades");
        conn.execute(
            "INSERT INTO sessions (game_id, started_at, ended_at, duration_seconds)
             VALUES (?1, '2026-01-01 10:00:00', '2026-01-01 11:00:00', 3600)",
            [id],
        )
        .unwrap();
        sessions::open(&conn, id).unwrap(); // still running — no duration yet
        let stats = playtime_stats(&conn, id).unwrap();
        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.finished_count, 1);
    }

    #[test]
    fn delete_cascading_survives_sessions_and_clips() {
        let mut conn = test_conn();
        let id = insert_game(&conn, "Tunic");
        sessions::open(&conn, id).unwrap();
        conn.execute(
            "INSERT INTO clips (game_id, file_path) VALUES (?1, '/tmp/clip.mp4')",
            [id],
        )
        .unwrap();
        // The FK case that originally broke delete_game: dependents exist, foreign_keys = ON.
        delete_cascading(&mut conn, id).unwrap();
        let games: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap();
        let sess: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap();
        let clips: i64 = conn.query_row("SELECT COUNT(*) FROM clips", [], |r| r.get(0)).unwrap();
        assert_eq!((games, sess, clips), (0, 0, 0));
    }

    #[test]
    fn promote_wishlist_only_touches_wishlist() {
        let conn = test_conn();
        let wish = insert_game(&conn, "Wish");
        let done = insert_game(&conn, "Done");
        conn.execute("UPDATE games SET status = 'wishlist' WHERE id = ?1", [wish]).unwrap();
        conn.execute("UPDATE games SET status = 'completed' WHERE id = ?1", [done]).unwrap();

        promote_wishlist_to_library(&conn, wish).unwrap();
        promote_wishlist_to_library(&conn, done).unwrap();

        let status = |id: i64| -> String {
            conn.query_row("SELECT status FROM games WHERE id = ?1", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(status(wish), "backlog");
        assert_eq!(status(done), "completed"); // manual mark untouched
    }

    #[test]
    fn set_status_owns_completed_at() {
        let conn = test_conn();
        let id = insert_game(&conn, "Celeste");
        set_status(&conn, id, Status::Completed).unwrap();
        let completed_at: Option<String> = conn
            .query_row("SELECT completed_at FROM games WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert!(completed_at.is_some());

        set_status(&conn, id, Status::Backlog).unwrap();
        let completed_at: Option<String> = conn
            .query_row("SELECT completed_at FROM games WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(completed_at, None);
    }

    #[test]
    fn status_parse_normalizes_legacy_playing() {
        assert_eq!(Status::parse("playing"), Some(Status::Backlog));
        assert_eq!(Status::parse("wishlist"), Some(Status::Wishlist));
        assert_eq!(Status::parse("bogus"), None);
    }

    #[test]
    fn upsert_auto_registered_takes_over_exe_and_promotes_wishlist() {
        let conn = test_conn();
        let id = conn
            .query_row(
                "INSERT INTO games (rawg_id, name, status) VALUES (42, 'Elden Ring', 'wishlist') RETURNING id",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap();

        let (upserted_id, name) =
            upsert_auto_registered(&conn, Some(42), "Elden Ring", None, None, None, "eldenring.exe")
                .unwrap();
        assert_eq!(upserted_id, id);
        assert_eq!(name, "Elden Ring");
        let (status, exe): (String, Option<String>) = conn
            .query_row("SELECT status, exe_name FROM games WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "backlog"); // wishlist → library on first launch
        assert_eq!(exe.as_deref(), Some("eldenring.exe"));
    }
}
