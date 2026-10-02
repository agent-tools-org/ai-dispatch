// Batch helper utilities (pathing, summaries, hooks, safety warnings).
// Exports: batch_summary, format_elapsed, warn_for_rate_limited_agents, low_disk_space_mb, resolve_batch_path, ensure_batch_workgroup, resolve_hook_targets, trigger_conditional
// Deps: crate::batch, crate::rate_limit, crate::store::Store, super::batch_validate
use crate::batch;
use crate::rate_limit;
use crate::store::Store;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::batch_types::BatchTaskOutcome;
use super::batch_validate::task_label;

pub(crate) fn batch_summary(
    outcomes: &[BatchTaskOutcome],
    task_ids: &[String],
    tasks: &[batch::BatchTask],
    store: &Store,
    start_time: Instant,
    repo_path: Option<&str>,
) -> String {
    let done = outcomes
        .iter()
        .filter(|outcome| **outcome == BatchTaskOutcome::Done)
        .count();
    let failed = outcomes
        .iter()
        .filter(|outcome| **outcome == BatchTaskOutcome::Failed)
        .count();
    let skipped = outcomes
        .iter()
        .filter(|outcome| **outcome == BatchTaskOutcome::Skipped)
        .count();
    let total = outcomes.len();
    let total_cost: f64 = task_ids
        .iter()
        .filter_map(|task_id| store.get_task(task_id).ok().flatten())
        .filter_map(|task| task.cost_usd)
        .sum();
    let mut summary = format!("[batch] {done}/{total} done, {failed} failed, {skipped} skipped");
    if total_cost > 0.0 {
        summary.push_str(&format!(". Cost: ${total_cost:.2}"));
    }
    summary.push_str(&format!(". Time: {}", format_elapsed(start_time.elapsed())));
    if let (Some(repo_path), Some(group_id)) = (
        repo_path,
        tasks.first().and_then(|task| task.group.as_deref()),
    ) && let Some(hint) = crate::cmd::batch_gitbutler::merge_back_hint(Path::new(repo_path), group_id) {
        summary.push('\n');
        summary.push_str(&hint);
    }
    if failed == 0 {
        return summary;
    }
    let failures = outcomes
        .iter()
        .enumerate()
        .filter(|(_, outcome)| **outcome == BatchTaskOutcome::Failed)
        .map(|(index, _)| format!("{} ({})", task_ids[index], task_label(&tasks[index], index)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{summary}\n[batch] Failed: {failures}")
}

pub(crate) fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        return format!("{seconds}s");
    }
    format!("{}m {}s", seconds / 60, seconds % 60)
}

pub(crate) fn warn_for_rate_limited_agents(tasks: &[batch::BatchTask]) {
    let mut seen = std::collections::HashSet::new();
    for task in tasks {
        if !seen.insert(task.agent.clone()) {
            continue;
        }
        let (agent, custom_name) = rate_limit::resolve_agent(&task.agent);
        if rate_limit::is_rate_limited(&agent, custom_name) {
            let count = tasks.iter().filter(|t| t.agent == task.agent).count();
            let label = rate_limit::marker_slug(&agent, custom_name);
            aid_warn!(
                "[aid] Warning: {label} is rate-limited — {count} task(s) may fail or need fallback"
            );
        }
    }
}

pub(crate) fn resolve_batch_path(path: &Path) -> std::path::PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    match path.file_name() {
        Some(file_name) => {
            let fallback = crate::paths::aid_dir().join("batches").join(file_name);
            if fallback.exists() {
                fallback
            } else {
                path.to_path_buf()
            }
        }
        None => path.to_path_buf(),
    }
}

