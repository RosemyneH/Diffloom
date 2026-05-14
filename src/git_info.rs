use std::path::Path;

use anyhow::Context;
use git2::{Oid, Repository, StatusOptions};

pub fn current_branch(root: &Path) -> Option<String> {
    let repo = Repository::discover(root).ok()?;
    let head = repo.head().ok()?;
    head.shorthand().map(|s| s.to_string())
}

pub fn repo_head_and_dirty(root: &Path) -> anyhow::Result<(Option<String>, bool)> {
    let repo = match Repository::discover(root) {
        Ok(r) => r,
        Err(_) => return Ok((None, false)),
    };
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.target().map(|oid| oid.to_string()));
    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    opts.include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts)).context("git status")?;
    let dirty = !statuses.is_empty();
    Ok((head, dirty))
}

pub fn format_snapshot_git_context(
    root: &Path,
    commit_oid_hex: Option<&str>,
    dirty: bool,
) -> Option<String> {
    let repo = Repository::discover(root).ok()?;
    let head_ok = repo.head().ok();
    let branch = head_ok
        .as_ref()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "(detached or empty)".to_string());

    let mut out = String::new();
    match commit_oid_hex.map(str::trim).filter(|s| !s.is_empty()) {
        Some(hex) => {
            let short: String = hex.chars().take(7).collect();
            match Oid::from_str(hex)
                .ok()
                .and_then(|oid| repo.find_commit(oid).ok())
            {
                Some(c) => {
                    let raw = c.summary().unwrap_or("");
                    let line = raw.lines().next().unwrap_or("").trim();
                    let line = if line.is_empty() {
                        "(no subject)"
                    } else {
                        line
                    };
                    out.push_str(&format!("Git: {branch} @ {short} — {line}"));
                }
                None => {
                    out.push_str(&format!(
                        "Git: {branch} @ {short} (oid not found as commit in this repo)"
                    ));
                }
            }
        }
        None => {
            if head_ok.is_some() {
                out.push_str(&format!(
                    "Git: {branch} (no HEAD oid recorded for this snapshot)"
                ));
            } else {
                out.push_str("Git: repository has no commits yet.");
            }
        }
    }
    if dirty {
        out.push_str("\nWorking tree was dirty when this snapshot was recorded (uncommitted or untracked changes).");
    }
    Some(out)
}
