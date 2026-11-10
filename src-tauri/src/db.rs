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

/// Results per search when the caller doesn't say, and the most we ever return.
pub const SEARCH_DEFAULT_LIMIT: u32 = 30;
pub const SEARCH_MAX_LIMIT: u32 = 100;

// FTS5's snippet() wraps every hit in these. They're Unicode private-use code points, so
// real OCR text practically never contains them, and they never reach the UI as markup:
// `parse_snippet` turns them into structured parts that React renders as plain text.
const HIT_START: char = '\u{E000}';
const HIT_END: char = '\u{E001}';

/// Turn whatever the user typed into a safe FTS5 MATCH expression, or None if there's
/// nothing to search for. Every word becomes a quoted string (inner `"` doubled), so
/// `-`, `:`, `*`, `(`, `AND`, `OR`, `NEAR` are matched as plain text, never parsed as
/// query syntax. Words are implicitly AND-ed, and the last one gets `*` so a half-typed
/// word ("deplo") matches its completions ("deploy"). Pure punctuation is dropped,
/// since the tokenizer would ignore it anyway.
pub fn build_fts_query(input: &str) -> Option<String> {
    let tokens: Vec<String> = input
        .split_whitespace()
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    Some(format!("{}*", tokens.join(" ")))
}

/// One run of snippet text; `hit` = this run matched the query (render as `<mark>`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SnippetPart {
    pub text: String,
    pub hit: bool,
}

