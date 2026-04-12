// Tests for verify helper output excerpts and dependency hints.
// Exports: none.
// Deps: super, crate::store, crate::types, tempfile.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskStatus, VerifyStatus};
use chrono::Local;
use tempfile::TempDir;

fn make_task(id: &str, worktree_path: &str) -> Task {
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
        worktree_path: Some(worktree_path.to_string()),
        worktree_branch: Some("feat/test".to_string()),
        start_sha: None,
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
        verify: Some("false".to_string()),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
    }
}

#[test]
fn verify_output_excerpt_keeps_last_lines() {
    let output = (1..=10)
        .map(|idx| format!("line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");

    let excerpt = verify_output_excerpt(&output).unwrap();

    assert_eq!(
        excerpt,
        "line 3 | line 4 | line 5 | line 6 | line 7 | line 8 | line 9 | line 10"
    );
}

#[test]
fn maybe_verify_records_missing_deps_hint_for_fresh_worktree() {
    let store = Store::open_memory().unwrap();
    let worktree = TempDir::new().unwrap();
    let worktree_str = worktree.path().to_string_lossy().to_string();
    crate::worktree_deps::prepare_worktree_dependencies(
        &store,
        &TaskId("t-verify-hint".to_string()),
        worktree.path(),
        worktree.path(),
        None,
        false,
        None,
        true,
    )
    .unwrap();
    store
        .insert_task(&make_task("t-verify-hint", &worktree_str))
        .unwrap();

    maybe_verify_impl(
        &store,
        &TaskId("t-verify-hint".to_string()),
        Some("false"),
        Some(&worktree_str),
        None,
    );

    let events = store.get_events("t-verify-hint").unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("verify likely failed because dependencies weren't installed")
    }));
}

#[test]
fn maybe_verify_reports_stale_worktree_when_dir_is_missing() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-stale-verify".to_string());
    let mut task = make_task("t-stale-verify", "/tmp/aid-wt-feat-stale");
    task.workgroup_id = Some("wg-stale".to_string());
    task.worktree_branch = Some("feat/stale".to_string());
    task.verify = Some("auto".to_string());
    task.verify_status = VerifyStatus::Skipped;
    store.insert_task(&task).unwrap();

    maybe_verify_impl(
        &store,
        &task_id,
        Some("auto"),
        Some("/tmp/aid-wt-feat-stale/.aid/batches"),
        None,
    );

    let error = store.latest_error(task_id.as_str()).unwrap();
    assert!(error.contains("batch file / task dir missing in worktree"));
    assert!(error.contains("aid worktree remove feat/stale"));
}
