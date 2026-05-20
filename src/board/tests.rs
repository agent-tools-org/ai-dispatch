// Tests for text board and detail rendering.
// Covers row status decorations, ETA, repository columns, and task detail output.
// Deps: parent board module, Store, and task/event fixtures.

use super::*;
use chrono::Local;
use serde_json::json;
use tempfile::TempDir;

use crate::paths::AidHomeGuard;
use crate::store::Store;

fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "test prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: Some("feat/test".to_string()),
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(45000),
        prompt_tokens: None,
        duration_ms: Some(227000),
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
        delivery_assessment: None,
    }
}

fn isolated_store() -> (TempDir, AidHomeGuard, Store) {
    let temp = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    (temp, guard, store)
}

#[test]
fn empty_board() {
    let (_temp, _guard, store) = isolated_store();
    assert_eq!(render_board(&[], &store).unwrap(), "No tasks found.");
}

#[test]
fn board_with_tasks() {
    let (_temp, _guard, store) = isolated_store();
    let tasks = vec![
        make_task("t-0001", AgentKind::Codex, TaskStatus::Done),
        make_task("t-0002", AgentKind::Gemini, TaskStatus::Running),
    ];
    let output = render_board(&tasks, &store).unwrap();
    assert!(output.contains("t-0001"));
    assert!(output.contains("codex"));
    assert!(output.contains("DONE"));
    assert!(output.contains("RUN"));
    assert!(output.contains("2 total"));
    assert!(output.contains("Cost"));
    assert!(output.contains("Caller"));
    assert!(output.contains("Group"));
}

#[test]
fn board_shows_empty_diff_delivery_assessment() {
    let (_temp, _guard, store) = isolated_store();
    let mut task = make_task("t-empty", AgentKind::Codex, TaskStatus::Done);
    task.delivery_assessment = Some(DeliveryAssessment::EmptyDiff);

    let output = render_board(&[task], &store).unwrap();

    assert!(output.contains("[delivery:empty_diff]"), "output: {output}");
}

#[test]
fn board_shows_hollow_output_delivery_assessment() {
    let (_temp, _guard, store) = isolated_store();
    let mut task = make_task("t-hollow", AgentKind::Codex, TaskStatus::Done);
    task.delivery_assessment = Some(DeliveryAssessment::HollowOutput);

    let output = render_board(&[task], &store).unwrap();

    assert!(output.contains("[delivery:hollow_output]"), "output: {output}");
}

#[test]
fn board_shows_running_task_milestone() {
    let (_temp, _guard, store) = isolated_store();
    let task = make_task("t-0003", AgentKind::Codex, TaskStatus::Running);
    store.insert_task(&task).unwrap();
    store.insert_event(&TaskEvent {
        task_id: task.id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail: "types defined".to_string(),
        metadata: None,
    }).unwrap();

    let output = render_board(&[task], &store).unwrap();
    assert!(output.contains("RUN — types defined"));
}

#[test]
fn board_shows_awaiting_input_reason() {
    let (_temp, _guard, store) = isolated_store();
    let task = make_task("t-0004", AgentKind::Codex, TaskStatus::AwaitingInput);
    store.insert_task(&task).unwrap();
    store.insert_event(&TaskEvent {
        task_id: task.id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Reasoning,
        detail: "115:    use super::board::render_board;".to_string(),
        metadata: Some(json!({ "awaiting_input": true, "awaiting_prompt": "Continue with fix?" })),
    }).unwrap();

    let output = render_board(&[task], &store).unwrap();
    assert!(output.contains("AWAIT — Continue with fix?"));
    assert!(!output.contains("115:    use super::board::render_board;"));
}

#[test]
fn board_shows_repo_column_when_present() {
    let (_temp, _guard, store) = isolated_store();
    let mut task = make_task("t-0005", AgentKind::Codex, TaskStatus::Done);
    task.repo_path = Some("/tmp/example-repo".to_string());

    let output = render_board(&[task], &store).unwrap();
    assert!(output.contains("Repo"));
    assert!(output.contains("/tmp/example-repo"));
}

#[test]
fn board_shows_pending_reason_for_failed_pending_timeout() {
    let (_temp, _guard, store) = isolated_store();
    let mut task = make_task("t-0006", AgentKind::Codex, TaskStatus::Failed);
    task.pending_reason = Some("rate_limited".to_string());

    let output = render_board(&[task], &store).unwrap();
    assert!(output.contains("FAIL — rate_limited"));
}

