use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct DbState(pub Mutex<Connection>);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS games (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  rawg_id INTEGER UNIQUE,
  name TEXT NOT NULL,
  cover_url TEXT,
  genre TEXT,
  platform TEXT,
  status TEXT CHECK(status IN ('backlog','playing','completed','dropped','wishlist')) DEFAULT 'backlog',
  rating INTEGER CHECK(rating BETWEEN 1 AND 5),
  notes TEXT,
  exe_name TEXT,
  added_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  completed_at DATETIME
);

CREATE TABLE IF NOT EXISTS sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  game_id INTEGER REFERENCES games(id),
  started_at DATETIME NOT NULL,
  ended_at DATETIME,
  duration_seconds INTEGER,
  auto_tracked BOOLEAN DEFAULT 1
);

CREATE TABLE IF NOT EXISTS clips (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  game_id INTEGER REFERENCES games(id),
  file_path TEXT NOT NULL,
  thumbnail_path TEXT,
  duration_seconds INTEGER,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  title TEXT,
  notes TEXT
);

CREATE TABLE IF NOT EXISTS recommendations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  prompt TEXT,
  response TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT
);

-- Single-row cache (id is always 1) for the Dashboard's \"based on your activity\" AI
-- recommendations — regenerated only when backlog size, top-played genre, or that genre's
-- playtime shift meaningfully (see commands::get_dashboard_recommendations), not on every
-- Dashboard load.
CREATE TABLE IF NOT EXISTS dashboard_recommendations_cache (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  generated_at INTEGER NOT NULL,
  backlog_count INTEGER NOT NULL,
  top_genre TEXT,
  top_genre_playtime_seconds INTEGER NOT NULL,
  reasoning TEXT NOT NULL,
  games_json TEXT NOT NULL
);
";

/// Adds a column to an existing table if it isn't there yet — lets us evolve `SCHEMA` without
/// breaking installs that already created the table via `CREATE TABLE IF NOT EXISTS`.
fn add_column_if_missing(conn: &Connection, table: &str, column: &str, column_def: &str) {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("failed to inspect table schema");
    let has_column = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("failed to read table schema")
        .filter_map(|r| r.ok())
        .any(|name| name == column);
    if !has_column {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column_def}"))
            .expect("failed to add column");
    }
}

pub fn init(app_data_dir: &PathBuf) -> Connection {
    std::fs::create_dir_all(app_data_dir).expect("failed to create app data dir");
    let db_path = app_data_dir.join("data.db");
    let conn = Connection::open(db_path).expect("failed to open sqlite database");
    // WAL + NORMAL sync: writes survive app crashes/kills without the fsync cost of FULL,
    // and readers never block on a writer mid-session-tracking.
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("failed to set WAL mode");
    conn.pragma_update(None, "synchronous", "NORMAL")
        .expect("failed to set synchronous mode");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("failed to enable foreign keys");
    conn.execute_batch(SCHEMA).expect("failed to initialize schema");

    // last_seen_at: heartbeat written on every poll a tracked session is confirmed still
    // running, so a crash/restart can be closed out near the true end time instead of at
    // `started_at` (0 duration) or "now" (inflated duration). ended_estimated flags sessions
    // closed this way (by tracker::reconcile_dangling_sessions) rather than at natural exit.
    add_column_if_missing(&conn, "sessions", "last_seen_at", "last_seen_at DATETIME");
    add_column_if_missing(
        &conn,
        "sessions",
        "ended_estimated",
        "ended_estimated BOOLEAN DEFAULT 0",
    );

    conn
}
