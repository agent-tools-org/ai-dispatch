// Project-level declared-profile enforcement for batch tasks.
// Exports: validate_required_profiles().
// Deps: batch task schema and detected project policy.

use anyhow::Result;

use crate::batch::BatchTask;

pub(super) fn validate_required_profiles(tasks: &[BatchTask]) -> Result<()> {
    let required = crate::project::detect_project()
        .is_some_and(|project| project.require_task_profile);
    if !required {
        return Ok(());
    }
    for (index, task) in tasks.iter().enumerate() {
        let complete = task.difficulty.is_some()
            && task.budget.is_some()
            && task.urgency.is_some()
            && task.rigor.is_some();
        if !complete {
            let label = task.name.as_deref().unwrap_or("unnamed");
            anyhow::bail!(
                "Task profile is required for batch task {label} (index {index}); declare difficulty, budget, urgency, and rigor"
            );
        }
    }
    Ok(())
}
