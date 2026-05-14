use std::path::Path;

use anyhow::Context;
use git2::{Repository, StatusOptions};

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
