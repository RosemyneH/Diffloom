use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

pub const MAX_STORED_BODY: usize = 512 * 1024;

pub fn configure(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        ",
    )
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            kind TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            closed_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER REFERENCES sessions(id),
            path TEXT NOT NULL,
            mtime_ns INTEGER,
            content_sha256 TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            git_commit TEXT,
            git_dirty INTEGER NOT NULL DEFAULT 0,
            parse_error TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(id)
        );

        CREATE INDEX IF NOT EXISTS idx_snapshots_path_created
            ON snapshots(path COLLATE NOCASE, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_snapshots_session_created
            ON snapshots(session_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS snapshot_summaries (
            snapshot_id INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
            summary_text TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS snapshot_bodies (
            snapshot_id INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
            content BLOB NOT NULL
        );

        CREATE TABLE IF NOT EXISTS symbols (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            fq_name TEXT,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_symbols_snapshot ON symbols(snapshot_id);

        CREATE TABLE IF NOT EXISTS symbol_changes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
            change TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            prev_snapshot_id INTEGER REFERENCES snapshots(id)
        );
        CREATE INDEX IF NOT EXISTS idx_symbol_changes_snapshot ON symbol_changes(snapshot_id);

        CREATE TABLE IF NOT EXISTS snapshot_llm_reviews (
            snapshot_id INTEGER PRIMARY KEY REFERENCES snapshots(id) ON DELETE CASCADE,
            model TEXT NOT NULL,
            body TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        ",
    )?;
    Ok(())
}

pub fn meta_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn read_paths_load(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let Some(raw) = meta_get(conn, "read_paths_json")? else {
        return Ok(HashSet::new());
    };
    let v: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    Ok(v.into_iter().collect())
}

pub fn read_paths_save(conn: &Connection, paths: &HashSet<String>) -> rusqlite::Result<()> {
    let mut v: Vec<String> = paths.iter().cloned().collect();
    v.sort();
    let raw = serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_string());
    meta_set(conn, "read_paths_json", &raw)
}

pub fn active_session_id(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    Ok(meta_get(conn, "active_session_id")?.and_then(|s| s.parse().ok()))
}

pub fn last_snapshot_for_path(
    conn: &Connection,
    path: &str,
) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT id, content_sha256 FROM snapshots WHERE path = ?1 ORDER BY id DESC LIMIT 1",
        [path],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

pub fn previous_snapshot_id(
    conn: &Connection,
    path: &str,
    before_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM snapshots WHERE path = ?1 AND id < ?2 ORDER BY id DESC LIMIT 1",
        params![path, before_id],
        |r| r.get(0),
    )
    .optional()
}

pub fn snapshot_body(conn: &Connection, snapshot_id: i64) -> rusqlite::Result<Option<Vec<u8>>> {
    conn.query_row(
        "SELECT content FROM snapshot_bodies WHERE snapshot_id = ?1",
        [snapshot_id],
        |r| r.get::<_, Vec<u8>>(0),
    )
    .optional()
}

