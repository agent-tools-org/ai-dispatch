// Unit tests for `cmd::show` — context, summary/audit, JSON. Loaded via `#[path]` from `show.rs`.
// Deps: `super::*`, `chrono`, `tempfile`.

use super::show_json::task_json;
use super::*;
use crate::cmd::summary::CompletionSummary;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

#[path = "context_tests.rs"]
mod context;
use context::research_task;
pub(super) use context::task_fixture;

#[test]
fn summary_text_shows_diff_stat_without_full_diff() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("note.txt"), "before\n").unwrap();
    Command::new("git")
        .args(["add", "note.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("note.txt"), "before\nafter\n").unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture(
        "t-summary",
        "summarize diff",
        None,
        Some(repo.display().to_string()),
    );
    store.insert_task(&task).unwrap();

    let text = summary_text(&store, "t-summary").unwrap();

    assert!(text.contains("--- Diff Stat ---"));
    assert!(text.contains("note.txt | 1 +"));
    assert!(!text.contains("--- Full Diff ---"));
    assert!(!text.contains("@@"));
}

#[test]
fn audit_text_shows_findings_for_research_task() {
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task = research_task("t-audit-findings", temp.path());
    store.insert_task(&task).unwrap();
    save_completion_summary(&store, task.id.as_str(), "Investigated the failure mode.");

    let text = audit_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("Findings:\nInvestigated the failure mode."));
}

#[test]
fn audit_text_shows_pending_reason() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = research_task("t-show-pending-reason", Path::new("."));
    task.status = TaskStatus::Failed;
    task.pending_reason = Some("agent_starting".to_string());
    store.insert_task(&task).unwrap();

    let text = audit_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("Pending reason: agent_starting"));
}

#[test]
fn audit_text_shows_done_status_when_result_file_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-done-no-result", "investigate issue", None, None);
    store.insert_task(&task).unwrap();

    let text = audit_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("Status: DONE (no result file - see --output / output.md)"));
}

#[test]
fn audit_text_shows_inconclusive_verification_tag() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task_fixture("t-show-timeout", "investigate issue", None, None);
    task.verify = Some("cargo test".to_string());
    task.verify_status = VerifyStatus::TimedOut;
    store.insert_task(&task).unwrap();

    let text = audit_text(&store, task.id.as_str()).unwrap();

    // audit_text prints the status twice — the detail header and the missing-result
    // notice. Pin the notice line, or the header alone satisfies the assertion.
    assert!(
        text.contains("Status: DONE [VTIMEOUT] (no result file"),
        "output: {text}"
    );
}

#[test]
fn task_hook_payload_adds_outcome_and_verification_status() {
    let mut task = task_fixture("t-hook-timeout", "hook task", None, None);
    task.verify = Some("cargo test".to_string());
    task.verify_status = VerifyStatus::TimedOut;

    let payload = task_hook_json(&task, "codex", Some("/repo"));

    assert_eq!(payload["status"], "done");
    assert_eq!(payload["outcome"], "unverified");
    assert_eq!(payload["verify_status"], "timed_out");
}

#[test]
fn audit_text_shows_failed_status_when_result_file_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task_fixture("t-failed-no-result", "investigate issue", None, None);
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();

    let text = audit_text(&store, task.id.as_str()).unwrap();

    // The summary header also prints "Status: FAILED", so anchor on the notice's
    // own line or a broken notice still satisfies this.
    assert!(text.contains("Status: FAILED\n"), "output: {text}");
}

#[test]
fn audit_text_shows_full_failure_details() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = research_task("t-show-failure-details", Path::new("."));
    task.status = TaskStatus::Failed;
    store.insert_task(&task).unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: "Failed during execution: agent exited with code 1\nFailure context: working directory: /tmp/demo; agent binary: /bin/sh; worktree path: /Users/test/.aid/worktrees/demo/feat/demo; worktree created: true".to_string(),
            metadata: None,
        })
        .unwrap();

    let text = audit_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("Failure details:"));
    assert!(text.contains("Failed during execution: agent exited with code 1"));
    assert!(text.contains("Failure context: working directory: /tmp/demo; agent binary: /bin/sh; worktree path: /Users/test/.aid/worktrees/demo/feat/demo; worktree created: true"));
}

#[test]
fn task_json_includes_pending_reason() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = research_task("t-show-json", Path::new("."));
    let output_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(output_file.path(), "full output\n").unwrap();
    task.pending_reason = Some("unknown".to_string());
    task.output_path = Some(output_file.path().display().to_string());
    store.insert_task(&task).unwrap();

    let payload: serde_json::Value =
        serde_json::from_str(&task_json(&store, task.id.as_str()).unwrap()).unwrap();

    assert_eq!(payload["pending_reason"], "unknown");
    assert_eq!(payload["output"], "full output\n");
}

#[test]
fn task_json_includes_delivery_assessment() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = research_task("t-show-delivery", Path::new("."));
    task.delivery_assessment = Some(crate::types::DeliveryAssessment::EmptyDiff);
    store.insert_task(&task).unwrap();

    let payload: serde_json::Value =
        serde_json::from_str(&task_json(&store, task.id.as_str()).unwrap()).unwrap();

    assert_eq!(payload["delivery_assessment"], "empty_diff");
}

#[test]
fn task_json_does_not_report_unobserved_as_success() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = research_task("t-show-unobs", Path::new("."));
    task.verify_status = VerifyStatus::Unobserved;
    store.insert_task(&task).unwrap();

    let payload: serde_json::Value =
        serde_json::from_str(&task_json(&store, task.id.as_str()).unwrap()).unwrap();

    assert_eq!(payload["status"], "done");
    assert_eq!(payload["verify_status"], "unobserved");
    assert_eq!(payload["outcome"], "unverified");
    assert_ne!(payload["outcome"], "delivered");
}

#[test]
fn result_text_reads_task_result_file() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let path = crate::support::paths::task_dir("t-result").join("result.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "structured result\n").unwrap();

    let text = result_text(&store, "t-result").unwrap();

    assert_eq!(text, "structured result\n");
}

#[test]
fn render_mode_text_reads_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-show-transcript", "raw prompt", None, None);
    store.insert_task(&task).unwrap();
    let transcript = crate::support::paths::transcript_path(task.id.as_str());
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "raw transcript\n").unwrap();

    let text = render_mode_text(&store, task.id.as_str(), ShowMode::Transcript).unwrap();

    assert_eq!(text, "raw transcript\n");
}

fn init_git_repo(repo: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::fs::write(repo.join("note.txt"), "baseline\n").unwrap();
    Command::new("git")
        .args(["add", "note.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(repo)
        .output()
        .unwrap();
}

fn save_completion_summary(store: &Store, task_id: &str, conclusion: &str) {
    let summary = CompletionSummary {
        task_id: task_id.to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec![],
        summary_text: "research task completed".to_string(),
        conclusion: conclusion.to_string(),
        duration_secs: None,
        token_count: None,
    };
    store
        .save_completion_summary(task_id, &serde_json::to_string(&summary).unwrap())
        .unwrap();
}
