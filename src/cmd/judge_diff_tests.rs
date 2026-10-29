// Task-relative diff regression tests for summaries and automated judges.
// Covers unchanged task custody plus committed and unstaged task changes.
// Deps: judge::gather_diff, Task metadata, temporary Git repositories.

use super::{gather_diff, judge_review_material};
use crate::types::{Task, TaskId, TaskStatus};

use std::path::Path;

use crate::support::test_env::git_output as git;

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("initial.txt"), "initial\n").unwrap();
    git(repo.path(), &["add", "initial.txt"]);
    git(repo.path(), &["commit", "-m", "initial commit"]);
    std::fs::write(repo.path().join("prior.txt"), "prior\n").unwrap();
    std::fs::write(repo.path().join("tracked.txt"), "initial\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "prior repository commit"]);
    repo
}

fn task(repo: &Path, start_sha: Option<String>) -> Task {
    Task {
        prompt: "test".into(),
        repo_path: Some(repo.display().to_string()),
        start_sha, exit_code: Some(1),
        ..crate::types::Task::test_fixture(TaskId("t-diff".into()), TaskStatus::Failed)
    }
}

#[test]
fn unchanged_task_does_not_borrow_the_previous_commit() {
    let repo = init_repo();
    let task = task(repo.path(), Some(git(repo.path(), &["rev-parse", "HEAD"])));

    assert_eq!(gather_diff(&task), None);
}

#[test]
fn task_diff_includes_committed_and_unstaged_changes_since_start() {
    let repo = init_repo();
    let start_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    std::fs::write(repo.path().join("prior.txt"), "committed by task\n").unwrap();
    git(repo.path(), &["add", "prior.txt"]);
    git(repo.path(), &["commit", "-m", "task commit"]);
    std::fs::write(repo.path().join("tracked.txt"), "unstaged by task\n").unwrap();

    let diff = gather_diff(&task(repo.path(), Some(start_sha))).unwrap();
    assert!(diff.contains("prior.txt"));
    assert!(diff.contains("tracked.txt"));
}

#[test]
fn task_without_start_sha_includes_current_staged_changes() {
    let repo = init_repo();
    std::fs::write(repo.path().join("tracked.txt"), "staged by task\n").unwrap();
    git(repo.path(), &["add", "tracked.txt"]);

    assert!(gather_diff(&task(repo.path(), None)).unwrap().contains("staged by task"));
}

#[test]
fn task_without_start_sha_includes_latest_committed_changes() {
    let repo = init_repo();
    std::fs::write(repo.path().join("tracked.txt"), "committed by task\n").unwrap();
    git(repo.path(), &["add", "tracked.txt"]);
    git(repo.path(), &["commit", "-m", "task commit"]);

    let diff = gather_diff(&task(repo.path(), None)).expect("latest task commit should be visible");
    assert!(diff.contains("tracked.txt"));
    assert!(diff.contains("committed by task"));
}

#[test]
fn judge_material_reports_missing_owned_output() {
    let repo = init_repo();
    let mut missing = task(repo.path(), Some(git(repo.path(), &["rev-parse", "HEAD"])));
    missing.output_path = Some("report.md".to_string());

    let material = judge_review_material(&missing);
    assert!(
        material.contains("No task-owned output file"),
        "absence must be explicit: {material}"
    );
    assert!(
        material.contains("(no diff or output)"),
        "must not invent a report: {material}"
    );
}
