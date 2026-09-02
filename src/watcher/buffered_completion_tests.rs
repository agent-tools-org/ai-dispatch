// Integration tests for buffered completion evidence across stdout and agent diagnostics.
// Covers exit-zero agy failures; depends on watch_buffered, Store, and real log fixtures.

use super::watch_buffered;
use crate::agent::antigravity::AntigravityAgent;
use crate::paths;
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::process::Stdio;
use std::sync::Arc;

fn buffered_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Antigravity,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: Some("claude-opus-4-6-thinking".to_string()),
        observed_model: None,
        attribution_source: None,
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

#[tokio::test]
async fn agy_exit_zero_with_terminal_agent_error_fails_completion() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task = buffered_task("t-agy-exit-zero-terminal-error");
    store.insert_task(&task).unwrap();
    std::fs::create_dir_all(paths::task_dir(task.id.as_str())).unwrap();
    std::fs::write(
        paths::agent_log_path(task.id.as_str()),
        include_str!("../../tests/fixtures/agy-exit0-terminal-error.log"),
    )
    .unwrap();
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("printf 'Now let me inspect the remaining call sites:\n'; exit 0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_buffered(
        &AntigravityAgent,
        &mut child,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.exit_code, Some(0));
    assert_eq!(info.status, TaskStatus::Failed);
    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Error
            && event.detail.contains("terminal executor failure")
    }));
    assert!(rate_limit::is_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "claude"
    ));
}

#[tokio::test]
async fn agy_exit_zero_with_non_quota_executor_error_fails_completion() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task = buffered_task("t-agy-exit-zero-network-error");
    store.insert_task(&task).unwrap();
    std::fs::create_dir_all(paths::task_dir(task.id.as_str())).unwrap();
    std::fs::write(
        paths::agent_log_path(task.id.as_str()),
        include_str!("../../tests/fixtures/agy-exit0-terminal-network-error.log"),
    )
    .unwrap();
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("printf 'Now let me inspect the remaining call sites:\n'; exit 0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_buffered(
        &AntigravityAgent,
        &mut child,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.exit_code, Some(0));
    assert_eq!(info.status, TaskStatus::Failed);
    assert!(!rate_limit::is_rate_limited(&AgentKind::Antigravity, None));
}

#[tokio::test]
async fn agy_recovered_tool_error_keeps_successful_completion() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task = buffered_task("t-agy-recovered-tool-error");
    store.insert_task(&task).unwrap();
    std::fs::create_dir_all(paths::task_dir(task.id.as_str())).unwrap();
    std::fs::write(
        paths::agent_log_path(task.id.as_str()),
        include_str!("../../tests/fixtures/agy-recovered-tool-error.log"),
    )
    .unwrap();
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("printf 'Audit complete. Overall verdict: SHIP.\n'; exit 0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_buffered(
        &AntigravityAgent,
        &mut child,
        &task.id,
        &store,
        &paths::log_path(task.id.as_str()),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.exit_code, Some(0));
    assert_eq!(info.status, TaskStatus::Done);
}
