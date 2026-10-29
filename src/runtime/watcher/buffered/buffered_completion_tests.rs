// Integration tests for buffered completion evidence across stdout and agent diagnostics.
// Covers exit-zero agy failures; depends on watch_buffered, Store, and real log fixtures.

use super::watch_buffered;
use crate::agent::antigravity::AntigravityAgent;
use crate::support::paths;
use crate::routing::rate_limit;
use crate::types::{AgentKind, EventKind, Task, TaskId, TaskStatus};

fn buffered_task(id: &str) -> Task {
    Task {
        agent: AgentKind::Antigravity,
        requested_model: Some("claude-opus-4-6-thinking".to_string()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Running)
    }
}

#[tokio::test]
async fn agy_exit_zero_with_terminal_agent_error_fails_completion() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = buffered_task("t-agy-exit-zero-terminal-error");
    store.insert_task(&task).unwrap();
    std::fs::create_dir_all(paths::task_dir(task.id.as_str())).unwrap();
    std::fs::write(
        paths::agent_log_path(task.id.as_str()),
        include_str!("../../../../tests/fixtures/agy-exit0-terminal-error.log"),
    )
    .unwrap();
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "printf 'Now let me inspect the remaining call sites:\n'; exit 0",
    );

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
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = buffered_task("t-agy-exit-zero-network-error");
    store.insert_task(&task).unwrap();
    std::fs::create_dir_all(paths::task_dir(task.id.as_str())).unwrap();
    std::fs::write(
        paths::agent_log_path(task.id.as_str()),
        include_str!("../../../../tests/fixtures/agy-exit0-terminal-network-error.log"),
    )
    .unwrap();
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "printf 'Now let me inspect the remaining call sites:\n'; exit 0",
    );

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
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = buffered_task("t-agy-recovered-tool-error");
    store.insert_task(&task).unwrap();
    std::fs::create_dir_all(paths::task_dir(task.id.as_str())).unwrap();
    std::fs::write(
        paths::agent_log_path(task.id.as_str()),
        include_str!("../../../../tests/fixtures/agy-recovered-tool-error.log"),
    )
    .unwrap();
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "printf 'Audit complete. Overall verdict: SHIP.\n'; exit 0",
    );

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
