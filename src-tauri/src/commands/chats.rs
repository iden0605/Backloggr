// Ask AI chat history (chat_conversations CRUD). Turns are an opaque JSON blob of the
// frontend's ChatTurn shape — Rust stores and returns it verbatim, the frontend owns
// (de)serialization.

use crate::db::DbState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversation {
    pub id: i64,
    pub title: String,
    pub turns_json: String,
    pub questions_asked: i64,
}

#[tauri::command]
pub fn list_chats(db: State<DbState>) -> Result<Vec<ChatSummary>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, updated_at FROM chat_conversations
             ORDER BY updated_at DESC, id DESC",
        )
        .map_err(|e| e.to_string())?;
    let chats = stmt
        .query_map([], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(chats)
}

#[tauri::command]
pub fn get_chat(db: State<DbState>, id: i64) -> Result<ChatConversation, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, title, turns_json, questions_asked FROM chat_conversations WHERE id = ?1",
        [id],
        |row| {
            Ok(ChatConversation {
                id: row.get(0)?,
                title: row.get(1)?,
                turns_json: row.get(2)?,
                questions_asked: row.get(3)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

/// Upsert a conversation after each completed exchange; returns the row id so the
/// frontend can adopt it after the first save of a new chat. An id that no longer
/// exists (deleted from the history panel mid-conversation) falls through to insert.
#[tauri::command]
pub fn save_chat(
    db: State<DbState>,
    id: Option<i64>,
    title: String,
    turns_json: String,
    questions_asked: i64,
) -> Result<i64, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    if let Some(id) = id {
        let updated = conn
            .execute(
                "UPDATE chat_conversations
                 SET turns_json = ?1, questions_asked = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?3",
                rusqlite::params![turns_json, questions_asked, id],
            )
            .map_err(|e| e.to_string())?;
        if updated > 0 {
            return Ok(id);
        }
    }
    conn.execute(
        "INSERT INTO chat_conversations (title, turns_json, questions_asked) VALUES (?1, ?2, ?3)",
        rusqlite::params![title, turns_json, questions_asked],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_chat(db: State<DbState>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_conversations WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
