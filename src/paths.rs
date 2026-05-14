use std::path::{Component, Path, PathBuf};

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
    rel.components().any(|c| {
        matches!(c, Component::Normal(name) if {
            let n = name.to_string_lossy();
            n == ".git" || n == ".diffloom" || n == "target" || n == "node_modules"
        })
    })
}
