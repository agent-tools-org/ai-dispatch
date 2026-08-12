// Task-relative diff regression tests for summaries and automated judges.
// Covers unchanged task custody plus committed and unstaged task changes.
// Deps: judge::gather_diff, Task metadata, temporary Git repositories.

use super::{gather_diff, judge_review_material};
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

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
        id: TaskId("t-diff".into()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "test".into(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: Some(repo.display().to_string()), project_id: None, worktree_path: None, effective_dir: None,
        worktree_branch: None, final_head_sha: None, final_branch: None,
        start_sha, log_path: None, output_path: None, tokens: None,
        prompt_tokens: None, duration_ms: None, requested_model: None,
        observed_model: None, attribution_source: None, cost_usd: None, exit_code: Some(1),
        created_at: Local::now(), completed_at: None, verify: None,
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
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