/// Split a raw FTS5 snippet at the HIT_START / HIT_END sentinels.
pub fn parse_snippet(raw: &str) -> Vec<SnippetPart> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut hit = false;
    for c in raw.chars() {
        let next = match c {
            HIT_START => true,
            HIT_END => false,
            _ => {
                buf.push(c);
                continue;
            }
        };
        if !buf.is_empty() {
            parts.push(SnippetPart {
                text: std::mem::take(&mut buf),
                hit,
            });
        }
        hit = next;
    }
    if !buf.is_empty() {
        parts.push(SnippetPart { text: buf, hit });
    }
    parts
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub frame_id: i64,
    pub session_id: i64,
    pub session_name: Option<String>,
    pub session_started_at: i64,
    pub ts: i64,
    /// Relative here; the `search` command makes it absolute. None once pruned.
    pub image_path: Option<String>,
    pub snippet: Vec<SnippetPart>,
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

    /// Full-text search over OCR text, best match first.
    /// `bm25()` is a relevance score where *lower is better* (rarer words, more hits and
    /// shorter text score lower), so it sorts ascending; ties go to the newer frame.
    pub fn search(&self, query: &str, limit: Option<u32>) -> rusqlite::Result<Vec<SearchHit>> {
        let Some(fts) = build_fts_query(query) else {
            return Ok(Vec::new());
        };
        let limit = limit
            .unwrap_or(SEARCH_DEFAULT_LIMIT)
            .clamp(1, SEARCH_MAX_LIMIT);
        let sql = format!(
            "SELECT f.id, f.session_id, s.name, s.started_at, f.ts, f.image_path,
                    snippet(frames_fts, 0, '{HIT_START}', '{HIT_END}', '…', 12)
             FROM frames_fts
             JOIN frames f ON f.id = frames_fts.rowid
             JOIN sessions s ON s.id = f.session_id
             WHERE frames_fts MATCH ?1 AND f.ocr_status = ?2 AND f.ocr_text != ''
             ORDER BY bm25(frames_fts) ASC, f.ts DESC
             LIMIT ?3"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![fts, OCR_DONE, limit], |r| {
            Ok(SearchHit {
                frame_id: r.get(0)?,
                session_id: r.get(1)?,
                session_name: r.get(2)?,
                session_started_at: r.get(3)?,
                ts: r.get(4)?,
                image_path: r.get(5)?,
                snippet: parse_snippet(&r.get::<_, String>(6)?),
            })
        })?;
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
        let hits = db.search("retry", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].frame_id, hits[0].session_id, hits[0].ts),
            (f, s.id, 1)
        );
    }

    #[test]
    fn build_fts_query_quotes_every_token() {
        assert_eq!(build_fts_query(""), None);
        assert_eq!(build_fts_query("   \t\n"), None);
        assert_eq!(build_fts_query("- : * ()"), None); // nothing searchable
        assert_eq!(build_fts_query("deplo").as_deref(), Some(r#""deplo"*"#));
        assert_eq!(
            build_fts_query("  spring   retry ").as_deref(),
            Some(r#""spring" "retry"*"#)
        );
        // Operators and syntax characters become literal text.
        assert_eq!(
            build_fts_query("foo AND bar OR NEAR(x) -baz col:val").as_deref(),
            Some(r#""foo" "AND" "bar" "OR" "NEAR(x)" "-baz" "col:val"*"#)
        );
        assert_eq!(
            build_fts_query("pre-commit").as_deref(),
            Some(r#""pre-commit"*"#)
        );
        // Inner quotes are doubled, so a stray `"` can't end the string early.
        assert_eq!(
            build_fts_query(r#"say "hi" o"k"#).as_deref(),
            Some(r#""say" """hi""" "o""k"*"#)
        );
        assert_eq!(
            build_fts_query("Grüße नमस्ते 東京").as_deref(),
            Some(r#""Grüße" "नमस्ते" "東京"*"#)
        );
    }

    #[test]
    fn parse_snippet_splits_on_markers() {
        let raw = format!("a {HIT_START}deploy{HIT_END} b <i>{HIT_START}x{HIT_END}");
        let parts = parse_snippet(&raw);
        let got: Vec<_> = parts.iter().map(|p| (p.text.as_str(), p.hit)).collect();
        assert_eq!(
            got,
            vec![
                ("a ", false),
                ("deploy", true),
                (" b <i>", false),
                ("x", true)
            ]
        );
        assert!(parse_snippet("").is_empty());
    }

    #[test]
    fn search_ranks_denser_shorter_matches_first() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        let long = db
            .add_frame(
                s.id,
                1,
                None,
                "a long page of notes where kubernetes is mentioned once among many many other words about lunch",
            )
            .unwrap();
        let dense = db
            .add_frame(s.id, 2, None, "kubernetes kubernetes kubernetes")
            .unwrap();
        db.add_frame(s.id, 3, None, "nothing relevant here")
            .unwrap();
        let ids: Vec<_> = db
            .search("kubernetes", None)
            .unwrap()
            .iter()
            .map(|h| h.frame_id)
            .collect();
        assert_eq!(ids, vec![dense, long]);
    }

    #[test]
    fn search_prefix_multiword_and_snippet() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(Some("Release day")).unwrap();
        let f = db
            .add_frame(
                s.id,
                7,
                Some("frames/1/7.jpg"),
                "run the deploy script <b>now</b> & check logs",
            )
            .unwrap();
        db.add_frame(s.id, 8, None, "deploy only").unwrap();

        let hits = db.search("deplo", None).unwrap(); // half-typed word
        assert_eq!(hits.len(), 2);
        let hits = db.search("script deplo", None).unwrap(); // words AND-ed, any order
        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        assert_eq!(h.frame_id, f);
        assert_eq!(h.session_id, s.id);
        assert_eq!(h.session_name.as_deref(), Some("Release day"));
        assert_eq!(h.session_started_at, s.started_at);
        assert_eq!(h.ts, 7);
        assert_eq!(h.image_path.as_deref(), Some("frames/1/7.jpg"));
        let marked: Vec<_> = h
            .snippet
            .iter()
            .filter(|p| p.hit)
            .map(|p| p.text.as_str())
            .collect();
        assert_eq!(marked, vec!["deploy", "script"]);
        // Raw `<b>` from OCR stays plain text inside a non-hit part, no sentinels leak.
        let all: String = h.snippet.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(all, "run the deploy script <b>now</b> & check logs");
        assert!(!all.contains(HIT_START) && !all.contains(HIT_END));
    }

    #[test]
    fn search_returns_pruned_frames_and_skips_unfinished_ocr() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        let pruned = db.add_frame(s.id, 1, None, "invoice total 420").unwrap();
        let pending = db.add_frame(s.id, 2, Some("frames/1/2.jpg"), "").unwrap();
        let failed = db.add_frame(s.id, 3, Some("frames/1/3.jpg"), "").unwrap();
        db.set_ocr_result(failed, "invoice draft", OCR_FAILED)
            .unwrap();
        // Text written but status still pending (shouldn't happen, but don't show it).
        db.conn
            .execute(
                "UPDATE frames SET ocr_text = 'invoice pending' WHERE id = ?1",
                [pending],
            )
            .unwrap();

        let hits = db.search("invoice", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].frame_id, pruned);
        assert_eq!(hits[0].image_path, None);
        assert_eq!(hits[0].session_name, None);
    }

    #[test]
    fn search_limits_and_blank_and_hostile_input() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        for ts in 0..120 {
            db.add_frame(s.id, ts, None, "same words every time")
                .unwrap();
        }
        assert_eq!(
            db.search("words", None).unwrap().len(),
            SEARCH_DEFAULT_LIMIT as usize
        );
        assert_eq!(db.search("words", Some(5)).unwrap().len(), 5);
        assert_eq!(db.search("words", Some(0)).unwrap().len(), 1);
        assert_eq!(
            db.search("words", Some(10_000)).unwrap().len(),
            SEARCH_MAX_LIMIT as usize
        );
        // Equal scores: newest first.
        assert_eq!(db.search("words", Some(1)).unwrap()[0].ts, 119);

        assert!(db.search("", None).unwrap().is_empty());
        assert!(db.search("   ", None).unwrap().is_empty());
        for nasty in [
            "\"",
            "\"\"\"",
            "words\"",
            "*",
            "words*",
            "AND",
            "OR words",
            "NOT words",
            "NEAR(words every)",
            "words -time",
            "ocr_text:words",
            "^words",
            "{ocr_text}: words",
            "(words",
            "'; DROP TABLE frames; --",
            "+",
            "\u{E000}words\u{E001}",
        ] {
            assert!(
                db.search(nasty, None).is_ok(),
                "query {nasty:?} should not error"
            );
        }
        // Still intact, and syntax-looking input is matched literally.
        assert_eq!(db.search("ocr_text:words", None).unwrap().len(), 0);
        assert_eq!(db.search("words", Some(100)).unwrap().len(), 100);
    }

    #[test]
    fn search_unicode() {
        let db = Db::open_in_memory().unwrap();
        let s = db.start_session(None).unwrap();
        let f = db
            .add_frame(s.id, 1, None, "Viele Grüße aus München")
            .unwrap();
        assert_eq!(db.search("grüße", None).unwrap()[0].frame_id, f);
        assert_eq!(db.search("MÜNCH", None).unwrap()[0].frame_id, f);
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
        assert_eq!(db.search("indexed", None).unwrap().len(), 1);

        db.set_ocr_result(rows[0].id, "migrated hello", OCR_DONE)
            .unwrap();
        assert_eq!(db.search("hello", None).unwrap()[0].frame_id, rows[0].id);
        // The UPDATE OF ocr_text trigger refreshed FTS (not a stale empty doc).
        assert!(!db.search("hello", None).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
