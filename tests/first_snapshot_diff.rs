use diffloom::{db, view};
use rusqlite::Connection;

#[test]
fn first_snapshot_diffs_against_empty_file() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure(&mut conn).unwrap();
    db::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_dirty)
         VALUES (NULL, 'src/new.rs', 0, 'deadbeef', 12, 1, 0)",
        [],
    )
    .unwrap();
    let sid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO snapshot_bodies (snapshot_id, content) VALUES (?1, ?2)",
        rusqlite::params![sid, b"fn main() {}\n"],
    )
    .unwrap();

    let (old, new) = view::snapshot_old_new_strings(&conn, sid).unwrap().unwrap();
    assert!(old.is_empty());
    assert_eq!(new, "fn main() {}\n");

    let u = view::unified_diff_for_snapshot(&conn, sid).unwrap();
    assert!(
        u.contains('+'),
        "unified diff should show added lines: {u:?}"
    );
}
