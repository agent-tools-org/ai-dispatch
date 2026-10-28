// Tests for task detail delivery reporting.
// Covers final commit stat, final branch display, and branch drift warnings.
// Deps: board::render_task_detail, git CLI, Task fixtures.

use super::render_task_detail;
use crate::support::test_subprocess;
use crate::types::{Task, TaskId, TaskStatus};

use std::path::Path;


use crate::support::test_env::git;

use crate::support::test_env::git_output as git_stdout;

fn task(repo: &Path, start_sha: &str, final_sha: &str) -> Task {
    Task {
        repo_path: Some(repo.to_string_lossy().to_string()),
        worktree_path: Some(repo.to_string_lossy().to_string()),
        worktree_branch: Some("dispatch/branch".to_string()),
        final_head_sha: Some(final_sha.to_string()),
        final_branch: Some("agent/final".to_string()),
        start_sha: Some(start_sha.to_string()),
        ..crate::types::Task::test_fixture(TaskId("t-detail-final".to_string()), TaskStatus::Done)
    }
}

#[test]
fn detail_shows_delivered_commit_stat_and_branch_drift() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["switch", "-c", "agent/final"]);
    std::fs::write(repo.path().join("feature.txt"), "one\ntwo\n").unwrap();
    git(repo.path(), &["add", "feature.txt"]);
    git(repo.path(), &["commit", "-m", "final work"]);
    let final_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    let output = render_task_detail(&task(repo.path(), &start_sha, &final_sha), &[], None);

    assert!(output.contains("Delivered: 1 files (+2/-0)"), "output: {output}");
    assert!(output.contains("\"final work\""), "output: {output}");
    assert!(output.contains("Branch:    agent/final"), "output: {output}");
    assert!(output.contains("agent switched branch: dispatch/branch -> agent/final"), "output: {output}");
}
