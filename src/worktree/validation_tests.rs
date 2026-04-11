// Linked-worktree validation tests for shared-ref worktree detection.
// Exports: none.
// Deps: super helpers, tempfile, crate::test_subprocess, std::process::Command.

use super::{create_worktree, is_valid_git_worktree};
use crate::test_subprocess;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn git(repo_dir: &Path, args: &[&str]) {
    let repo_dir = repo_dir.to_string_lossy().to_string();
    assert!(Command::new("git")
        .args(["-C", repo_dir.as_str()])
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
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn cleanup_worktree(repo_dir: &Path, worktree: &Path) {
    let _ = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "worktree",
            "remove",
            "--force",
            &worktree.to_string_lossy(),
        ])
        .status();
    let _ = std::fs::remove_dir_all(worktree);
}

#[test]
fn is_valid_git_worktree_rejects_standalone_repo() {
    let _permit = test_subprocess::acquire();
    let main_repo = init_repo();
    let standalone = TempDir::new().unwrap();
    git(standalone.path(), &["init", "-b", "main"]);
    assert!(!is_valid_git_worktree(main_repo.path(), standalone.path()).unwrap());
}

#[test]
fn is_valid_git_worktree_accepts_linked_worktree() {
    let _permit = test_subprocess::acquire();
    let main_repo = init_repo();
    let worktree_root = TempDir::new().unwrap();
    let worktree = worktree_root.path().join("linked");
    git(
        main_repo.path(),
        &["worktree", "add", "-b", "feat/linked-valid", &worktree.to_string_lossy()],
    );

    assert!(is_valid_git_worktree(main_repo.path(), &worktree).unwrap());

    cleanup_worktree(main_repo.path(), &worktree);
}

#[test]
fn is_valid_git_worktree_rejects_worktree_of_different_repo() {
    let _permit = test_subprocess::acquire();
    let repo_a = init_repo();
    let repo_b = init_repo();
    let worktree_root = TempDir::new().unwrap();
    let worktree = worktree_root.path().join("other");
    git(
        repo_b.path(),
        &["worktree", "add", "-b", "feat/other-repo", &worktree.to_string_lossy()],
    );

    assert!(!is_valid_git_worktree(repo_a.path(), &worktree).unwrap());

    cleanup_worktree(repo_b.path(), &worktree);
}

#[test]
fn create_worktree_recreates_when_existing_path_is_standalone() {
    let _permit = test_subprocess::acquire();
    let main_repo = init_repo();
    let branch = "test-branch";
    let expected_path = PathBuf::from(format!("/tmp/aid-wt-{branch}"));
    let _ = std::fs::remove_dir_all(&expected_path);
    std::fs::create_dir_all(&expected_path).unwrap();
    git(&expected_path, &["init", "-b", "main"]);
    std::fs::write(expected_path.join("standalone.txt"), "stale\n").unwrap();

    let info = create_worktree(main_repo.path(), branch, None).unwrap();

    assert_eq!(info.path, expected_path);
    assert!(is_valid_git_worktree(main_repo.path(), &info.path).unwrap());
    assert!(!info.path.join("standalone.txt").exists());

    cleanup_worktree(main_repo.path(), &info.path);
}
