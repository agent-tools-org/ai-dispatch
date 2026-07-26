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

#[test]
fn create_worktree_rejects_reuse_when_worker_lock_pid_is_alive() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/live-lock");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    let lock = format!(
        "version=1\ntask_id=t-running\nowner_pid=999999999\nworker_pid={}\n",
        std::process::id()
    );
    std::fs::write(info.path.join(".aid-lock"), lock).unwrap();

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();

    assert!(err.to_string().contains("locked by task t-running"));
    std::fs::remove_file(info.path.join(".aid-lock")).unwrap();
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

// Issue #113 regression: worktree drifts off the requested branch (e.g. agent
// ran `git checkout --detach` or switched to another branch). Subsequent
// dispatches must refuse the wrong HEAD instead of silently using it.
#[test]
fn create_worktree_refuses_when_head_drifted_detached() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale-drift");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    // Simulate an agent doing a detached checkout (e.g. `git checkout HEAD~0`).
    git(info.path.as_path(), &["checkout", "--detach", "HEAD"]);
    // Sanity: HEAD is no longer on the requested branch.
    let symref_status = Command::new("git")
        .args([
            "-C",
            &info.path.to_string_lossy(),
            "symbolic-ref",
            "--short",
            "-q",
            "HEAD",
        ])
        .status()
        .unwrap();
    assert!(!symref_status.success(), "expected detached HEAD");

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("(detached HEAD)"), "msg: {msg}");
    assert!(msg.contains(branch.as_str()), "msg: {msg}");

    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

// Companion case: drift + uncommitted changes — must NOT silently overwrite
// the dirty state. The error message should explain the drift and suggest
// manual cleanup.
#[test]
fn create_worktree_errors_when_head_drifted_and_dirty() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale-drift-dirty");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    git(info.path.as_path(), &["checkout", "--detach", "HEAD"]);
    std::fs::write(info.path.join("dirty.txt"), "dirty\n").unwrap();

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("(detached HEAD)"), "msg: {msg}");
    assert!(msg.contains("uncommitted changes"), "msg: {msg}");
    assert!(msg.contains(branch.as_str()), "msg: {msg}");

    // Cleanup
    std::fs::remove_file(info.path.join("dirty.txt")).ok();
    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

#[test]
fn create_worktree_refuses_when_worktree_is_on_different_branch() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/stale-other");
    let other = unique_branch("feat/stale-other-current");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();
    git(info.path.as_path(), &["checkout", "-b", other.as_str()]);

    let err = create_worktree(repo.path(), branch.as_str(), None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains(other.as_str()), "msg: {msg}");
    assert!(msg.contains(branch.as_str()), "msg: {msg}");

    git(
        repo.path(),
        &["worktree", "remove", "--force", &info.path.to_string_lossy()],
    );
}

#[test]
fn create_worktree_recreates_pruned_branch_with_unmerged_commit() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/pruned-unmerged");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    std::fs::write(info.path.join("agent.txt"), "agent\n").unwrap();
    git(info.path.as_path(), &["add", "agent.txt"]);
    git(info.path.as_path(), &["commit", "-m", "agent commit"]);
    std::fs::remove_dir_all(&info.path).unwrap();

    let branch_head = git_output(repo.path(), &["rev-parse", branch.as_str()]);

    let recreated = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    assert_eq!(git_output(repo.path(), &["rev-parse", branch.as_str()]), branch_head);
    assert_eq!(std::fs::read_to_string(recreated.path.join("agent.txt")).unwrap(), "agent\n");
    assert_eq!(
        git_output(recreated.path.as_path(), &["merge-base", "--is-ancestor", &branch_head, "HEAD"]),
        ""
    );
    git(
        repo.path(),
        &["worktree", "remove", "--force", &recreated.path.to_string_lossy()],
    );
}

#[test]
fn create_worktree_allows_pruned_branch_without_unmerged_commit() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = unique_branch("feat/pruned-merged");
    let info = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    std::fs::write(info.path.join("merged.txt"), "merged\n").unwrap();
    git(info.path.as_path(), &["add", "merged.txt"]);
    git(info.path.as_path(), &["commit", "-m", "merged commit"]);
    git(repo.path(), &["merge", "--no-edit", branch.as_str()]);
    std::fs::remove_dir_all(&info.path).unwrap();

    let recreated = create_worktree(repo.path(), branch.as_str(), None).unwrap();

    assert_eq!(recreated.path, aid_worktree_path(repo.path(), &branch));
    assert!(recreated.path.exists());
    assert_eq!(
        git_output(repo.path(), &["rev-parse", "HEAD"]),
        git_output(recreated.path.as_path(), &["rev-parse", "HEAD"])
    );
    git(
        repo.path(),
        &["worktree", "remove", "--force", &recreated.path.to_string_lossy()],
    );
}
