// Named task-status sets shared by store and lifecycle code.
// Exports active-task and active-execution failure state lists.
// Deps: TaskStatus enum.

use super::TaskStatus;

pub const ACTIVE_TASK_STATUSES: [TaskStatus; 5] = [
    TaskStatus::Waiting,
    TaskStatus::Pending,
    TaskStatus::Running,
    TaskStatus::AwaitingInput,
    TaskStatus::Stalled,
];

pub const ACTIVE_EXECUTION_FAILURE_STATUSES: [TaskStatus; 4] = [
    TaskStatus::Waiting,
    TaskStatus::Running,
    TaskStatus::AwaitingInput,
    TaskStatus::Stalled,
];
