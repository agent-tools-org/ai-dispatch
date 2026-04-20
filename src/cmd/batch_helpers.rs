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
        let Some(agent) = crate::types::AgentKind::parse_str(&task.agent) else {
            continue;
        };
        if !seen.insert(agent) {
            continue;
        }
        if rate_limit::is_rate_limited(&agent) {
            let count = tasks.iter().filter(|t| t.agent == agent.as_str()).count();
            aid_warn!(
                "[aid] Warning: {agent} is rate-limited — {count} task(s) may fail or need fallback"
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
