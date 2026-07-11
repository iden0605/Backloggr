//! The `settings` key/value table. The upsert and the optional-read dance live here once
//! instead of being hand-rolled at every call site.

use rusqlite::{Connection, OptionalExtension, Result};

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    #[test]
    fn get_missing_key_is_none() {
        let conn = test_conn();
        assert_eq!(get(&conn, "nope").unwrap(), None);
    }

    #[test]
    fn set_get_roundtrip_and_upsert_overwrites() {
        let conn = test_conn();
        set(&conn, "clip_seconds", "30").unwrap();
        assert_eq!(get(&conn, "clip_seconds").unwrap().as_deref(), Some("30"));
        set(&conn, "clip_seconds", "60").unwrap();
        assert_eq!(get(&conn, "clip_seconds").unwrap().as_deref(), Some("60"));
    }
}
