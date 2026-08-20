// Lightweight task diff presence checks for fleet payload enrichment.
// Exports: has_non_empty_diff from persisted delivery snapshot facts.
// Deps: Task and delivery assessment state.

use crate::types::{DeliveryAssessment, Task};

pub(crate) fn has_non_empty_diff(task: &Task) -> bool {
    if task.delivery_assessment.is_some_and(DeliveryAssessment::implies_no_changes) {
        return false;
    }
    task.worktree_path.is_some() || task.repo_path.is_some()
}
