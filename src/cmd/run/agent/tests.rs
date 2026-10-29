// Tests for execution failure persistence and exit-code recovery.
// Deps: production run_prompt failure helpers, Store, and temporary directories.

use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus};
use chrono::Local;
use std::path::Path;

fn task_fixture(id: &str, status: TaskStatus, worktree_path: Option<&str>) -> Task {
    Task {
        worktree_path: worktree_path.map(str::to_string),
        worktree_branch: worktree_path.map(|_| "aid-test".to_string()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

#[test]
fn record_execution_failure_stores_phase_event_and_snapshot() {
    let (_aid_home, store) = crate::support::test_env::TestHome::with_dirs().store();
    let worktree_path = crate::workspace::worktree::aid_worktree_path(Path::new(env!("CARGO_MANIFEST_DIR")), "feat/fast-fail");
    let worktree_path = worktree_path.to_string_lossy().to_string();
    let task = task_fixture("t-fast-fail", TaskStatus::Running, Some(&worktree_path));
    store.insert_task(&task).unwrap();
    std::fs::write(
        crate::support::paths::stderr_path(task.id.as_str()),
        "spawn blew up\nsecondary detail\n",
    )
    .unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.current_dir(workdir.path());

    let context = super::super::run_prompt::capture_failure_context(&store, &task.id, &cmd);
    super::super::run_prompt::record_execution_failure(&store, &task.id, 1_200, Some(1), &context);

    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("Failed during execution: agent exited with code 1")
            && event.detail.contains("Stderr: spawn blew up | secondary detail")
    }));
    assert!(events.iter().any(|event| {
        event.detail.contains("Failure context: working directory:")
            && event.detail.contains("agent binary: /bin/sh")
            && event.detail.contains(&format!("worktree path: {worktree_path}"))
            && event.detail.contains("worktree created: true")
    }));
}

#[test]
fn resolve_failure_exit_code_reads_completion_event_detail() {
    let store = Store::open_memory().unwrap();
    let task = task_fixture("t-exit-code", TaskStatus::Failed, None);
    store.insert_task(&task).unwrap();
    store
        .insert_event(&crate::types::TaskEvent::new(
            task.id.clone(),
            crate::types::EventKind::Error,
            "FAIL — 0 events, exit code 7".to_string(),
        ))
        .unwrap();

    let exit_code =
        super::super::run_prompt::resolve_failure_exit_code(&store, &task.id, None);

    assert_eq!(exit_code, Some(7));
}
