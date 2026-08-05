// Nested-agent delegation: parent/depth/env wiring and profile ceilings.
// Exports: apply_nested_delegation(), task_depth(), export depth helpers.
// Deps: RunArgs, Store, declared TaskDifficulty/TaskBudget ranks.

use anyhow::Result;

use super::RunArgs;
use crate::store::Store;

/// Maximum inclusive depth for nested `aid run` from an agent process.
pub(crate) const MAX_TASK_DEPTH: u32 = 2;

/// Apply AID_TASK_ID nesting rules before a task is claimed.
/// Only activates when the dispatcher itself runs inside an agent process.
pub(crate) fn apply_nested_delegation(store: &Store, args: &mut RunArgs) -> Result<()> {
    let Ok(env_parent) = std::env::var("AID_TASK_ID") else {
        return Ok(());
    };
    if env_parent.trim().is_empty() {
        return Ok(());
    }
    if args.parent_task_id.is_none() {
        args.parent_task_id = Some(env_parent.clone());
    }
    if args.background {
        anyhow::bail!(
            "delegated child tasks cannot use --bg; they must run synchronously so the parent worktree stays leased"
        );
    }
    let parent_depth = std::env::var("AID_TASK_DEPTH")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_else(|| task_depth(store, &env_parent).unwrap_or(0));
    let child_depth = parent_depth.saturating_add(1);
    if child_depth > MAX_TASK_DEPTH {
        anyhow::bail!(
            "refusing nested dispatch at depth {child_depth} (max {MAX_TASK_DEPTH}); flatten the work or finish the parent first"
        );
    }
    enforce_profile_ceiling(store, &env_parent, args)
}

/// Count ancestors for `task_id` (root = 0).
pub(crate) fn task_depth(store: &Store, task_id: &str) -> Result<u32> {
    let mut depth = 0u32;
    let mut current = store.get_task(task_id)?.and_then(|task| task.parent_task_id);
    while let Some(parent_id) = current {
        depth = depth.saturating_add(1);
        if depth > MAX_TASK_DEPTH.saturating_add(8) {
            break;
        }
        current = store.get_task(&parent_id)?.and_then(|task| task.parent_task_id);
    }
    Ok(depth)
}

fn enforce_profile_ceiling(store: &Store, parent_id: &str, args: &RunArgs) -> Result<()> {
    let parent = store.get_task_profile(parent_id)?;
    if let (Some(child), Some(parent_diff)) = (args.declared_difficulty, parent.difficulty)
        && child.rank() > parent_diff.rank()
    {
        anyhow::bail!(
            "child --difficulty {} exceeds parent {} ({parent_id}); delegation may only go down the ladder",
            child.label(),
            parent_diff.label()
        );
    }
    if let (Some(child), Some(parent_budget)) = (args.declared_budget, parent.budget)
        && child.rank() > parent_budget.rank()
    {
        anyhow::bail!(
            "child --budget {} exceeds parent {} ({parent_id}); delegation may only go down the ladder",
            child.label(),
            parent_budget.label()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "run_delegation_tests.rs"]
mod tests;