#[test]
fn board_shows_latest_error_for_failed_task() {
    let (_temp, _guard, store) = isolated_store();
    let task = make_task("t-err1", AgentKind::Codex, TaskStatus::Failed);
    store.insert_task(&task).unwrap();
    store.insert_event(&TaskEvent {
        task_id: TaskId("t-err1".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: "Quota exhausted".to_string(),
        metadata: None,
    }).unwrap();
    let output = render_board(&[task], &store).unwrap();
    assert!(output.contains("FAIL — Quota exhausted"), "output: {output}");
}

#[test]
fn test_board_shows_eta_for_running_task() {
    let (_temp, _guard, store) = isolated_store();
    let now = Local::now();
    for (id, minutes_ago, duration_ms) in [
        ("t-done-1", 10, 120_000),
        ("t-done-2", 20, 180_000),
        ("t-done-3", 30, 240_000),
    ] {
        let mut task = make_task(id, AgentKind::Codex, TaskStatus::Done);
        task.created_at = now - chrono::Duration::minutes(minutes_ago);
        task.duration_ms = Some(duration_ms);
        store.insert_task(&task).unwrap();
    }

    let mut running = make_task("t-run", AgentKind::Codex, TaskStatus::Running);
    running.created_at = now - chrono::Duration::seconds(90);
    running.duration_ms = None;
    store.insert_task(&running).unwrap();

    let output = render_board(&[running], &store).unwrap();
    assert!(output.contains("ETA"), "output: {output}");
    assert!(output.contains('%'), "output: {output}");
}

#[test]
fn task_detail_rendering() {
    let task = make_task("t-0001", AgentKind::Codex, TaskStatus::Done);
    let events = vec![TaskEvent {
        task_id: TaskId("t-0001".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::ToolCall,
        detail: "exec: cargo test".to_string(),
        metadata: None,
    }];
    let output = render_task_detail(&task, &events, None);
    assert!(output.contains("t-0001"));
    assert!(output.contains("cargo test"));
}

#[test]
fn task_detail_shows_pending_reason() {
    let mut task = make_task("t-0007", AgentKind::Codex, TaskStatus::Failed);
    task.pending_reason = Some("worker_capacity".to_string());

    let output = render_task_detail(&task, &[], None);
    assert!(output.contains("Pending reason: worker_capacity"));
}

#[test]
fn task_detail_shows_retry_chain() {
    let mut root = make_task("t-1001", AgentKind::Codex, TaskStatus::Done);
    root.duration_ms = Some(12_000);
    root.cost_usd = Some(0.03);
    let mut retry_1 = make_task("t-1002", AgentKind::Codex, TaskStatus::Failed);
    retry_1.parent_task_id = Some("t-1001".to_string());
    retry_1.duration_ms = Some(8_000);
    retry_1.cost_usd = Some(0.02);
    let mut retry_2 = make_task("t-1003", AgentKind::Codex, TaskStatus::Done);
    retry_2.parent_task_id = Some("t-1002".to_string());
    retry_2.duration_ms = Some(15_000);
    retry_2.cost_usd = Some(0.04);

    let output = render_task_detail(&retry_2, &[], Some(vec![root, retry_1, retry_2.clone()]));
    assert!(output.contains("Retry chain:"));
    assert!(output.contains("t-1001 (root)  → Done"));
    assert!(output.contains("t-1002 (retry)  → Failed"));
    assert!(output.contains("t-1003 (retry)  → Done"));
    assert!(output.contains("← current"));
}

#[test]
fn task_detail_shows_iteration_eval_output() {
    let task = make_task("t-iter", AgentKind::Codex, TaskStatus::Done);
    let output = render_task_detail(
        &task,
        &[TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "Iteration 1/3: eval failed (exit 1), retrying as t-next".to_string(),
            metadata: Some(json!({
                "iterate": {
                    "iteration": 1,
                    "max_iterations": 3,
                    "eval_output": "cargo test failed"
                }
            })),
        }],
        None,
    );

    assert!(output.contains("Iteration 1/3: eval failed"));
    assert!(output.contains("Eval output: cargo test failed"));
}
