// Task fixture shared by stop command and settlement regressions.
// Deps: task record types and the local clock.

use crate::types::{Task, TaskId, TaskStatus};


pub(crate) fn make_task(id: &str, status: TaskStatus) -> Task {
    crate::types::Task::test_fixture(TaskId(id.to_string()), status)
}
