use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use notify_debouncer_mini::{
    new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
    DebounceEventResult, Debouncer,
};

pub fn watch_workspace(
    root: PathBuf,
    debounce: Duration,
) -> anyhow::Result<(
    Debouncer<RecommendedWatcher>,
    std::sync::mpsc::Receiver<PathBuf>,
)> {
    let root = crate::paths::normalize_path(&root).context("normalize watch root")?;
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    let root_cb = root.clone();
    let mut debouncer = new_debouncer(debounce, move |res: DebounceEventResult| {
        if let Ok(events) = res {
            for ev in events {
                if !ev.path.is_file() {
                    continue;
                }
                let Ok(rel) = crate::paths::rel_under_root(&root_cb, &ev.path) else {
                    continue;
                };
                if crate::paths::should_skip_watch(&rel) {
                    continue;
                }
                let _ = tx.send(ev.path);
            }
        }
    })
    .context("debouncer")?;
    debouncer
        .watcher()
        .watch(&root, RecursiveMode::Recursive)
        .context("watch")?;
    Ok((debouncer, rx))
}
