// Tests for conditional trigger behavior in batch dispatch.
// Exports: (tests only)
// Deps: super::shared + batch_helpers
use super::shared::make_task;
use crate::store::Store;
use std::sync::Arc;

use super::super::batch_helpers::trigger_conditional;
use super::super::batch_types::BatchTaskOutcome;
use super::super::batch_validate::find_ready_tasks;

#[test]
fn trigger_success_marks_target() {
    let mut triggered = vec![true, false];
    let success_targets = vec![Some(1), None];
    let failure_targets = vec![None, None];
    trigger_conditional(
        BatchTaskOutcome::Done,
        0,
        &mut triggered,
        &success_targets,
        &failure_targets,
    );
    assert!(triggered[1]);
}

#[test]
fn trigger_failure_marks_target() {
    let mut triggered = vec![true, false];
    let success_targets = vec![None, None];
    let failure_targets = vec![Some(1), None];
    trigger_conditional(
        BatchTaskOutcome::Failed,
        0,
        &mut triggered,
        &success_targets,
        &failure_targets,
    );
    assert!(triggered[1]);
}

#[test]
fn conditional_task_stays_dormant_until_triggered() {
    let store = Arc::new(Store::open_memory().unwrap());
    let tasks = vec![make_task("first", false, Some("second")), make_task("second", true, None)];
    let deps = vec![Vec::new(), Vec::new()];
    let started = vec![false; 2];
    let mut outcomes = vec![None; 2];
    let triggered = vec![true, false];
    let ready =
        find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert_eq!(ready, vec![0]);
    let triggered = vec![true, true];
    let ready =
        find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert_eq!(ready, vec![0, 1]);
}

