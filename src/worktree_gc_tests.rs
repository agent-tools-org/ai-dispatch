// Tests for worktree GC candidate detection and merge heuristics.
// Covers branch prefix filtering, merge detection, and safe tmp-path matching.
// Deps: super, crate::test_subprocess, tempfile, std::process::Command.

use super::{
    MergeReason, branch_merge_reason, is_managed_branch, merge_reason_from_outputs,
    same_tmp_worktree_path,
};
use crate::test_subprocess;
use crate::worktree::is_aid_managed_worktree_path;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

#[test]
fn branch_merge_reason_detects_empty_git_cherry_after_merge() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    git(repo.path(), &["checkout", "-b", "feat/merged"]);
    std::fs::write(repo.path().join("file.txt"), "branch\n").unwrap();
    git(repo.path(), &["commit", "-am", "branch"]);
    git(repo.path(), &["checkout", "main"]);
    git(repo.path(), &["merge", "--no-ff", "feat/merged", "-m", "merge"]);

    let reason = branch_merge_reason(repo.path(), "main", "feat/merged").unwrap();

    assert_eq!(reason, Some(MergeReason::CherryEmpty));
}

#[test]
fn merge_reason_from_outputs_detects_empty_git_log_fallback() {
    let reason = merge_reason_from_outputs("+ abc123 commit\n", "");
    assert_eq!(reason, Some(MergeReason::LogEmpty));
}

#[test]
fn managed_branch_filter_skips_protected_names() {
    let prefixes = vec!["feat/".to_string(), "fix/".to_string()];

    assert!(is_managed_branch("feat/demo", &prefixes));
    assert!(!is_managed_branch("main", &prefixes));
    assert!(!is_managed_branch("master", &prefixes));
    assert!(!is_managed_branch("release/1.2.3", &prefixes));
}

#[test]
fn tmp_path_match_handles_private_tmp_alias() {
    assert!(same_tmp_worktree_path(
        "/tmp/aid-wt-demo",
        "/private/tmp/aid-wt-demo"
    ));
    assert!(is_aid_managed_worktree_path(Path::new("/tmp/aid-wt-demo")));
    assert!(is_aid_managed_worktree_path(Path::new("/private/tmp/aid-wt-demo")));
}
