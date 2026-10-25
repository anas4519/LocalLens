//! Local SQLite storage. Retention rule: text (OCR, later transcripts and
//! embeddings) is kept forever; frame images expire.
//! TODO(m1): pruning job that deletes old image files and nulls `image_path`
//! once a size/age budget is exceeded, leaving `ocr_text` searchable.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Session {
    pub id: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub frame_count: i64,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER
);
CREATE TABLE IF NOT EXISTS frames (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ts          INTEGER NOT NULL,
    image_path  TEXT,              -- nullable: images expire, text stays
    ocr_text    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_frames_session_ts ON frames(session_id, ts);
CREATE VIRTUAL TABLE IF NOT EXISTS frames_fts USING fts5(
    ocr_text, content='frames', content_rowid='id'
);
CREATE TRIGGER IF NOT EXISTS frames_ai AFTER INSERT ON frames BEGIN
    INSERT INTO frames_fts(rowid, ocr_text) VALUES (new.id, new.ocr_text);
END;
CREATE TRIGGER IF NOT EXISTS frames_ad AFTER DELETE ON frames BEGIN
    INSERT INTO frames_fts(frames_fts, rowid, ocr_text) VALUES ('delete', old.id, old.ocr_text);
END;
CREATE TRIGGER IF NOT EXISTS frames_au AFTER UPDATE OF ocr_text ON frames BEGIN
    INSERT INTO frames_fts(frames_fts, rowid, ocr_text) VALUES ('delete', old.id, old.ocr_text);
    INSERT INTO frames_fts(rowid, ocr_text) VALUES (new.id, new.ocr_text);
END;
"#;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[allow(dead_code)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn active_session(&self) -> rusqlite::Result<Option<Session>> {
        self.conn
            .query_row(
                "SELECT s.id, s.started_at, s.ended_at, (SELECT COUNT(*) FROM frames f WHERE f.session_id = s.id)
                 FROM sessions s WHERE s.ended_at IS NULL ORDER BY s.id DESC LIMIT 1",
                [],
                row_to_session,
            )
            .optional()
    }

    /// Starts a session, or returns the one already running.
    pub fn start_session(&self) -> rusqlite::Result<Session> {
        if let Some(s) = self.active_session()? {
            return Ok(s);
        }
        self.conn.execute("INSERT INTO sessions (started_at) VALUES (?1)", params![now_ms()])?;
        self.get(self.conn.last_insert_rowid())
    }

    pub fn stop_session(&self) -> rusqlite::Result<Option<Session>> {
        let Some(active) = self.active_session()? else { return Ok(None) };
        self.conn.execute("UPDATE sessions SET ended_at = ?1 WHERE id = ?2", params![now_ms(), active.id])?;
        self.get(active.id).map(Some)
    }

    pub fn list_sessions(&self) -> rusqlite::Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.started_at, s.ended_at, (SELECT COUNT(*) FROM frames f WHERE f.session_id = s.id)
             FROM sessions s ORDER BY s.started_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_session)?;
        rows.collect()
    }

    #[allow(dead_code)]
    pub fn add_frame(&self, session_id: i64, ts: i64, image_path: Option<&str>, ocr_text: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO frames (session_id, ts, image_path, ocr_text) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, ts, image_path, ocr_text],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Full-text search over OCR text; returns (frame_id, session_id, ts).
    #[allow(dead_code)]
    pub fn search(&self, query: &str) -> rusqlite::Result<Vec<(i64, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.session_id, f.ts FROM frames_fts JOIN frames f ON f.id = frames_fts.rowid
             WHERE frames_fts MATCH ?1 ORDER BY rank",
        )?;
        let rows = stmt.query_map([query], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    fn get(&self, id: i64) -> rusqlite::Result<Session> {
        self.conn.query_row(
            "SELECT s.id, s.started_at, s.ended_at, (SELECT COUNT(*) FROM frames f WHERE f.session_id = s.id)
             FROM sessions s WHERE s.id = ?1",
            [id],
            row_to_session,
        )
    }
}

fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session { id: r.get(0)?, started_at: r.get(1)?, ended_at: r.get(2)?, frame_count: r.get(3)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_start_stop_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session().unwrap();
        assert!(s.ended_at.is_none());
        // Starting again returns the same active session.
        assert_eq!(db.start_session().unwrap().id, s.id);

        let stopped = db.stop_session().unwrap().expect("active session");
        assert_eq!(stopped.id, s.id);
        assert!(stopped.ended_at.unwrap() >= s.started_at);
        assert!(db.active_session().unwrap().is_none());
        assert!(db.stop_session().unwrap().is_none());

        let all = db.list_sessions().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], stopped);
    }

    #[test]
    fn fts_finds_frame_text() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session().unwrap();
        let f = db.add_frame(s.id, 1, None, "spring retry config maxAttempts").unwrap();
        db.add_frame(s.id, 2, Some("frames/2.png"), "unrelated").unwrap();
        let hits = db.search("retry").unwrap();
        assert_eq!(hits, vec![(f, s.id, 1)]);
    }
}
