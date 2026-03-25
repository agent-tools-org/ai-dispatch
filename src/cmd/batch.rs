// Batch dispatch command for running tasks from a TOML file.
// Exports: BatchArgs, run()
// Deps: crate::batch, crate::cmd::run, crate::cmd::batch_validate, crate::store::Store
use anyhow::{Context, Result};
use std::collections::HashMap;
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
#[path = "batch_dispatch_support.rs"]
mod batch_dispatch_support;
#[path = "batch_analyze.rs"]
mod batch_analyze;
#[path = "batch_helpers.rs"]
mod batch_helpers;
#[path = "batch_types.rs"]
mod batch_types;

use batch_validate::{analyze_file_overlap, task_has_dependencies, validate_batch_config};
#[cfg(test)]
pub(crate) use batch_dispatch_support::{auto_fallback_agent, pre_dispatch_fallback_choice, should_auto_fallback};
#[cfg(test)]
pub(crate) use batch_types::BatchTaskOutcome;
pub use batch_retry::retry_failed;
pub struct BatchArgs {
    pub file: String,
    pub vars: Vec<String>,
    pub group: Option<String>,
    pub parallel: bool,
    pub analyze: bool,
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
    let cli_vars = parse_cli_vars(&args.vars)?;
    let mut config = if cli_vars.is_empty() {
        batch::parse_batch_file(path)
    } else {
        batch::parse_batch_file_with_vars(path, &cli_vars)
    }
    .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();
    let shared_dir_enabled = config.defaults.shared_dir.unwrap_or(false);
    validate_batch_config(&config.tasks)?;
    let has_dependencies = config.tasks.iter().any(task_has_dependencies);
    let effective_group = args.group.as_ref().or(config.defaults.group.as_ref());
    if let Some(group) = effective_group {
        if store.get_workgroup(group)?.is_none() {
            anyhow::bail!(
                "Workgroup '{group}' not found. Create it with: aid group create --name <name> --id {group}"
            );
        }
        let source = if args.group.is_some() { "--group flag" } else { "[defaults] group" };
        for task in &mut config.tasks {
            if task.group.is_none() {
                task.group = Some(group.clone());
            }
        }
        aid_info!("[aid] Using workgroup {group} from {source}");
    }
    let no_groups_set = config.tasks.iter().all(|t| t.group.is_none());
    if no_groups_set {
        if let Ok(env_group) = std::env::var("AID_GROUP") {
            for task in &mut config.tasks {
                task.group = Some(env_group.clone());
            }
            aid_info!("[aid] Using workspace {env_group} from AID_GROUP");
        } else if total >= 2 {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
            let (wg_id, shared_path) = batch_helpers::ensure_batch_workgroup(
                &store,
                stem,
                config.defaults.group_id.as_deref(),
                shared_dir_enabled,
            )?;
            for task in &mut config.tasks {
                task.group = Some(wg_id.clone());
            }
            if let Some(shared_path) = shared_path {
                aid_info!("[aid] Shared batch dir: {}", shared_path.display());
            }
        }
    }
    let shared_dir_path = if shared_dir_enabled {
        config
            .tasks
            .first()
            .and_then(|task| task.group.as_deref())
            .and_then(crate::shared_dir::shared_dir_path)
            .map(|path| path.display().to_string())
    } else {
        None
    };
    if args.dry_run {
        println!("Batch: previewing {total} task(s) from {}", path.display());
        for (task_idx, task) in config.tasks.iter().enumerate() {
            let siblings: Vec<_> = config.tasks
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != task_idx)
                .map(|(_, sibling)| sibling)
                .collect();
            let mut run_args = batch_args::task_to_run_args(
                task,
                &siblings,
                false,
                &store,
                shared_dir_path.as_deref(),
            );
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
    if args.analyze || config.defaults.analyze.unwrap_or(false) {
        let overlaps = analyze_file_overlap(&config.tasks, &config.defaults);
        if !overlaps.is_empty() {
            aid_warn!("[aid] Warning: potential merge conflicts detected:");
            for overlap in overlaps {
                aid_warn!(
                    "  {} - referenced by: {}",
                    overlap.file,
                    overlap.task_ids.join(", ")
                );
            }
            aid_warn!("[aid] Consider adding dependencies or using --worktree isolation");
        }
    }
    println!("Batch: dispatching {total} task(s) from {}", path.display());
    let start_time = Instant::now();
    let auto_fallback = config.defaults.auto_fallback.unwrap_or(false)
        || config.tasks.iter().any(|t| t.fallback.is_some());
    let dispatch = if has_dependencies && args.parallel {
        batch_dispatch::dispatch_parallel_with_dependencies(
            store.clone(),
            &config.tasks,
            args.max_concurrent,
            auto_fallback,
            shared_dir_path.as_deref(),
        )
        .await?
    } else if has_dependencies {
        batch_dispatch::dispatch_sequential_with_dependencies(
            store.clone(),
            &config.tasks,
            auto_fallback,
            shared_dir_path.as_deref(),
        )
        .await?
    } else if args.parallel {
        batch_dispatch::dispatch_parallel(
            store.clone(),
            &config.tasks,
            args.max_concurrent,
            auto_fallback,
            shared_dir_path.as_deref(),
        )
        .await?
    } else {
        batch_dispatch::dispatch_sequential(
            store.clone(),
            &config.tasks,
            auto_fallback,
            shared_dir_path.as_deref(),
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

fn parse_cli_vars(raw_vars: &[String]) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for raw_var in raw_vars {
        let Some((key, value)) = raw_var.split_once('=') else {
            anyhow::bail!("invalid --var '{}': expected key=value", raw_var);
        };
        let key = key.trim();
        anyhow::ensure!(!key.is_empty(), "invalid --var '{}': key cannot be empty", raw_var);
        vars.insert(key.to_string(), value.to_string());
    }
    Ok(vars)
}
#[cfg(test)]
#[path = "batch_tests.rs"]
mod batch_tests;
