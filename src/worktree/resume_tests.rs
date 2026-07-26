// Worktree resume tests for existing branches and unsafe target paths.
// Exports: none.
// Deps: super worktree helpers, git CLI, tempfile.

use super::*;
use crate::test_subprocess;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn unique_branch(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
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

#[test]
fn create_worktree_refuses_stale_directory_at_target_path() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale");
    let expected_path = aid_worktree_path(repo.path(), &branch);
    std::fs::create_dir_all(&expected_path).unwrap();
    std::fs::write(expected_path.join("stale.txt"), "stale\n").unwrap();

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("already exists"), "msg: {msg}");
    assert!(msg.contains("refusing to replace it automatically"), "msg: {msg}");
    let _ = std::fs::remove_dir_all(&expected_path);
}

#[test]
fn create_worktree_rejects_non_aid_branch_on_explicit_force_reset() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("legacy");
    git(repo.path(), &["branch", branch.as_str()]);

    let err = create_worktree(repo.path(), branch.as_str(), Some("main")).unwrap_err();
    assert!(err.to_string().contains("Refusing to force-reset branch"));
}

#[test]
fn create_worktree_resumes_existing_aid_branch_without_reset() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/reset");
    git(repo.path(), &["branch", branch.as_str()]);

    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    assert!(info.path.exists());
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}
