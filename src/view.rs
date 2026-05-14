use rusqlite::Connection;
use similar::{DiffOp, TextDiff};

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
    let Some((old_s, new_s)) = snapshot_old_new_strings(conn, snapshot_id)? else {
        return Ok("first snapshot for this path — nothing to compare\n".into());
    };
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

pub fn snapshot_old_new_strings(
    conn: &Connection,
    snapshot_id: i64,
) -> anyhow::Result<Option<(String, String)>> {
    let path = match db::snapshot_path(conn, snapshot_id)? {
        Some(p) => p,
        None => return Ok(None),
    };
    let prev = match db::previous_snapshot_id(conn, &path, snapshot_id)? {
        Some(p) => p,
        None => return Ok(None),
    };
    let cur = match db::snapshot_body(conn, snapshot_id)? {
        Some(b) => b,
        None => return Ok(None),
    };
    let old = match db::snapshot_body(conn, prev)? {
        Some(b) => b,
        None => return Ok(None),
    };
    Ok(Some((
        String::from_utf8_lossy(&old).into_owned(),
        String::from_utf8_lossy(&cur).into_owned(),
    )))
}

#[derive(Clone, Copy, Default)]
pub struct DiffLineStats {
    pub insertions: u32,
    pub deletions: u32,
}

pub fn count_line_changes(old: &str, new: &str) -> DiffLineStats {
    let diff = TextDiff::from_lines(old, new);
    let mut insertions = 0u32;
    let mut deletions = 0u32;
    for op in diff.ops() {
        match *op {
            DiffOp::Delete { old_len, .. } => deletions += old_len as u32,
            DiffOp::Insert { new_len, .. } => insertions += new_len as u32,
            DiffOp::Replace {
                old_len, new_len, ..
            } => {
                deletions += old_len as u32;
                insertions += new_len as u32;
            }
            _ => {}
        }
    }
    DiffLineStats {
        insertions,
        deletions,
    }
}

#[derive(Clone)]
pub enum SbsRow {
    Equal {
        old_ln: usize,
        new_ln: usize,
        left: String,
        right: String,
    },
    DeleteLine {
        old_ln: usize,
        text: String,
    },
    InsertLine {
        new_ln: usize,
        text: String,
    },
    Both {
        old_ln: usize,
        new_ln: usize,
        left: String,
        right: String,
    },
}

pub fn side_by_side_rows(old: &str, new: &str) -> (Vec<SbsRow>, DiffLineStats) {
    let stats = count_line_changes(old, new);
    let diff = TextDiff::from_lines(old, new);
    let old_slices = diff.old_slices();
    let new_slices = diff.new_slices();
    let mut rows = Vec::new();
    let mut old_ln = 1usize;
    let mut new_ln = 1usize;
    for op in diff.ops() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for i in 0..len {
                    rows.push(SbsRow::Equal {
                        old_ln: old_ln + i,
                        new_ln: new_ln + i,
                        left: old_slices[old_index + i].to_string(),
                        right: new_slices[new_index + i].to_string(),
                    });
                }
                old_ln += len;
                new_ln += len;
            }
            DiffOp::Delete {
                old_index,
                old_len,
                ..
            } => {
                for i in 0..old_len {
                    rows.push(SbsRow::DeleteLine {
                        old_ln: old_ln + i,
                        text: old_slices[old_index + i].to_string(),
                    });
                }
                old_ln += old_len;
            }
            DiffOp::Insert {
                new_index,
                new_len,
                ..
            } => {
                for i in 0..new_len {
                    rows.push(SbsRow::InsertLine {
                        new_ln: new_ln + i,
                        text: new_slices[new_index + i].to_string(),
                    });
                }
                new_ln += new_len;
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let m = old_len.min(new_len);
                for i in 0..m {
                    rows.push(SbsRow::Both {
                        old_ln: old_ln + i,
                        new_ln: new_ln + i,
                        left: old_slices[old_index + i].to_string(),
                        right: new_slices[new_index + i].to_string(),
                    });
                }
                for i in m..old_len {
                    rows.push(SbsRow::DeleteLine {
                        old_ln: old_ln + i,
                        text: old_slices[old_index + i].to_string(),
                    });
                }
                for i in m..new_len {
                    rows.push(SbsRow::InsertLine {
                        new_ln: new_ln + i,
                        text: new_slices[new_index + i].to_string(),
                    });
                }
                old_ln += old_len;
                new_ln += new_len;
            }
        }
    }
    (rows, stats)
}
