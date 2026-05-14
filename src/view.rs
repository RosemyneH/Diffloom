use rusqlite::Connection;
use similar::TextDiff;

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

pub fn unified_diff_for_snapshot(conn: &Connection, snapshot_id: i64) -> anyhow::Result<String> {
    let path = db::snapshot_path(conn, snapshot_id)?
        .ok_or_else(|| anyhow::anyhow!("snapshot not found"))?;
    let prev = match db::previous_snapshot_id(conn, &path, snapshot_id)? {
        Some(p) => p,
        None => return Ok("first snapshot for this path — nothing to compare\n".into()),
    };
    let cur = db::snapshot_body(conn, snapshot_id)?
        .ok_or_else(|| anyhow::anyhow!("missing stored body for current snapshot"))?;
    let old = db::snapshot_body(conn, prev)?
        .ok_or_else(|| anyhow::anyhow!("missing stored body for previous snapshot"))?;
    let old_s = String::from_utf8_lossy(&old);
    let new_s = String::from_utf8_lossy(&cur);
    let diff = TextDiff::from_lines(&*old_s, &*new_s);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
            similar::ChangeTag::Equal => " ",
        };
        for line in change.value().lines() {
            out.push_str(sign);
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        Ok("(no line changes)\n".into())
    } else {
        Ok(out)
    }
}

pub fn snapshot_summary_only(conn: &Connection, snapshot_id: i64) -> anyhow::Result<Option<String>> {
    db::snapshot_summary(conn, snapshot_id).map_err(Into::into)
}
