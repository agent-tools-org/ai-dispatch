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
