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
        return Ok("(diff unavailable — missing stored file contents for this snapshot)\n".into());
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
    let cur = match db::snapshot_body(conn, snapshot_id)? {
        Some(b) => b,
        None => return Ok(None),
    };
    let cur_s = String::from_utf8_lossy(&cur).into_owned();
    let prev = match db::previous_snapshot_id(conn, &path, snapshot_id)? {
        Some(p) => p,
        None => return Ok(Some((String::new(), cur_s))),
    };
    let old = match db::snapshot_body(conn, prev)? {
        Some(b) => b,
        None => return Ok(None),
    };
    Ok(Some((String::from_utf8_lossy(&old).into_owned(), cur_s)))
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
    Skipped {
        unchanged: usize,
    },
}

pub fn side_by_side_rows(old: &str, new: &str) -> (Vec<SbsRow>, DiffLineStats) {
    let stats = count_line_changes(old, new);
    let diff = TextDiff::from_lines(old, new);
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
                        left: diff.old_slice(old_index + i).expect("old").to_string(),
                        right: diff.new_slice(new_index + i).expect("new").to_string(),
                    });
                }
                old_ln += len;
                new_ln += len;
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for i in 0..old_len {
                    rows.push(SbsRow::DeleteLine {
                        old_ln: old_ln + i,
                        text: diff.old_slice(old_index + i).expect("old").to_string(),
                    });
                }
                old_ln += old_len;
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for i in 0..new_len {
                    rows.push(SbsRow::InsertLine {
                        new_ln: new_ln + i,
                        text: diff.new_slice(new_index + i).expect("new").to_string(),
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
                        left: diff.old_slice(old_index + i).expect("old").to_string(),
                        right: diff.new_slice(new_index + i).expect("new").to_string(),
                    });
                }
                for i in m..old_len {
                    rows.push(SbsRow::DeleteLine {
                        old_ln: old_ln + i,
                        text: diff.old_slice(old_index + i).expect("old").to_string(),
                    });
                }
                for i in m..new_len {
                    rows.push(SbsRow::InsertLine {
                        new_ln: new_ln + i,
                        text: diff.new_slice(new_index + i).expect("new").to_string(),
                    });
                }
                old_ln += old_len;
                new_ln += new_len;
            }
        }
    }
    (rows, stats)
}

fn is_sbs_equal(row: &SbsRow) -> bool {
    matches!(row, SbsRow::Equal { .. })
}

fn trim_all_equal_block(rows: Vec<SbsRow>, ctx: usize) -> Vec<SbsRow> {
    let n = rows.len();
    if n <= 2 * ctx + 1 {
        return rows;
    }
    let mut out = rows[..ctx].to_vec();
    out.push(SbsRow::Skipped {
        unchanged: n - 2 * ctx,
    });
    out.extend(rows[n - ctx..].iter().cloned());
    out
}

enum EqualRunPlace {
    BeforeFirstChange,
    Between,
    AfterLastChange,
}

fn take_equal_run(run: &[SbsRow], ctx: usize, place: EqualRunPlace) -> Vec<SbsRow> {
    if run.is_empty() {
        return vec![];
    }
    let n = run.len();
    match place {
        EqualRunPlace::BeforeFirstChange => {
            if n <= ctx {
                run.to_vec()
            } else {
                run[n - ctx..].to_vec()
            }
        }
        EqualRunPlace::AfterLastChange => {
            if n <= ctx {
                run.to_vec()
            } else {
                run[..ctx].to_vec()
            }
        }
        EqualRunPlace::Between => {
            if n <= 2 * ctx {
                run.to_vec()
            } else {
                let mut v = run[..ctx].to_vec();
                v.push(SbsRow::Skipped {
                    unchanged: n - 2 * ctx,
                });
                v.extend(run[n - ctx..].iter().cloned());
                v
            }
        }
    }
}

pub fn collapse_sbs_context(rows: Vec<SbsRow>, ctx: usize) -> Vec<SbsRow> {
    if rows.is_empty() || ctx == 0 {
        return rows;
    }
    let n = rows.len();
    let change_ix: Vec<usize> = (0..n).filter(|&i| !is_sbs_equal(&rows[i])).collect();
    if change_ix.is_empty() {
        return trim_all_equal_block(rows, ctx);
    }
    let mut out = Vec::new();
    let mut seg_start = 0usize;
    for (ci, &ix) in change_ix.iter().enumerate() {
        let place = if ci == 0 {
            EqualRunPlace::BeforeFirstChange
        } else {
            EqualRunPlace::Between
        };
        let seg = &rows[seg_start..ix];
        if !seg.is_empty() && seg.iter().all(is_sbs_equal) {
            out.extend(take_equal_run(seg, ctx, place));
        } else if !seg.is_empty() {
            out.extend_from_slice(seg);
        }
        out.push(rows[ix].clone());
        seg_start = ix + 1;
    }
    let tail = &rows[seg_start..n];
    if !tail.is_empty() && tail.iter().all(is_sbs_equal) {
        out.extend(take_equal_run(tail, ctx, EqualRunPlace::AfterLastChange));
    } else if !tail.is_empty() {
        out.extend_from_slice(tail);
    }
    out
}

pub fn side_by_side_rows_focused(
    old: &str,
    new: &str,
    context_lines: usize,
) -> (Vec<SbsRow>, DiffLineStats) {
    let (rows, stats) = side_by_side_rows(old, new);
    if context_lines == 0 {
        (rows, stats)
    } else {
        (collapse_sbs_context(rows, context_lines), stats)
    }
}
