// Batch preview, conflict warnings, and dispatch selection.
// Deps: batch command types and dispatch helpers.
use super::*;

pub(super) async fn preview_batch(
    store: &Arc<Store>,
    config: &batch::BatchConfig,
    path: &Path,
    shared_dir_path: Option<&str>,
    explicit_repo_path: &Option<String>,
) -> Result<()> {
    let total = config.tasks.len();
    println!("Batch: previewing {total} task(s) from {}", path.display());
    for (task_idx, task) in config.tasks.iter().enumerate() {
        let siblings: Vec<_> = config
            .tasks
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != task_idx)
            .map(|(_, sibling)| sibling)
            .collect();
        let mut run_args =
            batch_args::task_to_run_args(task, &siblings, false, &store, shared_dir_path);
        run_args.repo_root = explicit_repo_path.clone();
        run_args.suppress_nested_repo_warning = true;
        run_args.dry_run = true;
        let _ = run::run(store.clone(), run_args).await?;
    }
    println!("Batch: {total} task(s) previewed");
    Ok(())
}

pub(super) fn warn_batch_conflicts(args: &BatchArgs, config: &batch::BatchConfig) -> Result<()> {
    if let Some(avail) = batch_helpers::low_disk_space_mb(500) {
        aid_warn!("[aid] Warning: low disk space ({avail} MB free) — parallel dispatch may fail");
    }
    batch_helpers::warn_for_rate_limited_agents(&config.tasks);
    if args.analyze || config.defaults.analyze.unwrap_or(false) {
        let overlaps = analyze_file_overlap(&config.tasks, &config.defaults);
        if !overlaps.is_empty() {
            let has_errors = overlaps
                .iter()
                .any(|o| o.severity == OverlapSeverity::Error);
            let label = if has_errors {
                "output conflicts"
            } else {
                "potential merge conflicts"
            };
            aid_warn!("[aid] Warning: {label} detected:");
            for overlap in &overlaps {
                let severity = if overlap.severity == OverlapSeverity::Error {
                    "ERROR"
                } else {
                    "WARN"
                };
                aid_warn!(
                    "  [{severity}] {} - referenced by: {}",
                    overlap.file,
                    overlap.task_ids.join(", ")
                );
            }
            if has_errors {
                anyhow::bail!(
                    "Output file conflicts detected — parallel tasks would overwrite each other. \
                    Add depends_on or use different output paths."
                );
            }
            aid_warn!("[aid] Consider adding dependencies or using --worktree isolation");
        }
    }
    Ok(())
}

pub(super) async fn dispatch_batch(
    store: &Arc<Store>,
    config: &batch::BatchConfig,
    args: &BatchArgs,
    has_dependencies: bool,
    max_concurrent: Option<usize>,
    shared_dir_path: Option<&str>,
    explicit_repo_path: Option<&str>,
) -> Result<batch_types::BatchDispatchResult> {
    let auto_fallback = config.defaults.auto_fallback.unwrap_or(false);
    Ok(if has_dependencies && args.parallel {
        batch_dispatch::dispatch_parallel_with_dependencies(
            store.clone(),
            &config.tasks,
            max_concurrent,
            auto_fallback,
            shared_dir_path,
            explicit_repo_path,
        )
        .await?
    } else if has_dependencies {
        batch_dispatch::dispatch_sequential_with_dependencies(
            store.clone(),
            &config.tasks,
            auto_fallback,
            shared_dir_path,
            explicit_repo_path,
        )
        .await?
    } else if args.parallel {
        batch_dispatch::dispatch_parallel(
            store.clone(),
            &config.tasks,
            max_concurrent,
            auto_fallback,
            shared_dir_path,
            explicit_repo_path,
        )
        .await?
    } else {
        batch_dispatch::dispatch_sequential(
            store.clone(),
            &config.tasks,
            auto_fallback,
            shared_dir_path,
            explicit_repo_path,
        )
        .await?
    })
}