pub(crate) fn ensure_batch_workgroup(
    store: &Store,
    stem: &str,
    custom_gid: Option<&str>,
    shared_dir: bool,
) -> Result<(String, Option<PathBuf>)> {
    if let Some(gid) = custom_gid
        && store.get_workgroup(gid)?.is_some()
    {
        aid_info!("[aid] Reusing existing workgroup {gid} for batch {stem}");
        let path = if shared_dir {
            match crate::shared_dir::shared_dir_path(gid) {
                Some(path) => Some(path),
                None => Some(crate::shared_dir::create_shared_dir(gid)?),
            }
        } else {
            None
        };
        return Ok((gid.to_string(), path));
    }
    let wg = store.create_workgroup(
        stem,
        "Auto-created for batch dispatch",
        Some(stem),
        custom_gid,
    )?;
    aid_info!("[aid] Auto-created workgroup {} for batch {stem}", wg.id);
    let path = if shared_dir {
        Some(crate::shared_dir::create_shared_dir(wg.id.as_str())?)
    } else {
        None
    };
    Ok((wg.id.to_string(), path))
}

pub(crate) fn resolve_hook_targets<F>(
    tasks: &[batch::BatchTask],
    name_map: &HashMap<&str, usize>,
    selector: F,
) -> Result<Vec<Option<usize>>>
where
    F: Fn(&batch::BatchTask) -> Option<&str>,
{
    tasks
        .iter()
        .map(|task| {
            if let Some(reference) = selector(task) {
                let trimmed = reference.trim();
                let &target_idx = name_map
                    .get(trimmed)
                    .ok_or_else(|| anyhow!("unknown hook target: {trimmed}"))?;
                Ok(Some(target_idx))
            } else {
                Ok(None)
            }
        })
        .collect()
}

pub(crate) fn trigger_conditional(
    outcome: BatchTaskOutcome,
    task_idx: usize,
    triggered: &mut [bool],
    success_targets: &[Option<usize>],
    failure_targets: &[Option<usize>],
) {
    match outcome {
        BatchTaskOutcome::Done => {
            if let Some(target_idx) = success_targets[task_idx] {
                triggered[target_idx] = true;
            }
        }
        BatchTaskOutcome::Failed => {
            if let Some(target_idx) = failure_targets[task_idx] {
                triggered[target_idx] = true;
            }
        }
        BatchTaskOutcome::Skipped => {}
    }
}

/// Returns Some(available_mb) if disk space is below the threshold, None if OK.
pub(crate) fn low_disk_space_mb(min_mb: u64) -> Option<u64> {
    let avail = crate::system_resources::available_disk_mb(".")?;
    if avail < min_mb {
        Some(avail)
    } else {
        None
    }
}

pub(super) fn finalize_batch(
    path: &Path,
    dispatch: &super::batch_types::BatchDispatchResult,
    task_ids: &[String],
    config: &batch::BatchConfig,
    store: &Store,
    start_time: Instant,
    batch_repo_path: Option<&str>,
    wait_error: Option<anyhow::Error>,
    total: usize,
) -> Result<()> {
    aid_info!(
        "{}",
        batch_summary(
            &dispatch.outcomes,
            &dispatch.task_ids,
            &config.tasks,
            store,
            start_time,
            batch_repo_path,
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
    let group_id = config.tasks.first().and_then(|t| t.group.as_deref());
    if let Some(gid) = group_id {
        aid_hint!("[aid] Wait: aid wait --group {gid}");
    } else if task_ids.len() == 1 {
        aid_hint!("[aid] Wait: aid wait {}", task_ids[0]);
    }
    aid_hint!("[aid] TUI:   aid watch --tui");
    if let Some(error) = wait_error {
        return Err(error);
    }
    Ok(())
}

pub(super) fn parse_cli_vars(raw_vars: &[String]) -> Result<HashMap<String, String>> {
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

pub(super) fn resolve_config_path(batch_path: &Path, value: &str) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        return value.to_string();
    }
    batch_path.parent().unwrap_or_else(|| Path::new(".")).join(path).to_string_lossy().into_owned()
}
pub(super) fn warn_nested_repo_for_batch(tasks: &[batch::BatchTask]) {
    let Some(task) = tasks.iter().find(|task| task.worktree.is_some()) else {
        return;
    };
    crate::repo_root::warn_if_nested_repo(task.dir.as_deref().unwrap_or("."));
}
