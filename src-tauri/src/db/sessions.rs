//! Session lifecycle: the tracker's verbs plus the live "currently playing" read. The
//! close formula — `ended_at` source, the julianday duration cast, the `ended_estimated`
//! flag — is written once in `finalize`; the three public `close_*` verbs are the only
//! close semantics that exist (natural exit, crash reconciliation, stale sweep).

use rusqlite::{Connection, OptionalExtension, Result};
use serde::Serialize;

/// A session row left open (`ended_at IS NULL`) by a previous run, newest-first per game.
/// `exe_name` is NULL when the game was deleted mid-session (LEFT JOIN miss) or never linked.
pub struct Dangling {
    pub session_id: i64,
    pub game_id: i64,
    pub exe_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentGame {
    pub game_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub started_at: String,
    /// How many OTHER games also have an open session right now. Multiple games running at
    /// once is normal (launcher-spawned games, two games mid-swap) — consumers show the most
    /// recently launched one plus a "+N more" so the display isn't silently lying.
    pub also_playing: i64,
}

/// Opens a session at CURRENT_TIMESTAMP (started_at == last_seen_at, auto_tracked).
/// Returns (session_id, started_at) for the session-started event.
pub fn open(conn: &Connection, game_id: i64) -> Result<(i64, String)> {
    conn.query_row(
        "INSERT INTO sessions (game_id, started_at, last_seen_at, auto_tracked)
         VALUES (?1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 1) RETURNING id, started_at",
        [game_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

/// Heartbeat: written on every poll a tracked session is confirmed still running, so an
/// unclean shutdown can be closed out near the true end time by `close_estimated`.
pub fn heartbeat(conn: &Connection, session_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET last_seen_at = CURRENT_TIMESTAMP WHERE id = ?1",
        [session_id],
    )?;
    Ok(())
}

/// Natural process exit: the session ends NOW, duration measured to this instant.
pub fn close_on_exit(conn: &Connection, session_id: i64) -> Result<Option<i64>> {
    finalize(conn, session_id, EndAt::Now, false, false)
}

/// Crash reconciliation: the true exit was never observed, so the session ends at its last
/// heartbeat (falling back to started_at) and is flagged `ended_estimated`.
pub fn close_estimated(conn: &Connection, session_id: i64) -> Result<Option<i64>> {
    finalize(conn, session_id, EndAt::LastHeartbeat, true, false)
}

/// Stale sweep (game deleted / exe link cleared mid-session): closes at the last heartbeat,
/// guarded by `ended_at IS NULL` so an already-closed or deleted row is a no-op (`None`).
pub fn close_orphaned(conn: &Connection, session_id: i64) -> Result<Option<i64>> {
    finalize(conn, session_id, EndAt::LastHeartbeat, false, true)
}

enum EndAt {
    Now,
    LastHeartbeat,
}

/// The ONLY place the session-close formula is written. Returns Some(duration_seconds) when
/// a row closed (→ emit session-ended), None when nothing matched.
fn finalize(
    conn: &Connection,
    session_id: i64,
    end: EndAt,
    estimated: bool,
    only_if_open: bool,
) -> Result<Option<i64>> {
    let ended = match end {
        EndAt::Now => "CURRENT_TIMESTAMP",
        EndAt::LastHeartbeat => "COALESCE(last_seen_at, started_at)",
    };
    let est = if estimated { ", ended_estimated = 1" } else { "" };
    let guard = if only_if_open { "AND ended_at IS NULL" } else { "" };
    let sql = format!(
        "UPDATE sessions SET ended_at = {ended},
            duration_seconds = CAST((julianday({ended}) - julianday(started_at)) * 86400 AS INTEGER){est}
         WHERE id = ?1 {guard}
         RETURNING duration_seconds"
    );
    conn.query_row(&sql, [session_id], |row| row.get(0)).optional()
}

/// Feed for the startup reconciliation pass: every open session, newest-first per game so
/// the caller can adopt only the most recent one per game and close the rest.
pub fn dangling(conn: &Connection) -> Result<Vec<Dangling>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.game_id, g.exe_name
         FROM sessions s LEFT JOIN games g ON g.id = s.game_id
         WHERE s.ended_at IS NULL
         ORDER BY s.game_id, s.started_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Dangling {
            session_id: row.get(0)?,
            game_id: row.get(1)?,
            exe_name: row.get(2)?,
        })
    })?;
    rows.collect()
}

