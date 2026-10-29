// Tests for aid merge — unit tests for git helpers + integration tests for merge workflows.
// Deps: super::*, test_subprocess, tempfile

use super::*;
use crate::support::test_subprocess;
use crate::types::*;

use std::env;
use std::path::Path;
use tempfile::TempDir;

use crate::support::test_env::git;

use crate::support::test_repo::unique_name as unique;

/// Create a git repo with one commit. Returns the TempDir.
fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@aid.dev"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("init.txt"), "init\n").unwrap();
    git(repo.path(), &["add", "init.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

/// Create a worktree branch with one committed change. Returns (worktree_dir, branch_name).
fn create_worktree_with_commit(repo: &Path) -> (TempDir, String) {
    let branch = unique("test-branch");
    let wt = TempDir::new().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            &branch,
        ],
    );
    std::fs::write(wt.path().join("agent-work.txt"), "agent output\n").unwrap();
    git(wt.path(), &["add", "agent-work.txt"]);
    git(wt.path(), &["commit", "-m", "agent: implement feature"]);
    (wt, branch)
}

fn create_empty_worktree_branch(repo: &Path) -> (TempDir, String) {
    let branch = unique("empty-branch");
    let wt = TempDir::new().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            &branch,
        ],
    );
    (wt, branch)
}

fn create_conflict_worktree(repo: &Path, branch: &str) -> TempDir {
    let wt = TempDir::new().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            &wt.path().to_string_lossy(),
            "-b",
            branch,
        ],
    );
    std::fs::write(wt.path().join("init.txt"), "branch version\n").unwrap();
    git(wt.path(), &["add", "init.txt"]);
    git(wt.path(), &["commit", "-m", "branch change"]);
    std::fs::write(repo.join("init.txt"), "main version\n").unwrap();
    git(repo, &["add", "init.txt"]);
    git(repo, &["commit", "-m", "main change"]);
    wt
}

fn worktree_status(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "status", "--short"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn head_message(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn make_task_with_worktree(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        prompt: "test".to_string(),
        repo_path: Some(repo.to_string_lossy().to_string()),
        worktree_path: Some(wt.to_string_lossy().to_string()),
        worktree_branch: Some(branch.to_string()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
    }
}

#[path = "force_tests.rs"]
mod force_tests;
#[path = "git_tests.rs"]
mod git_tests;
#[path = "group_tests.rs"]
mod group_tests;
#[path = "lock_tests.rs"]
mod lock_tests;
#[path = "merge_check_tests.rs"]
mod merge_check_tests;
#[path = "single_tests.rs"]
mod single_tests;
#[path = "stash_tests.rs"]
mod stash_tests;
