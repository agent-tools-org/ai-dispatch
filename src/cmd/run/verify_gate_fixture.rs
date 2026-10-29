// Provisional task fixture for verification-gate tests.
// Deps: task record types and the local clock.

use crate::types::{Task, TaskId, TaskStatus};

pub(super) fn task(id: &str, status: TaskStatus, dir: Option<&str>, verify: Option<&str>) -> Task {
    Task {
        worktree_path: dir.map(str::to_string),
        worktree_branch: Some("fix/verify-gate".to_string()),
        duration_ms: Some(10),
        exit_code: Some(0),
        verify: verify.map(str::to_string),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}
