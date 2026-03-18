// Batch dispatch command for running tasks from a TOML file.
// Exports: BatchArgs, run()
// Deps: crate::batch, crate::cmd::run, crate::cmd::batch_validate, crate::store::Store
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc, time::Instant};
use crate::batch;
use crate::cmd::run;
use crate::store::Store;
#[path = "batch_validate.rs"]
mod batch_validate;
#[path = "batch_init.rs"]
mod batch_init;
#[path = "batch_args.rs"]
mod batch_args;
#[path = "batch_retry.rs"]
mod batch_retry;
#[path = "batch_dispatch.rs"]
mod batch_dispatch;
#[path = "batch_helpers.rs"]
mod batch_helpers;
#[path = "batch_types.rs"]
mod batch_types;

use batch_validate::{task_has_dependencies, validate_batch_config};
#[cfg(test)]
pub(crate) use batch_dispatch::{auto_fallback_agent, should_auto_fallback};
#[cfg(test)]
pub(crate) use batch_types::BatchTaskOutcome;
pub use batch_retry::retry_failed;
pub struct BatchArgs {
    pub file: String,
    pub parallel: bool,
    pub wait: bool,
    pub dry_run: bool,
    pub max_concurrent: Option<usize>,
}

pub fn init(output_path: Option<&str>) -> Result<()> {
    batch_init::init(output_path)
}

pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    if args.max_concurrent == Some(0) {
        anyhow::bail!("--max-concurrent must be at least 1");
    }
    let resolved_path = batch_helpers::resolve_batch_path(Path::new(&args.file));
    let path = resolved_path.as_path();
    let mut config = batch::parse_batch_file(path)
        .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();
    validate_batch_config(&config.tasks)?;
    let has_dependencies = config.tasks.iter().any(task_has_dependencies);
    let no_groups_set = config.tasks.iter().all(|t| t.group.is_none());
    if no_groups_set {
        if let Ok(env_group) = std::env::var("AID_GROUP") {
            for task in &mut config.tasks {
                task.group = Some(env_group.clone());
            }
            aid_info!("[aid] Using workspace {env_group} from AID_GROUP");
        } else if total >= 2 {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
            let wg_id = batch_helpers::ensure_batch_workgroup(
                &store,
                stem,
                config.defaults.group_id.as_deref(),
            )?;
            for task in &mut config.tasks {
                task.group = Some(wg_id.clone());
            }
        }
    }
    if args.dry_run {
        println!("Batch: previewing {total} task(s) from {}", path.display());
        for (task_idx, task) in config.tasks.iter().enumerate() {
            let siblings: Vec<_> = config.tasks
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != task_idx)
                .map(|(_, sibling)| sibling)
                .collect();
            let mut run_args = batch_args::task_to_run_args(task, &siblings, false, &store);
            run_args.dry_run = true;
            let _ = run::run(store.clone(), run_args).await?;
        }
        println!("Batch: {total} task(s) previewed");
        return Ok(());
    }
    if let Some(avail) = batch_helpers::low_disk_space_mb(500) {
        aid_warn!("[aid] Warning: low disk space ({avail} MB free) — parallel dispatch may fail");
    }
    batch_helpers::warn_for_rate_limited_agents(&config.tasks);
    println!("Batch: dispatching {total} task(s) from {}", path.display());
    let start_time = Instant::now();
    let dispatch = if has_dependencies && args.parallel {
        batch_dispatch::dispatch_parallel_with_dependencies(
            store.clone(),
            &config.tasks,
            args.max_concurrent,
            config.defaults.auto_fallback.unwrap_or(false),
        )
        .await?
    } else if has_dependencies {
        batch_dispatch::dispatch_sequential_with_dependencies(
            store.clone(),
            &config.tasks,
            config.defaults.auto_fallback.unwrap_or(false),
        )
        .await?
    } else if args.parallel {
        batch_dispatch::dispatch_parallel(
            store.clone(),
            &config.tasks,
            args.max_concurrent,
            config.defaults.auto_fallback.unwrap_or(false),
        )
        .await?
    } else {
        batch_dispatch::dispatch_sequential(
            store.clone(),
            &config.tasks,
            config.defaults.auto_fallback.unwrap_or(false),
        )
        .await?
    };
    let task_ids = dispatch.dispatched_task_ids();
    if args.wait && args.parallel && !has_dependencies && !task_ids.is_empty() {
        crate::cmd::wait::wait_for_task_ids(&store, &task_ids, false, None).await?;
    }
    aid_info!(
        "{}",
        batch_helpers::batch_summary(
            &dispatch.outcomes,
            &dispatch.task_ids,
            &config.tasks,
            &store,
            start_time,
        )
    );
    let archive_dir = crate::paths::aid_dir().join("batches");
    if let Err(e) = std::fs::create_dir_all(&archive_dir) {
        aid_error!("[aid] Failed to create batch archive dir: {e}");
    } else {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let dest = archive_dir.join(format!("{timestamp}-{stem}.toml"));
        match std::fs::copy(path, &dest) {
            Ok(_) => aid_info!("[aid] Archived batch to {}", dest.display()),
            Err(e) => aid_error!("[aid] Failed to archive batch: {e}"),
        }
    }
    println!("Batch: {total} task(s) dispatched");
    // Print watch hint for the caller
    let group_id = config.tasks.first().and_then(|t| t.group.as_deref());
    if let Some(gid) = group_id {
        aid_hint!("[aid] Watch: aid watch --quiet --group {gid}");
    } else if task_ids.len() == 1 {
        aid_hint!("[aid] Watch: aid watch --quiet {}", task_ids[0]);
    }
    aid_hint!("[aid] TUI:   aid watch --tui");
    Ok(())
}
#[cfg(test)]
#[path = "batch_tests.rs"]
mod batch_tests;
