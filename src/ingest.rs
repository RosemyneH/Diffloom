use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::params;
use sha2::{Digest, Sha256};

use crate::db::{self, SymbolRow};
use crate::git_info;
use crate::paths::{rel_under_root, should_skip_watch};
use crate::rust_parse::{self, SymChange, SymbolRec};
use crate::view;

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn ingest_path(
    conn: &mut rusqlite::Connection,
    root: &Path,
    abs_path: &Path,
) -> anyhow::Result<bool> {
    let rel = rel_under_root(root, abs_path).context("path outside workspace")?;
    if should_skip_watch(&rel) {
        return Ok(false);
    }
    if !abs_path.is_file() {
        return Ok(false);
    }
    let path_str = rel.to_string_lossy().replace('\\', "/");
    let bytes = std::fs::read(abs_path).with_context(|| format!("read {}", abs_path.display()))?;
    let size = bytes.len() as i64;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha = hex::encode(hasher.finalize());
    if let Some((_, prev_sha)) = db::last_snapshot_for_path(conn, &path_str)? {
        if prev_sha == sha {
            return Ok(false);
        }
    }
    let meta = std::fs::metadata(abs_path)?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let (git_commit, git_dirty) = git_info::repo_head_and_dirty(root)?;
    let session_id = db::active_session_id(conn)?;
    let now_ms = now_unix_ms();
    let prev_id = db::last_snapshot_for_path(conn, &path_str)?.map(|(id, _)| id);
    let prev_symbols: Vec<SymbolRow> = if let Some(pid) = prev_id {
        db::load_symbols(conn, pid)?
    } else {
        Vec::new()
    };
    let is_rs = path_str.ends_with(".rs");
    let (symbols, parse_error): (Vec<SymbolRec>, Option<String>) = if is_rs {
        let src = String::from_utf8_lossy(&bytes);
        match rust_parse::parse_rust_symbols(&src) {
            Ok((syms, pe)) => (syms, pe),
            Err(e) => (Vec::new(), Some(format!("{e:#}"))),
        }
    } else {
        (Vec::new(), None)
    };
    let changes = rust_parse::diff_symbol_maps(&prev_symbols, &symbols);
    let file_line = if is_rs {
        format!("{} — {}", path_str, rust_parse::summarize_changes(&changes))
    } else {
        let hint = match prev_id {
            None => "first snapshot for this path".to_string(),
            Some(pid) => match db::snapshot_body(conn, pid)? {
                Some(prev_bytes) => {
                    let old_s = String::from_utf8_lossy(&prev_bytes);
                    let new_s = String::from_utf8_lossy(&bytes);
                    let st = view::count_line_changes(&old_s, &new_s);
                    if st.insertions == 0 && st.deletions == 0 {
                        "no line deltas vs previous snapshot".to_string()
                    } else {
                        format!("lines +{} -{}", st.insertions, st.deletions)
                    }
                }
                None => "previous snapshot body not stored — line delta unavailable in summary"
                    .to_string(),
            },
        };
        format!("{} — {}", path_str, hint)
    };
    let mut summary_line = file_line;
    if let Some(git_txt) =
        git_info::format_snapshot_git_context(root, git_commit.as_deref(), git_dirty)
    {
        summary_line.push_str("\n\n");
        summary_line.push_str(&git_txt);
    }
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO snapshots (session_id, path, mtime_ns, content_sha256, size_bytes, created_at, git_commit, git_dirty, parse_error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            session_id,
            path_str,
            mtime_ns,
            sha,
            size,
            now_ms,
            git_commit,
            git_dirty as i64,
            parse_error,
        ],
    )?;
    let snapshot_id = tx.last_insert_rowid();
    if bytes.len() <= db::MAX_STORED_BODY {
        tx.execute(
            "INSERT INTO snapshot_bodies (snapshot_id, content) VALUES (?1, ?2)",
            params![snapshot_id, bytes],
        )?;
    }
    tx.execute(
        "INSERT INTO snapshot_summaries (snapshot_id, summary_text, updated_at) VALUES (?1, ?2, ?3)",
        params![snapshot_id, summary_line, now_ms],
    )?;
    for s in &symbols {
        tx.execute(
            "INSERT INTO symbols (snapshot_id, kind, name, fq_name, start_byte, end_byte) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                snapshot_id,
                s.kind,
                s.name,
                s.fq_name,
                s.start_byte as i64,
                s.end_byte as i64,
            ],
        )?;
    }
    for (ch, name, kind) in &changes {
        let c = match ch {
            SymChange::Added => "added",
            SymChange::Removed => "removed",
            SymChange::Modified => "modified",
        };
        tx.execute(
            "INSERT INTO symbol_changes (snapshot_id, change, name, kind, prev_snapshot_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![snapshot_id, c, name, kind, prev_id],
        )?;
    }
    tx.commit()?;
    Ok(true)
}
