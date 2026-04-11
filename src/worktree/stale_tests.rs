// Worktree stale-state tests for issue #92.
// Exports: none.
// Deps: super::create_worktree, tempfile, std::process::Command.
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

fn git_output(repo_dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
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
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

#[test]
fn create_worktree_errors_when_stale_worktree_has_local_changes() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale-error");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    std::fs::write(info.path.join("dirty.txt"), "dirty\n").unwrap();
    std::fs::write(repo.path().join("new-batch.toml"), "[[tasks]]\nagent = \"codex\"\n").unwrap();
    git(repo.path(), &["add", "new-batch.toml"]);
    git(repo.path(), &["commit", "-m", "advance main"]);

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();

    assert!(err.to_string().contains("cannot be auto-refreshed"));
    assert!(err.to_string().contains("aid worktree remove"));
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

#[test]
fn create_worktree_refreshes_clean_stale_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale-refresh");
    let first = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    std::fs::write(repo.path().join("batch.toml"), "[[tasks]]\nagent = \"codex\"\n").unwrap();
    git(repo.path(), &["add", "batch.toml"]);
    git(repo.path(), &["commit", "-m", "advance main"]);

    let refreshed = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    assert_eq!(refreshed.path, first.path);
    assert!(refreshed.path.join("batch.toml").exists());
    assert_eq!(
        git_output(repo.path(), &["rev-parse", "HEAD"]),
        git_output(refreshed.path.as_path(), &["rev-parse", "HEAD"])
    );
    git(
        repo.path(),
        &["worktree", "remove", "--force", &refreshed.path.to_string_lossy()],
    );
}

#[test]
fn create_worktree_reuses_non_diverged_worktree() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale-normal");
    let first = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    let second = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    assert_eq!(first.path, second.path);
    assert_eq!(
        git_output(repo.path(), &["rev-parse", "HEAD"]),
        git_output(second.path.as_path(), &["rev-parse", "HEAD"])
    );
    git(
        repo.path(),
        &["worktree", "remove", "--force", &second.path.to_string_lossy()],
    );
}
