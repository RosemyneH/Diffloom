use diffloom::db::{self, SessionRow};
use rusqlite::Connection;

#[test]
fn migrate_creates_expected_tables() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='snapshots'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_snapshots_path_created'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn meta_roundtrip_and_active_session() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    assert_eq!(db::meta_get(&conn, "k").unwrap(), None);
    db::meta_set(&conn, "k", "v").unwrap();
    assert_eq!(db::meta_get(&conn, "k").unwrap(), Some("v".into()));
    db::meta_set(&conn, "active_session_id", "42").unwrap();
    assert_eq!(db::active_session_id(&conn).unwrap(), Some(42));
}

#[test]
fn open_db_creates_sqlite_under_diffloom_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let conn = db::open_db(root).unwrap();
    drop(conn);
    assert!(root.join(".diffloom/db.sqlite").is_file());
}

#[test]
fn snapshots_queries_empty_then_rows() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    assert!(db::last_snapshot_for_path(&conn, "src/a.rs").unwrap().is_none());
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid: i64 = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
             VALUES (?1, 'p/a.rs', 0, 'abc', 3, 10, 0)",
            [sid],
        )
        .unwrap();
    let id1 = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
             VALUES (?1, 'p/a.rs', 0, 'def', 4, 20, 0)",
            [sid],
        )
        .unwrap();
    let id2 = conn.last_insert_rowid();
    assert!(id2 > id1);
    let last = db::last_snapshot_for_path(&conn, "p/a.rs").unwrap().unwrap();
    assert_eq!(last.0, id2);
    assert_eq!(last.1, "def");
    let prev = db::previous_snapshot_id(&conn, "p/a.rs", id2).unwrap();
    assert_eq!(prev, Some(id1));
    let rows = db::list_snapshots_for_session(&conn, sid, 10).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, id2);
}

#[test]
fn list_sessions_orders_newest_first() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('a', 'x', 1)",
            [],
        )
        .unwrap();
    let id_a = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('b', 'y', 2)",
            [],
        )
        .unwrap();
    let id_b = conn.last_insert_rowid();
    let rows: Vec<SessionRow> = db::list_sessions(&conn, 10).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, id_b);
    assert_eq!(rows[0].title, "b");
    assert_eq!(rows[1].id, id_a);
}

#[test]
fn snapshot_body_roundtrip() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
             VALUES (?1, 'f', 0, 'sha', 2, 0, 0)",
            [sid],
        )
        .unwrap();
    let snap = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO snapshot_bodies (snapshot_id, content) VALUES (?1, x'0102')",
            [snap],
        )
        .unwrap();
    let body = db::snapshot_body(&conn, snap).unwrap().unwrap();
    assert_eq!(body, vec![1u8, 2u8]);
}

#[test]
fn symbols_load_roundtrip() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
             VALUES (?1, 'f.rs', 0, 'sha', 1, 0, 0)",
            [sid],
        )
        .unwrap();
    let snap = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO symbols (snapshot_id, kind, name, fq_name, start_byte, end_byte)
             VALUES (?1, 'fn', 'foo', 'foo', 0, 3)",
            [snap],
        )
        .unwrap();
    let syms = db::load_symbols(&conn, snap).unwrap();
    assert_eq!(syms.len(), 1);
    assert_eq!(syms[0].name, "foo");
    assert_eq!(syms[0].fq_name.as_deref(), Some("foo"));
}

#[test]
fn snapshot_path_and_summary_helpers() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    conn
        .execute(
            "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
             VALUES (?1, 'path/x', 0, 'sha', 1, 0, 0)",
            [sid],
        )
        .unwrap();
    let snap = conn.last_insert_rowid();
    assert_eq!(
        db::snapshot_path(&conn, snap).unwrap().as_deref(),
        Some("path/x")
    );
    conn
        .execute(
            "INSERT INTO snapshot_summaries (snapshot_id, summary_text, updated_at) VALUES (?1, 'hi', 9)",
            [snap],
        )
        .unwrap();
    assert_eq!(
        db::snapshot_summary(&conn, snap).unwrap().as_deref(),
        Some("hi")
    );
}

#[test]
fn list_snapshots_for_path_orders_newest_first() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    for (sha, ts) in [("aaa", 1i64), ("bbb", 2), ("ccc", 3)] {
        conn.execute(
            "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
             VALUES (?1, 'src/x.rs', 0, ?2, 1, ?3, 0)",
            rusqlite::params![sid, sha, ts],
        )
        .unwrap();
    }
    let rows = db::list_snapshots_for_path(&conn, "src/x.rs", 10).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].content_sha256, "ccc");
    assert_eq!(rows[2].content_sha256, "aaa");
}

#[test]
fn list_commit_groups_dedupes_by_commit() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_commit, git_dirty)
         VALUES (?1, 'a.rs', 0, 's1', 1, 1, 'deadbeef', 0)",
        [sid],
    )
    .unwrap();
    let id1 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_commit, git_dirty)
         VALUES (?1, 'b.rs', 0, 's2', 1, 2, 'deadbeef', 0)",
        [sid],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_commit, git_dirty)
         VALUES (?1, 'c.rs', 0, 's3', 1, 3, 'cafebabe', 0)",
        [sid],
    )
    .unwrap();
    let groups = db::list_commit_groups(&conn, 10).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].git_commit, "cafebabe");
    assert_eq!(groups[0].snapshot_id, id1 + 2);
    assert_eq!(groups[1].git_commit, "deadbeef");
    assert_eq!(groups[1].snapshot_id, id1 + 1);
}

#[test]
fn list_paths_by_scope_requires_two_snapshots_when_all_sessions() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
         VALUES (?1, 'only.rs', 0, 'a', 1, 1, 0)",
        [sid],
    )
    .unwrap();
    let paths = db::list_paths_by_scope(&conn, None, 20).unwrap();
    assert!(paths.is_empty(), "single snapshot path should not list globally");

    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
         VALUES (?1, 'only.rs', 0, 'b', 1, 2, 0)",
        [sid],
    )
    .unwrap();
    let paths = db::list_paths_by_scope(&conn, None, 20).unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].path, "only.rs");
    assert_eq!(paths[0].snapshot_count, 2);
}

#[test]
fn list_paths_by_scope_session_lists_single_snapshot_paths() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn
        .execute(
            "INSERT INTO sessions (title, kind, created_at) VALUES ('t', 'k', 1)",
            [],
        )
        .unwrap();
    let sid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
         VALUES (?1, 'one.rs', 0, 'a', 1, 1, 0)",
        [sid],
    )
    .unwrap();
    let paths = db::list_paths_by_scope(&conn, Some(sid), 20).unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].snapshot_count, 1);
}
