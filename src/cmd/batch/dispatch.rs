// Batch dispatch orchestration (parallel/sequential, deps, fallback).
// Exports: dispatch_parallel, dispatch_sequential, dispatch_parallel_with_dependencies, dispatch_sequential_with_dependencies
// Deps: crate::batch, crate::cmd::run, crate::store::Store, super::{batch_args,batch_helpers,batch_types,batch_validate}
use crate::batch;
use crate::store::Store;
use crate::types::Task;
#[path = "dispatch/progress.rs"]
mod progress;
use anyhow::Result;
pub(crate) use progress::completion_progress_label;
use std::sync::Arc;
#[path = "dispatch_concurrency.rs"]
mod batch_dispatch_concurrency;
use self::batch_dispatch_concurrency::effective_max_active;
use super::batch_dispatch_support::{
    dispatch_level_with_ids, maybe_dispatch_auto_fallback, poll_completed_tasks,
};
use super::batch_helpers::{resolve_hook_targets, trigger_conditional};
use super::batch_types::{BatchDispatchResult, BatchTaskOutcome};
use super::batch_validate::{find_ready_tasks, resolve_dependencies};
use super::batch_wait_timeout::ReadyWaitTracker;

pub(crate) async fn dispatch_parallel(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(
        store,
        tasks,
        &dependencies,
        max_concurrent,
        auto_fallback,
        shared_dir_path,
        repo_root,
    )
    .await
}
pub(crate) async fn dispatch_sequential(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(
        store,
        tasks,
        &dependencies,
        Some(1),
        auto_fallback,
        shared_dir_path,
        repo_root,
    )
    .await
}
pub(crate) async fn dispatch_parallel_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(
        store,
        tasks,
        &dependencies,
        max_concurrent,
        auto_fallback,
        shared_dir_path,
        repo_root,
    )
    .await
}
pub(crate) async fn dispatch_sequential_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(
        store,
        tasks,
        &dependencies,
        Some(1),
        auto_fallback,
        shared_dir_path,
        repo_root,
    )
    .await
}
pub(super) fn reconcile_and_poll_completed_tasks(
    store: &Arc<Store>,
    active: &mut Vec<(usize, String)>,
) -> Result<Vec<super::batch_types::CompletedTask>> {
    let _ = crate::runtime::background::check_zombie_tasks(store.as_ref())?;
    poll_completed_tasks(store, active)
}

fn warn_rate_limited_agents(tasks: &[batch::BatchTask]) {
    let mut rate_warned = std::collections::HashSet::new();
    for task in tasks {
        let agent_name = if task.agent.is_empty() {
            "codex"
        } else {
            task.agent.as_str()
        };
        let (kind, custom_name) = crate::routing::rate_limit::resolve_agent(agent_name);
        if !crate::routing::rate_limit::is_rate_limited(&kind, custom_name)
            || !rate_warned.insert(agent_name.to_string())
        {
            continue;
        }
        if task.fallback.is_some() {
            aid_warn!(
                "[batch] {} is rate-limited — tasks with fallback will auto-cascade",
                agent_name
            );
        } else {
            aid_warn!(
                "[batch] {} is rate-limited — tasks without fallback may fail. Consider adding fallback.",
                agent_name
            );
        }
    }
}

#[path = "dispatch/dependencies.rs"]
mod dependencies;
use dependencies::dispatch_with_dependencies;
