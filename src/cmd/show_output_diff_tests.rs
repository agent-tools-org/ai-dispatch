// Tests for start-SHA-aware diff rendering in `aid show`.
// Exports: none; validates task-scoped diff bases and failed-task no-op messaging.
// Deps: show_output hub, Store, Task, git CLI, tempfile.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}

fn write_and_commit(dir: &Path, file: &str, content: &str, message: &str) {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", message]);
}

fn init_repo(dir: &Path) {
    git(dir, &["init"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    write_and_commit(dir, "base.txt", "base\n", "base");
}

fn task_fixture(id: &str, repo: &Path, start_sha: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(repo.display().to_string()),
        worktree_branch: Some("task-branch".to_string()),
        start_sha: Some(start_sha.to_string()),
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
    }
}

#[test]
fn diff_text_uses_start_sha_to_exclude_prior_task_commits() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "old.txt", "old task\n", "previous task");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "new.txt", "new task\n", "current task");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-start-sha", repo.path(), &start_sha, TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("new.txt"), "got: {text}");
    assert!(!text.contains("old.txt"), "got: {text}");
}

#[test]
fn failed_task_without_new_commits_reports_no_changes() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    write_and_commit(repo.path(), "old.txt", "old task\n", "previous task");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-no-commit", repo.path(), &start_sha, TaskStatus::Failed);
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("No changes (task failed before making commits)"));
    assert!(!text.contains("old.txt"), "got: {text}");
}
