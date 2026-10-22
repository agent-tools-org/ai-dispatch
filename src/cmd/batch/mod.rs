// Batch dispatch command for running tasks from a TOML file.
// Exports: BatchArgs, run()
// Deps: crate::batch, crate::cmd::run, crate::cmd::batch_validate, crate::store::Store
use crate::batch;
use crate::cmd::run;
use crate::store::Store;
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc, time::Instant};
#[path = "analyze.rs"]
mod batch_analyze;
#[path = "args.rs"]
mod batch_args;
#[path = "dispatch.rs"]
mod batch_dispatch;
#[path = "dispatch_support.rs"]
mod batch_dispatch_support;
#[path = "helpers.rs"]
mod batch_helpers;
#[path = "init.rs"]
mod batch_init;
#[path = "output_dedup.rs"]
mod batch_output_dedup;
#[path = "profile.rs"]
mod batch_profile;
#[path = "retry.rs"]
mod batch_retry;
#[path = "types.rs"]
mod batch_types;
#[path = "validate.rs"]
mod batch_validate;
#[path = "wait_timeout.rs"]
mod batch_wait_timeout;

use batch_analyze::OverlapSeverity;
#[cfg(test)]
pub(crate) use batch_dispatch::completion_progress_label;
#[cfg(test)]
pub(crate) use batch_dispatch_support::{
    auto_fallback_args, pre_dispatch_fallback_choice, should_auto_fallback,
};
use batch_helpers::{
    finalize_batch, parse_cli_vars, resolve_config_path, warn_nested_repo_for_batch,
};
pub use batch_retry::retry_failed;
#[cfg(test)]
pub(crate) use batch_types::BatchTaskOutcome;
use batch_validate::{analyze_file_overlap, task_has_dependencies, validate_batch_config};
pub struct BatchArgs {
    pub file: String,
    pub vars: Vec<String>,
    pub group: Option<String>,
    pub repo_root: Option<String>,
    pub parallel: bool,
    pub analyze: bool,
    pub wait: bool,
    pub dry_run: bool,
    pub no_prompt: bool,
    pub yes: bool,
    pub force: bool,
    pub max_concurrent: Option<usize>,
}

pub fn init(output_path: Option<&str>) -> Result<()> {
    batch_init::init(output_path)
}

pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    let (resolved_path, mut config) = load_batch_config(&args)?;
    let path = resolved_path.as_path();
    let max_concurrent = resolve_batch_concurrency(&args, &config)?;
    let total = config.tasks.len();
    let shared_dir_enabled = config.defaults.shared_dir.unwrap_or(false);
    let (explicit_repo_path, batch_repo_path) = resolve_batch_repository(&args, &config, path)?;
    prepare_batch_tasks(&args, &mut config, &explicit_repo_path)?;
    let has_dependencies = config.tasks.iter().any(task_has_dependencies);
    assign_batch_groups(&store, &args, &mut config, path, total, shared_dir_enabled)?;
    let shared_dir_path = batch_shared_directory(&config, shared_dir_enabled);
    if args.dry_run {
        preview_batch(
            &store,
            &config,
            path,
            shared_dir_path.as_deref(),
            &explicit_repo_path,
        )
        .await?;
        return Ok(());
    }
    warn_batch_conflicts(&args, &config)?;
    println!("Batch: dispatching {total} task(s) from {}", path.display());
    let start_time = Instant::now();
    let dispatch = dispatch_batch(
        &store,
        &config,
        &args,
        has_dependencies,
        max_concurrent,
        shared_dir_path.as_deref(),
        explicit_repo_path.as_deref(),
    )
    .await?;
    let task_ids = dispatch.dispatched_task_ids();
    let wait_error = wait_for_batch(&store, &args, has_dependencies, &task_ids).await?;
    finalize_batch(
        path,
        &dispatch,
        &task_ids,
        &config,
        &store,
        start_time,
        batch_repo_path.as_deref(),
        wait_error,
        total,
    )
}

pub(super) fn batch_wait_error(outcome: crate::cmd::wait::WaitOutcome) -> Option<anyhow::Error> {
    match outcome {
        crate::cmd::wait::WaitOutcome::Completed => None,
        crate::cmd::wait::WaitOutcome::Failed(task_ids) => Some(anyhow::anyhow!(
            "Batch task(s) did not succeed: {}",
            task_ids.join(", ")
        )),
        crate::cmd::wait::WaitOutcome::TimedOut(task_ids) => Some(anyhow::anyhow!(
            "Batch wait timed out with task(s) running: {}",
            task_ids.join(", ")
        )),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod batch_tests;

#[path = "orchestration/configuration.rs"]
mod configuration;
use configuration::{
    assign_batch_groups, batch_shared_directory, load_batch_config, prepare_batch_tasks,
    resolve_batch_concurrency, resolve_batch_repository, wait_for_batch,
};

#[path = "orchestration/execution.rs"]
mod execution;
use execution::{dispatch_batch, preview_batch, warn_batch_conflicts};
