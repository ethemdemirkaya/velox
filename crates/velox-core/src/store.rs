//! SQLite persistence (WAL mode). Records are stored as JSON blobs: simple,
//! schema-flexible and still fast for tens of thousands of rows.

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use crate::settings::Settings;
use crate::types::Record;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS downloads (
                id TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            "CREATE TABLE downloads (id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, data TEXT NOT NULL);
             CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        Self { conn: Mutex::new(conn) }
    }

    pub fn load_all(&self) -> Vec<Record> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare("SELECT data FROM downloads ORDER BY created_at") {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = stmt.query_map([], |r| r.get::<_, String>(0));
        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).filter_map(|s| serde_json::from_str(&s).ok()).collect(),
            Err(_) => vec![],
        }
    }

    pub fn upsert(&self, rec: &Record) {
        let Ok(json) = serde_json::to_string(rec) else { return };
        let conn = self.conn.lock();
        if let Err(e) = conn.execute(
            "INSERT INTO downloads (id, created_at, data) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![rec.id, rec.created_at, json],
        ) {
            tracing::warn!("store upsert failed: {e}");
        }
    }

    pub fn upsert_many(&self, recs: &[Record]) {
        if recs.is_empty() {
            return;
        }
        let mut conn = self.conn.lock();
        let Ok(tx) = conn.transaction() else { return };
        for rec in recs {
            if let Ok(json) = serde_json::to_string(rec) {
                let _ = tx.execute(
                    "INSERT INTO downloads (id, created_at, data) VALUES (?1, ?2, ?3)
                     ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                    params![rec.id, rec.created_at, json],
                );
            }
        }
        let _ = tx.commit();
    }

    pub fn delete(&self, id: &str) {
        let _ = self.conn.lock().execute("DELETE FROM downloads WHERE id = ?1", params![id]);
    }

    pub fn settings(&self) -> Settings {
        let conn = self.conn.lock();
        conn.query_row("SELECT value FROM kv WHERE key = 'settings'", [], |r| r.get::<_, String>(0))
            .optional()
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default()
            .sanitized()
    }

    pub fn save_settings(&self, s: &Settings) {
        if let Ok(json) = serde_json::to_string(s) {
            let _ = self.conn.lock().execute(
                "INSERT INTO kv (key, value) VALUES ('settings', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![json],
            );
        }
    }
}
