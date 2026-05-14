use std::fs;
use std::path::Path;

use diffloom::db;
use diffloom::ingest;
use git2::Repository;

fn init_repo_with_commit(root: &Path) {
    let repo = Repository::init(root).unwrap();
    fs::write(root.join("README"), b"init").unwrap();
    let mut idx = repo.index().unwrap();
    idx.add_path(Path::new("README")).unwrap();
    idx.write().unwrap();
    let tree_id = idx.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
}

#[test]
fn ingest_skips_diffloom_and_git_paths() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let mut conn = db::open_db(tmp.path()).unwrap();
    let p_git = tmp.path().join(".git/zz_marker");
    fs::write(&p_git, b"x").unwrap();
    assert!(!ingest::ingest_path(&mut conn, tmp.path(), &p_git).unwrap());
    let p_dl = tmp.path().join(".diffloom/inside.txt");
    fs::create_dir_all(p_dl.parent().unwrap()).unwrap();
    fs::write(&p_dl, b"y").unwrap();
    assert!(!ingest::ingest_path(&mut conn, tmp.path(), &p_dl).unwrap());
}

#[test]
fn ingest_rust_file_twice_skips_when_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let mut conn = db::open_db(tmp.path()).unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("lib.rs");
    fs::write(
        &file,
        r#"pub fn one() -> u32 { 1 }
pub fn two() -> u32 { 2 }
"#,
    )
    .unwrap();
    assert!(ingest::ingest_path(&mut conn, tmp.path(), &file).unwrap());
    assert!(!ingest::ingest_path(&mut conn, tmp.path(), &file).unwrap());
    let rows = db::list_recent_snapshots(&conn, 5).unwrap();
    assert_eq!(rows.len(), 1);
    let syms = db::load_symbols(&conn, rows[0].id).unwrap();
    assert!(syms.iter().any(|s| s.name == "one"));
    assert!(syms.iter().any(|s| s.name == "two"));
}

#[test]
fn ingest_rust_detects_symbol_change() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let mut conn = db::open_db(tmp.path()).unwrap();
    let file = tmp.path().join("m.rs");
    fs::write(&file, "pub fn a() {}\n").unwrap();
    assert!(ingest::ingest_path(&mut conn, tmp.path(), &file).unwrap());
    fs::write(&file, "pub fn a() {}\npub fn b() {}\n").unwrap();
    assert!(ingest::ingest_path(&mut conn, tmp.path(), &file).unwrap());
    let rows = db::list_recent_snapshots(&conn, 2).unwrap();
    assert_eq!(rows.len(), 2);
    let latest = rows[0].id;
    let changes = db::load_symbol_changes(&conn, latest).unwrap();
    assert!(changes.iter().any(|(c, n, _)| c == "added" && n == "b"));
}

#[test]
fn ingest_non_rust_snapshot_without_symbols() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let mut conn = db::open_db(tmp.path()).unwrap();
    let file = tmp.path().join("note.md");
    fs::write(&file, "# title\n").unwrap();
    assert!(ingest::ingest_path(&mut conn, tmp.path(), &file).unwrap());
    let rows = db::list_recent_snapshots(&conn, 1).unwrap();
    let syms = db::load_symbols(&conn, rows[0].id).unwrap();
    assert!(syms.is_empty());
    let summary = db::snapshot_summary(&conn, rows[0].id).unwrap().unwrap();
    assert!(summary.contains("non-Rust"));
}