pub fn list_sessions(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, kind, created_at, closed_at FROM sessions
         ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(SessionRow {
            id: r.get(0)?,
            title: r.get(1)?,
            kind: r.get(2)?,
            created_at: r.get(3)?,
            closed_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn list_snapshots_for_session(
    conn: &Connection,
    session_id: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SnapshotListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, content_sha256, created_at FROM snapshots
         WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![session_id, limit], |r| {
        Ok(SnapshotListRow {
            id: r.get(0)?,
            path: r.get(1)?,
            content_sha256: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn list_recent_snapshots(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<SnapshotListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, content_sha256, created_at FROM snapshots
         ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(SnapshotListRow {
            id: r.get(0)?,
            path: r.get(1)?,
            content_sha256: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    rows.collect()
}

#[derive(Debug, serde::Serialize)]
pub struct SessionRow {
    pub id: i64,
    pub title: String,
    pub kind: String,
    pub created_at: i64,
    pub closed_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SnapshotListRow {
    pub id: i64,
    pub path: String,
    pub content_sha256: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct CommitGroupRow {
    pub git_commit: String,
    pub snapshot_id: i64,
    pub created_at: i64,
}

pub fn list_snapshots_for_path(
    conn: &Connection,
    path: &str,
    limit: i64,
) -> rusqlite::Result<Vec<SnapshotListRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, content_sha256, created_at FROM snapshots
         WHERE path = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![path, limit], |r| {
        Ok(SnapshotListRow {
            id: r.get(0)?,
            path: r.get(1)?,
            content_sha256: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone)]
pub struct PathHistoryRow {
    pub path: String,
    pub latest_id: i64,
    pub snapshot_count: i64,
}

pub fn list_paths_by_scope(
    conn: &Connection,
    session_id: Option<i64>,
    limit: i64,
) -> rusqlite::Result<Vec<PathHistoryRow>> {
    match session_id {
        None => {
            let mut stmt = conn.prepare(
                "SELECT path, MAX(id) AS lid, COUNT(*) AS n
                 FROM snapshots
                 GROUP BY path
                 ORDER BY lid DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit], |r| {
                Ok(PathHistoryRow {
                    path: r.get(0)?,
                    latest_id: r.get(1)?,
                    snapshot_count: r.get(2)?,
                })
            })?;
            rows.collect()
        }
        Some(sid) => {
            let mut stmt = conn.prepare(
                "SELECT path, MAX(id) AS lid, COUNT(*) AS n
                 FROM snapshots
                 WHERE session_id = ?1
                 GROUP BY path
                 ORDER BY lid DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![sid, limit], |r| {
                Ok(PathHistoryRow {
                    path: r.get(0)?,
                    latest_id: r.get(1)?,
                    snapshot_count: r.get(2)?,
                })
            })?;
            rows.collect()
        }
    }
}

pub fn list_commit_groups(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<CommitGroupRow>> {
    let mut stmt = conn.prepare(
        "SELECT s.git_commit, s.id, s.created_at
         FROM snapshots s
         INNER JOIN (
             SELECT git_commit AS gc, MAX(id) AS mid FROM snapshots
             WHERE git_commit IS NOT NULL AND TRIM(git_commit) != ''
             GROUP BY git_commit
         ) g ON s.git_commit = g.gc AND s.id = g.mid
         ORDER BY s.id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(CommitGroupRow {
            git_commit: r.get(0)?,
            snapshot_id: r.get(1)?,
            created_at: r.get(2)?,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone)]
pub struct SymbolRow {
    pub kind: String,
    pub name: String,
    pub fq_name: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub fn load_symbols(conn: &Connection, snapshot_id: i64) -> rusqlite::Result<Vec<SymbolRow>> {
    let mut stmt = conn.prepare(
        "SELECT kind, name, fq_name, start_byte, end_byte FROM symbols WHERE snapshot_id = ?1",
    )?;
    let rows = stmt.query_map([snapshot_id], |r| {
        Ok(SymbolRow {
            kind: r.get(0)?,
            name: r.get(1)?,
            fq_name: r.get(2)?,
            start_byte: r.get::<_, i64>(3)? as usize,
            end_byte: r.get::<_, i64>(4)? as usize,
        })
    })?;
    rows.collect()
}

pub fn load_symbol_changes(
    conn: &Connection,
    snapshot_id: i64,
) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT change, name, kind FROM symbol_changes WHERE snapshot_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([snapshot_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

pub fn snapshot_path(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT path FROM snapshots WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .optional()
}

pub fn snapshot_summary(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT summary_text FROM snapshot_summaries WHERE snapshot_id = ?1",
        [id],
        |r| r.get(0),
    )
    .optional()
}

pub fn llm_review_get(
    conn: &Connection,
    snapshot_id: i64,
) -> rusqlite::Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT model, body FROM snapshot_llm_reviews WHERE snapshot_id = ?1",
        [snapshot_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

pub fn llm_review_save(
    conn: &Connection,
    snapshot_id: i64,
    model: &str,
    body: &str,
    updated_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO snapshot_llm_reviews (snapshot_id, model, body, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(snapshot_id) DO UPDATE SET
           model = excluded.model,
           body = excluded.body,
           updated_at = excluded.updated_at",
        params![snapshot_id, model, body, updated_at],
    )?;
    Ok(())
}

pub fn open_db(workspace_root: &Path) -> anyhow::Result<Connection> {
    let dir = workspace_root.join(".diffloom");
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("db.sqlite");
    let mut conn = Connection::open(db_path)?;
    configure(&mut conn)?;
    migrate(&conn)?;
    Ok(conn)
}
