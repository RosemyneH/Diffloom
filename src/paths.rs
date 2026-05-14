use std::path::{Path, PathBuf};

pub fn normalize_path(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

pub fn rel_under_root(root: &Path, abs: &Path) -> anyhow::Result<PathBuf> {
    let root = normalize_path(root)?;
    let abs = normalize_path(abs)?;
    if !abs.starts_with(&root) {
        anyhow::bail!("path outside workspace");
    }
    Ok(abs.strip_prefix(&root)?.to_path_buf())
}

pub fn should_skip_watch(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    s.contains("/.git/")
        || s.starts_with(".git/")
        || s.contains("/.diffloom/")
        || s.starts_with(".diffloom/")
        || s.contains("/target/")
        || s.starts_with("target/")
        || s.contains("/node_modules/")
        || s.starts_with("node_modules/")
}
