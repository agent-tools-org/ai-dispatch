// Board outcome rendering regressions for inconclusive verification.
// Exports: module-scoped tests.
// Deps: board::render_board, Store, and task outcome types.

use super::render_board;
use chrono::Local;
use tempfile::TempDir;

use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

fn timed_out_task() -> Task {
    Task {
        id: TaskId("t-timeout".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test prompt".to_string(),
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
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: Some("cargo test".to_string()),
        verify_status: VerifyStatus::TimedOut,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn board_marks_timed_out_verification_without_counting_success() {
    let temp = TempDir::new().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();

    let output = render_board(&[timed_out_task()], &store).unwrap();

    assert!(output.contains("[VTIMEOUT]"), "output: {output}");
    assert!(output.contains("1 total | 0 done"), "output: {output}");
}

#[test]
fn board_omits_verification_tags_when_no_verification_result_exists() {
    let temp = TempDir::new().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let mut failed = timed_out_task();
    failed.id = TaskId("t-failed".to_string());
    failed.status = TaskStatus::Failed;
    failed.verify = None;
    failed.verify_status = VerifyStatus::Skipped;
    let mut running = timed_out_task();
    running.id = TaskId("t-running".to_string());
    running.status = TaskStatus::Running;
    running.verify = None;
    running.verify_status = VerifyStatus::Skipped;

    let output = render_board(&[failed, running], &store).unwrap();

    assert!(!output.contains("[VSKIPPED]"), "output: {output}");
    assert!(!output.contains("[V"), "output: {output}");
}

#[test]
fn board_counts_only_active_lifecycle_statuses_as_running() {
    let temp = TempDir::new().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let mut waiting = timed_out_task();
    waiting.id = TaskId("t-waiting".to_string());
    waiting.status = TaskStatus::Waiting;
    waiting.verify = None;
    waiting.verify_status = VerifyStatus::Skipped;
    let mut pending = waiting.clone();
    pending.id = TaskId("t-pending".to_string());
    pending.status = TaskStatus::Pending;
    let mut running = waiting.clone();
    running.id = TaskId("t-running".to_string());
    running.status = TaskStatus::Running;

    let output = render_board(&[pending, running, waiting], &store).unwrap();

    assert!(output.contains("3 total | 0 done | 1 running | 0 failed"), "output: {output}");
}
