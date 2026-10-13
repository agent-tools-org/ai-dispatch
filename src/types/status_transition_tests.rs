// Tests for lifecycle status capabilities and terminal failure immutability.
// Deps: TaskStatus; loaded by the types module only during tests.

use crate::types::TaskStatus;

#[test]
fn task_status_transition_graph_matches_expected_edges() {
    let legal = [
        (TaskStatus::Waiting, TaskStatus::Pending),
        (TaskStatus::Waiting, TaskStatus::Running),
        (TaskStatus::Waiting, TaskStatus::Skipped),
        (TaskStatus::Waiting, TaskStatus::Failed),
        (TaskStatus::Waiting, TaskStatus::Stopped),
        (TaskStatus::Pending, TaskStatus::Running),
        (TaskStatus::Pending, TaskStatus::Skipped),
        (TaskStatus::Pending, TaskStatus::Failed),
        (TaskStatus::Pending, TaskStatus::Stopped),
        (TaskStatus::Running, TaskStatus::AwaitingInput),
        (TaskStatus::Running, TaskStatus::Stalled),
        (TaskStatus::Running, TaskStatus::Done),
        (TaskStatus::Running, TaskStatus::Failed),
        (TaskStatus::Running, TaskStatus::Stopped),
        (TaskStatus::AwaitingInput, TaskStatus::Running),
        (TaskStatus::AwaitingInput, TaskStatus::Stalled),
        (TaskStatus::AwaitingInput, TaskStatus::Done),
        (TaskStatus::AwaitingInput, TaskStatus::Failed),
        (TaskStatus::AwaitingInput, TaskStatus::Stopped),
        (TaskStatus::Stalled, TaskStatus::Running),
        (TaskStatus::Stalled, TaskStatus::Done),
        (TaskStatus::Stalled, TaskStatus::Failed),
        (TaskStatus::Stalled, TaskStatus::Stopped),
        (TaskStatus::Done, TaskStatus::Merged),
        (TaskStatus::Done, TaskStatus::Failed),
        (TaskStatus::Failed, TaskStatus::Merged),
        (TaskStatus::Stopped, TaskStatus::Merged),
    ];
    for current in TaskStatus::ALL {
        for next in TaskStatus::ALL {
            let expected = current == next || legal.contains(&(current, next));
            assert_eq!(
                current.can_transition_to(next),
                expected,
                "{} -> {}",
                current.as_str(),
                next.as_str()
            );
        }
    }
}

#[test]
fn terminal_failure_cannot_be_refined_to_done() {
    assert!(!TaskStatus::Failed.can_transition_to(TaskStatus::Done));
    assert!(!TaskStatus::Merged.can_transition_to(TaskStatus::Done));
    assert!(!TaskStatus::Stopped.can_transition_to(TaskStatus::Done));
}
