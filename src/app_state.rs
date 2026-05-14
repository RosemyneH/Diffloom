use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    last_workspace: Option<PathBuf>,
}

fn data_dir() -> anyhow::Result<PathBuf> {
    if let Ok(v) = std::env::var("DIFFLOOM_STATE_DIR") {
        return Ok(PathBuf::from(v));
    }
    dirs::data_local_dir()
        .map(|p| p.join("diffloom"))
        .context("could not resolve data directory (set DIFFLOOM_STATE_DIR to override)")
}

fn state_path(base: &Path) -> PathBuf {
    base.join("state.json")
}

fn read_state(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let s: StateFile = serde_json::from_str(&raw).unwrap_or_default();
    Ok(s.last_workspace.filter(|p| p.is_dir()))
}

fn write_state(path: &Path, root: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let s = StateFile {
        last_workspace: Some(root.to_path_buf()),
    };
    fs::write(
        path,
        serde_json::to_string(&s).with_context(|| "serialize state")?,
    )
    .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_last_workspace() -> anyhow::Result<Option<PathBuf>> {
    let base = data_dir()?;
    read_state(&state_path(&base))
}

pub fn save_last_workspace(root: &Path) -> anyhow::Result<()> {
    let base = data_dir()?;
    fs::create_dir_all(&base).with_context(|| format!("create {}", base.display()))?;
    write_state(&state_path(&base), root)
}

pub fn prompt_workspace() -> anyhow::Result<PathBuf> {
    use std::io::{self, Write};
    if !io::stdin().is_terminal() {
        anyhow::bail!(
            "Pick a workspace once in a terminal, or pass --root. Example: diffloom --root ~/src/myproject"
        );
    }
    eprint!("Workspace to watch: ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let p = line.trim();
    if p.is_empty() {
        anyhow::bail!("Workspace path cannot be empty. Example: diffloom --root ~/src/myproject");
    }
    Ok(PathBuf::from(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip_under_custom_base() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("appdata");
        fs::create_dir_all(&base).unwrap();
        let ws = tmp.path().join("myworkspace");
        fs::create_dir_all(&ws).unwrap();
        let sp = state_path(&base);
        write_state(&sp, &ws).unwrap();
        let got = read_state(&sp).unwrap().unwrap();
        assert_eq!(got, ws);
    }

    #[test]
    fn load_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let sp = state_path(tmp.path());
        assert!(read_state(&sp).unwrap().is_none());
    }

    #[test]
    fn load_ignores_missing_workspace_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sp = state_path(tmp.path());
        write_state(&sp, Path::new("/nonexistent/diffloom_test_workspace_xyz")).unwrap();
        assert!(read_state(&sp).unwrap().is_none());
    }
}