/// The live "playing now" read: the MOST RECENTLY STARTED open session, straight from the
/// DB rather than from events (events emitted before a listener attaches are lost).
pub fn current_game(conn: &Connection) -> Result<Option<CurrentGame>> {
    conn.query_row(
        "SELECT g.id, g.name, g.cover_url, s.started_at,
                (SELECT COUNT(*) - 1 FROM sessions WHERE ended_at IS NULL) AS also_playing
         FROM sessions s JOIN games g ON g.id = s.game_id
         WHERE s.ended_at IS NULL
         ORDER BY s.started_at DESC LIMIT 1",
        [],
        |row| {
            Ok(CurrentGame {
                game_id: row.get(0)?,
                name: row.get(1)?,
                cover_url: row.get(2)?,
                started_at: row.get(3)?,
                also_playing: row.get::<_, i64>(4)?.max(0),
            })
        },
    )
    .optional()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    fn insert_game(conn: &Connection, name: &str) -> i64 {
        conn.query_row(
            "INSERT INTO games (name) VALUES (?1) RETURNING id",
            [name],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn open_heartbeat_close_on_exit() {
        let conn = test_conn();
        let game_id = insert_game(&conn, "Celeste");
        let (sid, started_at) = open(&conn, game_id).unwrap();
        assert!(!started_at.is_empty());
        heartbeat(&conn, sid).unwrap();
        // Same-instant close: duration rounds to 0, but a row must close and return Some.
        assert_eq!(close_on_exit(&conn, sid).unwrap(), Some(0));
        let (ended_at, estimated): (Option<String>, bool) = conn
            .query_row(
                "SELECT ended_at, ended_estimated FROM sessions WHERE id = ?1",
                [sid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(ended_at.is_some());
        assert!(!estimated);
    }

    #[test]
    fn close_estimated_uses_last_heartbeat_and_flags() {
        let conn = test_conn();
        let game_id = insert_game(&conn, "Hades");
        let (sid, _) = open(&conn, game_id).unwrap();
        conn.execute(
            "UPDATE sessions SET started_at = '2026-01-01 10:00:00',
                                 last_seen_at = '2026-01-01 11:00:00' WHERE id = ?1",
            [sid],
        )
        .unwrap();
        assert_eq!(close_estimated(&conn, sid).unwrap(), Some(3600));
        let (ended_at, estimated): (String, bool) = conn
            .query_row(
                "SELECT ended_at, ended_estimated FROM sessions WHERE id = ?1",
                [sid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(ended_at, "2026-01-01 11:00:00");
        assert!(estimated);
    }

    #[test]
    fn close_estimated_falls_back_to_started_at_without_heartbeat() {
        let conn = test_conn();
        let game_id = insert_game(&conn, "Outer Wilds");
        let (sid, _) = open(&conn, game_id).unwrap();
        conn.execute(
            "UPDATE sessions SET started_at = '2026-01-01 10:00:00', last_seen_at = NULL WHERE id = ?1",
            [sid],
        )
        .unwrap();
        assert_eq!(close_estimated(&conn, sid).unwrap(), Some(0));
    }

    #[test]
    fn close_orphaned_is_noop_on_closed_or_missing_rows() {
        let conn = test_conn();
        let game_id = insert_game(&conn, "Tunic");
        let (sid, _) = open(&conn, game_id).unwrap();
        assert!(close_on_exit(&conn, sid).unwrap().is_some());
        // Already closed → guarded update misses.
        assert_eq!(close_orphaned(&conn, sid).unwrap(), None);
        // Row gone entirely (deleted game took its sessions) → also None.
        assert_eq!(close_orphaned(&conn, 9999).unwrap(), None);
    }

    #[test]
    fn dangling_lists_open_sessions_newest_first_per_game() {
        let conn = test_conn();
        let game_id = insert_game(&conn, "Balatro");
        let (old_sid, _) = open(&conn, game_id).unwrap();
        conn.execute(
            "UPDATE sessions SET started_at = '2026-01-01 08:00:00' WHERE id = ?1",
            [old_sid],
        )
        .unwrap();
        let (new_sid, _) = open(&conn, game_id).unwrap();
        let rows = dangling(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].session_id, new_sid);
        assert_eq!(rows[1].session_id, old_sid);
        assert_eq!(rows[0].exe_name, None);
    }

    #[test]
    fn current_game_returns_most_recent_open_with_also_playing() {
        let conn = test_conn();
        assert!(current_game(&conn).unwrap().is_none());

        let first = insert_game(&conn, "First");
        let second = insert_game(&conn, "Second");
        let (first_sid, _) = open(&conn, first).unwrap();
        conn.execute(
            "UPDATE sessions SET started_at = '2026-01-01 08:00:00' WHERE id = ?1",
            [first_sid],
        )
        .unwrap();
        open(&conn, second).unwrap();

        let current = current_game(&conn).unwrap().unwrap();
        assert_eq!(current.name, "Second");
        assert_eq!(current.also_playing, 1);
    }
}
