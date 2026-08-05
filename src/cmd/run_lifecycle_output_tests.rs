// Focused tests for post-run output length detection.
// Exports no helpers; exercises transcript fallback used by hollow-output checks.
// Deps: run_lifecycle output/final-state helpers, paths, task domain types.

use super::run_lifecycle::{capture_final_worktree_state, output_content_length};
use crate::store::Store;
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None,
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
        delivery_assessment: None,
    }
}

#[test]
fn output_content_length_counts_plain_text_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let task = task("t-transcript-output");
    let transcript = crate::paths::transcript_path(task.id.as_str());
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "plain transcript output\n".repeat(20)).unwrap();

    assert!(output_content_length(&task) >= 200);
}

#[test]
fn output_content_length_counts_multibyte_output_as_characters() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.md");
    let content = format!("{}{}", "a".repeat(196), "\u{2019}".repeat(2));
    assert!(content.len() >= 200);
    assert!(content.chars().count() < 200);
    std::fs::write(&output_path, content).unwrap();
    let mut task = task("t-short-multibyte");
    task.output_path = Some(output_path.to_string_lossy().to_string());

    assert!(output_content_length(&task) < 200);
}

#[test]
fn output_content_length_keeps_long_ascii_output_at_threshold() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.md");
    std::fs::write(&output_path, "a".repeat(200)).unwrap();
    let mut task = task("t-long-ascii");
    task.output_path = Some(output_path.to_string_lossy().to_string());

    assert!(output_content_length(&task) >= 200);
}

#[test]
fn capture_final_worktree_state_records_switched_branch() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    git(repo.path(), &["switch", "-c", "agent/final"]);
    std::fs::write(repo.path().join("final.txt"), "done\n").unwrap();
    git(repo.path(), &["add", "final.txt"]);
    git(repo.path(), &["commit", "-m", "final work"]);
    let head = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    let store = Store::open_memory().unwrap();
    let mut task = task("t-final-capture");
    task.worktree_path = Some(repo.path().to_string_lossy().to_string());
    task.worktree_branch = Some("dispatch/branch".to_string());
    store.insert_task(&task).unwrap();

    capture_final_worktree_state(&store, &task.id).unwrap();
    git(repo.path(), &["switch", "-c", "agent/later"]);
    capture_final_worktree_state(&store, &task.id).unwrap();

    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.final_head_sha.as_deref(), Some(head.as_str()));
    assert_eq!(loaded.final_branch.as_deref(), Some("agent/final"));
}
