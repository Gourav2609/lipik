//! Scratchpad notes in `scratchpad.db` in the app data folder.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use specta::Type;
use std::path::PathBuf;

#[derive(Serialize, Debug, Clone, Type)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    /// Unix milliseconds.
    pub updated_at: i64,
}

pub struct Notes {
    path: PathBuf,
}

impl Notes {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn open(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notes (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .map_err(|e| e.to_string())?;
        Ok(conn)
    }

    fn map(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
        Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            body: row.get(2)?,
            updated_at: row.get(3)?,
        })
    }

    pub fn list(&self) -> Result<Vec<Note>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT id, title, body, updated_at FROM notes ORDER BY updated_at DESC")
            .map_err(|e| e.to_string())?;
        let notes = stmt
            .query_map([], Self::map)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|e| e.to_string())?;
        Ok(notes)
    }

    pub fn get(&self, id: &str) -> Result<Option<Note>, String> {
        self.open()?
            .query_row(
                "SELECT id, title, body, updated_at FROM notes WHERE id = ?1",
                params![id],
                Self::map,
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Inserts or updates; a caller-chosen id is kept so notes made in the page
    /// keep their ids.
    pub fn save(&self, id: Option<String>, title: &str, body: &str) -> Result<String, String> {
        let id = id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(super::store::new_id);
        self.open()?
            .execute(
                "INSERT INTO notes (id, title, body, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET title = ?2, body = ?3, updated_at = ?4",
                params![id, title, body, chrono::Utc::now().timestamp_millis()],
            )
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.open()?
            .execute("DELETE FROM notes WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
