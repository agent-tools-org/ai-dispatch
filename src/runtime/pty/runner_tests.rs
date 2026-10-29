// Tests for PTY-backed background agent execution.
// Covers immediate task failure when the agent process cannot spawn.

use super::run_agent_process_with_control;
use crate::agent::get_agent;
use crate::support::paths;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

#[test]
fn run_agent_process_marks_task_failed_when_spawn_fails() {
    let (_aid_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task_id = TaskId("t-kilo-missing".to_string());
    store
        .insert_task(&task_fixture(&task_id, TaskStatus::Running))
        .unwrap();
    let log_path = paths::log_path(task_id.as_str());
    let cmd = std::process::Command::new("/definitely/missing/kilo");

    let err = run_agent_process_with_control(
        false, get_agent(AgentKind::Kilo).as_ref(),
        &cmd,
        &task_id,
        &store,
        &log_path,
        None,
        None,
        true,
        crate::config::timeout_policy::TimeoutPolicy::from_command(&cmd),
        None,
        None,
        std::time::Instant::now(),
    )
    .err()
    .unwrap();

    assert!(err.to_string().contains("Failed to spawn agent process"));
    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert!(store
        .latest_error(task_id.as_str())
        .unwrap()
        .contains("Failed to spawn agent process"));
    assert!(std::fs::read_to_string(paths::stderr_path(task_id.as_str()))
        .unwrap()
        .contains("Failed to spawn agent process"));
}

pub(super) fn task_fixture(id: &TaskId, status: TaskStatus) -> Task {
    Task {
        agent: AgentKind::Kilo,
        ..crate::types::Task::test_fixture(id.clone(), status)
    }
}
