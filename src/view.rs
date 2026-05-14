use rusqlite::Connection;

use crate::db;

pub fn snapshot_detail(
    conn: &Connection,
    snaps: &[db::SnapshotListRow],
    sel: Option<usize>,
) -> String {
    let Some(i) = sel else {
        return String::new();
    };
    let Some(s) = snaps.get(i) else {
        return String::new();
    };
    let sum = db::snapshot_summary(conn, s.id).unwrap_or(None);
    let changes = db::load_symbol_changes(conn, s.id).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!(
        "id={} path={}\nsha={}\n\n",
        s.id, s.path, s.content_sha256
    ));
    if let Some(t) = sum {
        out.push_str("Summary:\n");
        out.push_str(&t);
        out.push('\n');
    }
    if !changes.is_empty() {
        out.push_str("\nSymbols:\n");
        for (c, n, k) in changes {
            out.push_str(&format!("  {c} {k} {n}\n"));
        }
    }
    out
}
