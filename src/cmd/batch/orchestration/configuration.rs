// Extracted configuration phases.
// Deps: parent module types and helpers.
use super::*;

pub(super) fn load_batch_config(
    args: &BatchArgs,
) -> Result<(std::path::PathBuf, batch::BatchConfig)> {
    if args.max_concurrent == Some(0) {
        anyhow::bail!("--max-concurrent must be at least 1");
    }
    let requested_path = Path::new(&args.file);
    let source_path = requested_path.exists().then_some(requested_path);
    let invoked_pwd = std::env::current_dir().ok();
    let resolved_path = batch_helpers::resolve_batch_path(requested_path);
    let path = resolved_path.as_path();
    let cli_vars = parse_cli_vars(&args.vars)?;
    let config = if source_path == Some(path) && cli_vars.is_empty() {
        batch::parse_batch_file(path)
    } else if source_path == Some(path) {
        batch::parse_batch_file_with_vars(path, &cli_vars)
    } else {
        batch::parse_batch_file_with_vars_and_source(
            path,
            &cli_vars,
            source_path,
            invoked_pwd.as_deref(),
        )
    }
    .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    Ok((resolved_path, config))
}

pub(super) fn resolve_batch_repository(
    args: &BatchArgs,
    config: &batch::BatchConfig,
    path: &Path,
) -> Result<(Option<String>, Option<String>)> {
    let configured_repo_root = args.repo_root.clone().or_else(|| {
        config
            .defaults
            .repo_root
            .as_ref()
            .map(|value| resolve_config_path(path, value))
    });
    let explicit_repo_path = crate::support::repo_root::resolve_explicit_repo_path(
        configured_repo_root.as_deref(),
        None,
    )?;
    let batch_repo_path = explicit_repo_path
        .clone()
        .or_else(|| crate::support::repo_root::resolve_git_root_string(".").ok());
    if let Some(repo_path) = batch_repo_path.as_deref() {
        crate::cmd::batch_gitbutler::maybe_prompt_gitbutler_batch_integration(
            Path::new(repo_path),
            args.no_prompt || args.yes,
        )?;
    }
    Ok((explicit_repo_path, batch_repo_path))
}

pub(super) fn assign_batch_groups(
    store: &Store,
    args: &BatchArgs,
    config: &mut batch::BatchConfig,
    path: &Path,
    total: usize,
    shared_dir_enabled: bool,
) -> Result<()> {
    assign_explicit_batch_group(store, args, config)?;
    let no_groups_set = config.tasks.iter().all(|t| t.group.is_none());
    if no_groups_set {
        if let Ok(env_group) = std::env::var("AID_GROUP") {
            for task in &mut config.tasks {
                task.group = Some(env_group.clone());
            }
            aid_info!("[aid] Using workspace {env_group} from AID_GROUP");
        } else if total >= 2 {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
            let workgroup_name = config.title.as_deref().unwrap_or(stem);
            let (wg_id, shared_path) = batch_helpers::ensure_batch_workgroup(
                &store,
                workgroup_name,
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
    Ok(())
}

pub(super) fn prepare_batch_tasks(
    args: &BatchArgs,
    config: &mut batch::BatchConfig,
    explicit_repo_path: &Option<String>,
) -> Result<()> {
    if explicit_repo_path.is_none() {
        warn_nested_repo_for_batch(&config.tasks);
    }
    batch_profile::validate_required_profiles(&config.tasks)?;
    validate_batch_config(&config.tasks, args.parallel, args.force)?;
    let deps = batch::dependency_indices(&config.tasks)
        .unwrap_or_else(|_| vec![Vec::new(); config.tasks.len()]);
    let deduped = batch_output_dedup::dedup_output_paths(&mut config.tasks, &deps);
    if deduped > 0 {
        aid_info!("[aid] Auto-suffixed {deduped} task output path(s) to avoid conflicts");
    }
    Ok(())
}

pub(super) fn batch_shared_directory(
    config: &batch::BatchConfig,
    shared_dir_enabled: bool,
) -> Option<String> {
    let shared_dir_path = if shared_dir_enabled {
        config
            .tasks
            .first()
            .and_then(|task| task.group.as_deref())
            .and_then(crate::support::shared_dir::shared_dir_path)
            .map(|path| path.display().to_string())
    } else {
        None
    };
    shared_dir_path
}

fn assign_explicit_batch_group(
    store: &Store,
    args: &BatchArgs,
    config: &mut batch::BatchConfig,
) -> Result<()> {
    let effective_group = args.group.as_ref().or(config.defaults.group.as_ref());
    if let Some(group) = effective_group {
        if store.get_workgroup(group)?.is_none() {
            anyhow::bail!(
                "Workgroup '{group}' not found. Create it with: aid group create <name> --id {group}"
            );
        }
        let source = if args.group.is_some() {
            "--group flag"
        } else {
            "[defaults] group"
        };
        for task in &mut config.tasks {
            if task.group.is_none() {
                task.group = Some(group.clone());
            }
        }
        aid_info!("[aid] Using workgroup {group} from {source}");
    }
    Ok(())
}

pub(super) async fn wait_for_batch(
    store: &Arc<Store>,
    args: &BatchArgs,
    has_dependencies: bool,
    task_ids: &[String],
) -> Result<Option<anyhow::Error>> {
    let wait_error = if args.wait && args.parallel && !has_dependencies && !task_ids.is_empty() {
        Some(batch_wait_error(
            crate::cmd::wait::wait_for_task_ids(&store, &task_ids, None, false, None).await?,
        ))
    } else {
        None
    };
    Ok(wait_error.flatten())
}

pub(super) fn resolve_batch_concurrency(
    args: &BatchArgs,
    config: &batch::BatchConfig,
) -> Result<Option<usize>> {
    let max_concurrent = args.max_concurrent.or(config.defaults.max_concurrent);
    if max_concurrent == Some(0) {
        anyhow::bail!("max_concurrent must be at least 1");
    }
    Ok(max_concurrent)
}
