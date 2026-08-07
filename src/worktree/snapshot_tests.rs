use super::{
    WorktreeStatusKind, capture_worktree_snapshot, capture_worktree_snapshot_with_base,
    is_rescuable_path, parse_status_entry,
};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo_with_main() -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "aid@example.com"]);
    git(dir.path(), &["config", "user.name", "Aid Tester"]);
    std::fs::write(dir.path().join("file.txt"), "initial").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn checkout_feature_with_change(dir: &Path) {
    git(dir, &["checkout", "-b", "feature"]);
    std::fs::write(dir.join("file.txt"), "updated").unwrap();
    git(dir, &["add", "file.txt"]);
    git(dir, &["commit", "-m", "feature change"]);
}

#[test]
fn worktree_snapshot_parses_status_entries() {
    let untracked = parse_status_entry("?? src/new.rs").unwrap();
    assert_eq!(untracked.path, "src/new.rs");
    assert_eq!(untracked.kind, WorktreeStatusKind::Untracked);

    let modified = parse_status_entry(" M src/lib.rs").unwrap();
    assert_eq!(modified.path, "src/lib.rs");
    assert_eq!(modified.kind, WorktreeStatusKind::Modified);

    assert!(parse_status_entry(" D src/lib.rs").is_none());
}

#[test]
fn worktree_snapshot_filters_non_source_artifacts() {
    assert!(is_rescuable_path("src/lib.rs"));
    assert!(!is_rescuable_path("target/debug/app"));
    assert!(!is_rescuable_path("cache/file.pyc"));
}

#[test]
fn is_rescuable_path_excludes_aid_artifacts() {
    assert!(!is_rescuable_path("result-t-abc123.md"));
    assert!(!is_rescuable_path("result-t-0d8f.md"));
    assert!(!is_rescuable_path(".aid/results/foo.md"));
    assert!(is_rescuable_path("results/foo.md"));
    assert!(is_rescuable_path("my-result-t.md"));
}

#[test]
fn empty_diff_is_false_for_committed_only_change_against_base() {
    let dir = repo_with_main();
    checkout_feature_with_change(dir.path());

    let snapshot = capture_worktree_snapshot_with_base(dir.path(), Some("main")).unwrap();

    assert_eq!(snapshot.empty_diff, Some(false));
    assert!(!snapshot.has_uncommitted_changes());
}

#[test]
fn empty_diff_is_true_for_clean_worktree_without_commits_ahead() {
    let dir = repo_with_main();

    let snapshot = capture_worktree_snapshot_with_base(dir.path(), Some("main")).unwrap();

    assert_eq!(snapshot.empty_diff, Some(true));
}

#[test]
fn empty_diff_is_false_for_dirty_uncommitted_change() {
    let dir = repo_with_main();
    std::fs::write(dir.path().join("file.txt"), "dirty").unwrap();

    let snapshot = capture_worktree_snapshot_with_base(dir.path(), Some("main")).unwrap();

    assert_eq!(snapshot.empty_diff, Some(false));
    assert!(snapshot.has_uncommitted_changes());
}

#[test]
fn empty_diff_uses_default_branch_fallback_for_committed_change() {
    let dir = repo_with_main();
    checkout_feature_with_change(dir.path());

    let snapshot = capture_worktree_snapshot(dir.path()).unwrap();

    assert_eq!(snapshot.empty_diff, Some(false));
}
