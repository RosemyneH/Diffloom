use std::fs;
use std::path::Path;

use diffloom::git_info;
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
fn repo_head_present_and_dirty_tracks_untracked() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_commit(tmp.path());
    let (head, dirty) = git_info::repo_head_and_dirty(tmp.path()).unwrap();
    assert!(head.is_some());
    fs::write(tmp.path().join("untracked.txt"), b"u").unwrap();
    let (_, dirty2) = git_info::repo_head_and_dirty(tmp.path()).unwrap();
    assert!(dirty2);
    assert!(!dirty);
}
