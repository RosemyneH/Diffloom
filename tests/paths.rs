use std::fs;
use std::path::Path;

use diffloom::paths::{rel_under_root, should_skip_watch};

#[test]
fn should_skip_watch_ignores_vcs_and_artifacts() {
    assert!(should_skip_watch(Path::new(".git/config")));
    assert!(should_skip_watch(Path::new("pkg/.git/HEAD")));
    assert!(should_skip_watch(Path::new(".diffloom/db.sqlite")));
    assert!(should_skip_watch(Path::new("lib/.diffloom/x")));
    assert!(should_skip_watch(Path::new("target/debug/foo")));
    assert!(should_skip_watch(Path::new("crates/foo/target/bar")));
    assert!(should_skip_watch(Path::new("node_modules/lodash/index.js")));
}

#[test]
fn should_skip_watch_allows_sources() {
    assert!(!should_skip_watch(Path::new("src/main.rs")));
    assert!(!should_skip_watch(Path::new("crates/core/src/lib.rs")));
}

#[test]
fn rel_under_root_resolves_nested_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let nested = root.join("deep").join("file.txt");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, b"x").unwrap();
    let rel = rel_under_root(root, &nested).unwrap();
    assert_eq!(rel, Path::new("deep/file.txt"));
}

#[test]
fn rel_under_root_rejects_path_outside_root() {
    let inner = tempfile::tempdir().unwrap();
    let outer = tempfile::tempdir().unwrap();
    let file = outer.path().join("outside.txt");
    fs::write(&file, b"y").unwrap();
    let err = rel_under_root(inner.path(), &file).unwrap_err();
    assert!(err.to_string().contains("outside workspace"));
}
