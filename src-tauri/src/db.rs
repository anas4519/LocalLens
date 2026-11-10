//! Local SQLite storage. Retention rule: text (OCR, later transcripts and
//! embeddings) is kept forever; frame images expire.
//! TODO(m1): pruning job that deletes old image files and nulls `image_path`
//! once a size/age budget is exceeded, leaving `ocr_text` searchable.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const OCR_PENDING: i64 = 0;
pub const OCR_DONE: i64 = 1;
pub const OCR_FAILED: i64 = 2;

/// Longest session name we store, in characters (not bytes, so emoji/Hindi are safe).
pub const MAX_SESSION_NAME: usize = 80;

/// Trim, treat blank as "no name", and cap the length. Used for start and rename.
pub fn normalize_name(name: Option<&str>) -> Option<String> {
    let t = name?.trim();
    if t.is_empty() {
        return None;
    }
    Some(
        t.chars()
            .take(MAX_SESSION_NAME)
            .collect::<String>()
            .trim_end()
            .to_string(),
    )
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FrameRow {
    pub id: i64,
    pub ts: i64,
    /// Relative to the app data dir; None once the image was pruned.
    pub image_path: Option<String>,
    pub ocr_text: String,
    /// 0 = pending, 1 = done (text may still be empty), 2 = failed.
    pub ocr_status: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Session {
    pub id: i64,
    /// Optional user label; the UI falls back to the start date + time.
    pub name: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub frame_count: i64,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    name        TEXT               -- nullable: unnamed sessions show date + time
);
CREATE TABLE IF NOT EXISTS frames (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ts          INTEGER NOT NULL,
    image_path  TEXT,              -- nullable: images expire, text stays
    ocr_text    TEXT NOT NULL DEFAULT '',
    ocr_status  INTEGER NOT NULL DEFAULT 0  -- 0 pending, 1 done, 2 failed
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

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Bring databases created by older builds up to the current schema.
/// Each step is idempotent (checks before it alters), so it is safe on every launch.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    // v0.3: optional session name. Existing rows get NULL = unnamed.
    if !column_exists(conn, "sessions", "name")? {
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN name TEXT;")?;
    }
    // v0.2: `ocr_status`, and classify already-stored rows so we don't re-OCR finished work.
    if !column_exists(conn, "frames", "ocr_status")? {
        conn.execute_batch("ALTER TABLE frames ADD COLUMN ocr_status INTEGER NOT NULL DEFAULT 0;")?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_frames_ocr_status ON frames(ocr_status);")?;
    // Already has text, or can never be read (image pruned): mark done.
    conn.execute(
        "UPDATE frames SET ocr_status = ?1 WHERE ocr_status = ?2 AND (ocr_text != '' OR image_path IS NULL)",
        params![OCR_DONE, OCR_PENDING],
    )?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn active_session(&self) -> rusqlite::Result<Option<Session>> {
        self.conn
            .query_row(
                "SELECT s.id, s.name, s.started_at, s.ended_at, (SELECT COUNT(*) FROM frames f WHERE f.session_id = s.id)
                 FROM sessions s WHERE s.ended_at IS NULL ORDER BY s.id DESC LIMIT 1",
                [],
                row_to_session,
            )
            .optional()
    }

    /// Starts a session, or returns the one already running (its name is left as is).
    pub fn start_session(&self, name: Option<&str>) -> rusqlite::Result<Session> {
        if let Some(s) = self.active_session()? {
            return Ok(s);
        }
        self.conn.execute(
            "INSERT INTO sessions (started_at, name) VALUES (?1, ?2)",
            params![now_ms(), normalize_name(name)],
        )?;
        self.get(self.conn.last_insert_rowid())
    }

    /// Set, change, or clear (None / blank) a session's name. Ok(None) if no such session.
    pub fn rename_session(&self, id: i64, name: Option<&str>) -> rusqlite::Result<Option<Session>> {
        let changed = self.conn.execute(
            "UPDATE sessions SET name = ?1 WHERE id = ?2",
            params![normalize_name(name), id],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.get(id).map(Some)
    }

    pub fn stop_session(&self) -> rusqlite::Result<Option<Session>> {
        let Some(active) = self.active_session()? else {
            return Ok(None);
        };
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE id = ?2",
            params![now_ms(), active.id],
        )?;
        self.get(active.id).map(Some)
    }

    pub fn list_sessions(&self) -> rusqlite::Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.started_at, s.ended_at, (SELECT COUNT(*) FROM frames f WHERE f.session_id = s.id)
             FROM sessions s ORDER BY s.started_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_session)?;
        rows.collect()
    }

    #[allow(dead_code)]
    pub fn add_frame(
        &self,
        session_id: i64,
        ts: i64,
        image_path: Option<&str>,
        ocr_text: &str,
    ) -> rusqlite::Result<i64> {
        let status = if !ocr_text.is_empty() {
            OCR_DONE
        } else if image_path.is_some() {
            OCR_PENDING
        } else {
            OCR_DONE // no image to read
        };
        self.conn.execute(
            "INSERT INTO frames (session_id, ts, image_path, ocr_text, ocr_status) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, ts, image_path, ocr_text, status],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_ocr_result(&self, id: i64, ocr_text: &str, ocr_status: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE frames SET ocr_text = ?1, ocr_status = ?2 WHERE id = ?3",
            params![ocr_text, ocr_status, id],
        )?;
        Ok(())
    }

    /// Pending jobs that still have an image on disk (path stored; file may still be missing).
    pub fn pending_ocr_ids(&self) -> rusqlite::Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM frames WHERE ocr_status = ?1 AND image_path IS NOT NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([OCR_PENDING], |r| r.get(0))?;
        rows.collect()
    }

    pub fn ocr_job(&self, id: i64) -> rusqlite::Result<Option<(Option<String>, i64)>> {
        self.conn
            .query_row(
                "SELECT image_path, ocr_status FROM frames WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    pub fn ocr_pending_count(&self) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM frames WHERE ocr_status = ?1",
            [OCR_PENDING],
            |r| r.get(0),
        )
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

    /// All frames of a session, oldest first.
    pub fn list_frames(&self, session_id: i64) -> rusqlite::Result<Vec<FrameRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, image_path, ocr_text, ocr_status FROM frames WHERE session_id = ?1 ORDER BY ts, id",
        )?;
        let rows = stmt.query_map([session_id], |r| {
            Ok(FrameRow {
                id: r.get(0)?,
                ts: r.get(1)?,
                image_path: r.get(2)?,
                ocr_text: r.get(3)?,
                ocr_status: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    fn get(&self, id: i64) -> rusqlite::Result<Session> {
        self.conn.query_row(
            "SELECT s.id, s.name, s.started_at, s.ended_at, (SELECT COUNT(*) FROM frames f WHERE f.session_id = s.id)
             FROM sessions s WHERE s.id = ?1",
            [id],
            row_to_session,
        )
    }
}

fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        name: r.get(1)?,
        started_at: r.get(2)?,
        ended_at: r.get(3)?,
        frame_count: r.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_start_stop_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        assert!(s.ended_at.is_none());
        // Starting again returns the same active session.
        assert_eq!(db.start_session(None).unwrap().id, s.id);

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
    fn normalize_name_trims_blanks_and_caps() {
        assert_eq!(normalize_name(None), None);
        assert_eq!(normalize_name(Some("")), None);
        assert_eq!(normalize_name(Some("   \t\n ")), None);
        assert_eq!(
            normalize_name(Some("  Spring retry  ")).as_deref(),
            Some("Spring retry")
        );
        let long = "é".repeat(200); // multi-byte: must cut on a char boundary
        let n = normalize_name(Some(&long)).unwrap();
        assert_eq!(n.chars().count(), MAX_SESSION_NAME);
        // A cut that lands on a space doesn't leave trailing whitespace.
        let spaced = format!("{} tail", "a".repeat(MAX_SESSION_NAME - 1));
        assert_eq!(
            normalize_name(Some(&spaced)).unwrap(),
            "a".repeat(MAX_SESSION_NAME - 1)
        );
    }

    #[test]
    fn start_session_stores_normalized_name() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(Some("  Standup notes ")).unwrap();
        assert_eq!(s.name.as_deref(), Some("Standup notes"));
        // Already running: returns it unchanged, ignoring the new name.
        assert_eq!(
            db.start_session(Some("other")).unwrap().name.as_deref(),
            Some("Standup notes")
        );
        db.stop_session().unwrap();
        assert_eq!(db.start_session(Some("   ")).unwrap().name, None);
    }

    #[test]
    fn rename_sets_changes_and_clears() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        db.stop_session().unwrap();
        let r = db
            .rename_session(s.id, Some(" Design review "))
            .unwrap()
            .unwrap();
        assert_eq!(r.name.as_deref(), Some("Design review"));
        assert_eq!(r.started_at, s.started_at); // date + time untouched
        let r = db.rename_session(s.id, Some("Retro")).unwrap().unwrap();
        assert_eq!(r.name.as_deref(), Some("Retro"));
        assert_eq!(
            db.list_sessions().unwrap()[0].name.as_deref(),
            Some("Retro")
        );
        assert_eq!(
            db.rename_session(s.id, Some("")).unwrap().unwrap().name,
            None
        );
        assert_eq!(db.rename_session(s.id, None).unwrap().unwrap().name, None);
        assert!(db.rename_session(9999, Some("x")).unwrap().is_none());
    }

    #[test]
    fn list_frames_is_ordered_and_keeps_pruned() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        db.add_frame(s.id, 20, Some("frames/1/20.jpg"), "").unwrap();
        db.add_frame(s.id, 10, None, "old").unwrap();
        let other = db
            .stop_session()
            .and_then(|_| db.start_session(None))
            .unwrap();
        db.add_frame(other.id, 5, None, "").unwrap();
        let f = db.list_frames(s.id).unwrap();
        assert_eq!(f.iter().map(|f| f.ts).collect::<Vec<_>>(), vec![10, 20]);
        assert_eq!(f[0].image_path, None);
        assert_eq!(f[1].image_path.as_deref(), Some("frames/1/20.jpg"));
    }

    #[test]
    fn fts_finds_frame_text() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        let f = db
            .add_frame(s.id, 1, None, "spring retry config maxAttempts")
            .unwrap();
        db.add_frame(s.id, 2, Some("frames/2.png"), "unrelated")
            .unwrap();
        let hits = db.search("retry").unwrap();
        assert_eq!(hits, vec![(f, s.id, 1)]);
    }

    #[test]
    fn migrates_legacy_db_and_updates_fts_on_ocr_write() {
        let path = std::env::temp_dir().join(format!(
            "locallens-mig-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(r#"
                CREATE TABLE sessions (id INTEGER PRIMARY KEY AUTOINCREMENT, started_at INTEGER NOT NULL, ended_at INTEGER);
                CREATE TABLE frames (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                    ts INTEGER NOT NULL,
                    image_path TEXT,
                    ocr_text TEXT NOT NULL DEFAULT ''
                );
                CREATE VIRTUAL TABLE frames_fts USING fts5(ocr_text, content='frames', content_rowid='id');
                CREATE TRIGGER frames_ai AFTER INSERT ON frames BEGIN
                    INSERT INTO frames_fts(rowid, ocr_text) VALUES (new.id, new.ocr_text);
                END;
                CREATE TRIGGER frames_ad AFTER DELETE ON frames BEGIN
                    INSERT INTO frames_fts(frames_fts, rowid, ocr_text) VALUES ('delete', old.id, old.ocr_text);
                END;
                CREATE TRIGGER frames_au AFTER UPDATE OF ocr_text ON frames BEGIN
                    INSERT INTO frames_fts(frames_fts, rowid, ocr_text) VALUES ('delete', old.id, old.ocr_text);
                    INSERT INTO frames_fts(rowid, ocr_text) VALUES (new.id, new.ocr_text);
                END;
                INSERT INTO sessions (started_at) VALUES (1);
                INSERT INTO frames (session_id, ts, image_path, ocr_text) VALUES (1, 10, 'frames/1/a.jpg', '');
                INSERT INTO frames (session_id, ts, image_path, ocr_text) VALUES (1, 20, NULL, '');
                INSERT INTO frames (session_id, ts, image_path, ocr_text) VALUES (1, 30, NULL, 'already indexed');
            "#).unwrap();
        }

        let db = Db::open(&path).unwrap();
        // Old sessions survive the `name` migration as unnamed, and can be named.
        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, None);
        assert_eq!(sessions[0].started_at, 1);
        assert_eq!(
            db.rename_session(1, Some("legacy"))
                .unwrap()
                .unwrap()
                .name
                .as_deref(),
            Some("legacy")
        );
        drop(db);
        // Re-opening an already-migrated db is a no-op (no "duplicate column" error).
        let db = Db::open(&path).unwrap();
        assert_eq!(
            db.list_sessions().unwrap()[0].name.as_deref(),
            Some("legacy")
        );
        let rows = db.list_frames(1).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].ocr_status, OCR_PENDING); // has image, empty text
        assert_eq!(rows[1].ocr_status, OCR_DONE); // pruned, nothing to read
        assert_eq!(rows[2].ocr_status, OCR_DONE);
        assert_eq!(rows[2].ocr_text, "already indexed");
        assert_eq!(db.search("indexed").unwrap().len(), 1);

        db.set_ocr_result(rows[0].id, "migrated hello", OCR_DONE)
            .unwrap();
        assert_eq!(db.search("hello").unwrap()[0].0, rows[0].id);
        // The UPDATE OF ocr_text trigger refreshed FTS (not a stale empty doc).
        assert!(!db.search("hello").unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
