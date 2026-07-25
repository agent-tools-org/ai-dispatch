// Batch retry logic for failed/skipped tasks.
// Exports: retry_failed
// Deps: crate::cmd::run, crate::store::Store, crate::types::Task
use crate::cmd::run::{self, apply_retry_target, RunArgs};
use crate::cmd::wait::{wait_for_task_ids, WaitOutcome};
use crate::store::Store;
use crate::types::{Task, TaskStatus};
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tokio::time::Duration;
/// Caps the serialized retry waiter only; the retried task keeps running.
const SERIAL_RETRY_TIMEOUT_SECS: u64 = 30 * 60;
type WorktreeIdentity = (Option<String>, Option<String>);
#[derive(Debug)]
struct RetryBucket {
    worktree_path: Option<String>,
    worktree_branch: Option<String>,
    tasks: Vec<Task>,
}
impl RetryBucket {
    fn new(task: Task) -> Self {
        Self {
            worktree_path: task.worktree_path.clone(),
            worktree_branch: task.worktree_branch.clone(),
            tasks: vec![task],
        }
    }

    fn label(&self) -> String {
        self.worktree_branch
            .clone()
            .or_else(|| self.worktree_path.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

pub async fn retry_failed(
    store: Arc<Store>,
    group_id: &str,
    agent_override: Option<&str>,
    include_waiting: bool,
) -> Result<()> {
    crate::sanitize::validate_workgroup_id(group_id)?;
    let tasks = store.list_tasks_by_group(group_id)?;
    let total = tasks.len();
    let retry_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| should_retry_task(task.status, include_waiting))
        .collect();
    if retry_tasks.is_empty() {
        println!("No retryable tasks in {group_id}");
        return Ok(());
    }
    println!("[batch] Retrying {}/{} task(s) in {group_id}", retry_tasks.len(), total);
    for bucket in bucket_retry_tasks(retry_tasks) {
        dispatch_retry_bucket(&store, bucket, group_id, agent_override).await?;
    }
    Ok(())
}

pub(super) fn should_retry_task(status: TaskStatus, include_waiting: bool) -> bool {
    matches!(status, TaskStatus::Failed | TaskStatus::Skipped)
        || (include_waiting && status == TaskStatus::Waiting)
}

pub(crate) fn retry_task_to_run_args(store: &Store, task: &Task, group_id: &str, agent_override: Option<&str>) -> Result<RunArgs> {
    let agent_name = if let Some(override_name) = agent_override {
        override_name.to_string()
    } else {
        let original = task.agent_display_name().to_string();
        if let Some(kind) = crate::types::AgentKind::parse_str(&original) {
            if crate::rate_limit::is_rate_limited(&kind) {
                if let Some(fallback) = crate::agent::selection::coding_fallback_for(&kind) {
                    crate::aid_info!(
                        "[aid] {} is rate-limited, retrying with fallback: {}",
                        original,
                        fallback.as_str()
                    );
                    fallback.as_str().to_string()
                } else {
                    original
                }
            } else {
                original
            }
        } else {
            original
        }
    };
    let mut run_args = RunArgs::saved_for_task(store, task.id.as_str())?.unwrap_or_else(|| {
        RunArgs {
            repo: task.repo_path.clone(),
            output: task.output_path.clone(),
            model: task.model.clone(),
            verify: task.verify.clone(),
            read_only: task.read_only,
            budget: task.budget,
            ..Default::default()
        }
    });
    run_args.agent_name = agent_name;
    run_args.prompt = task.prompt.clone();
    apply_retry_target(task, &mut run_args);
    run_args.group = Some(group_id.to_string());
    run_args.background = true;
    run_args.announce = true;
    run_args.parent_task_id = Some(task.id.to_string());
    run_args.existing_task_id = None;
    Ok(run_args)
}

fn bucket_retry_tasks(tasks: Vec<Task>) -> Vec<RetryBucket> {
    let mut buckets = Vec::new();
    let mut bucket_indexes = HashMap::<WorktreeIdentity, usize>::new();
    for task in tasks {
        let Some(identity) = retry_bucket_identity(&task) else {
            buckets.push(RetryBucket::new(task));
            continue;
        };
        if let Some(bucket_idx) = bucket_indexes.get(&identity).copied() {
            buckets[bucket_idx].tasks.push(task);
            continue;
        }
        let bucket_idx = buckets.len();
        bucket_indexes.insert(identity, bucket_idx);
        buckets.push(RetryBucket::new(task));
    }
    buckets
}

fn retry_bucket_identity(task: &Task) -> Option<WorktreeIdentity> {
    if task.worktree_path.is_none() && task.worktree_branch.is_none() {
        None
    } else {
        Some((task.worktree_path.clone(), task.worktree_branch.clone()))
    }
}

async fn dispatch_retry_bucket(
    store: &Arc<Store>,
    bucket: RetryBucket,
    group_id: &str,
    agent_override: Option<&str>,
) -> Result<()> {
    if bucket.tasks.len() == 1 {
        let run_args = retry_task_to_run_args(store, &bucket.tasks[0], group_id, agent_override)?;
        let _ = run::run(store.clone(), run_args).await?;
        return Ok(());
    }
    let label = bucket.label();
    println!("[aid] Serializing {} retries in worktree {}", bucket.tasks.len(), label);
    for task in bucket.tasks {
        let run_args = retry_task_to_run_args(store, &task, group_id, agent_override)?;
        let task_id = run::run(store.clone(), run_args).await?;
        wait_for_retry_completion(store, task_id.as_str()).await?;
    }
    println!("[aid] Worktree {} bucket complete", label);
    Ok(())
}

async fn wait_for_retry_completion(store: &Arc<Store>, task_id: &str) -> Result<()> {
    let timeout = Duration::from_secs(SERIAL_RETRY_TIMEOUT_SECS);
    let task_ids = [task_id.to_string()];
    match wait_for_task_ids(store, &task_ids, None, false, Some(timeout)).await? {
        WaitOutcome::Completed => Ok(()),
        WaitOutcome::TimedOut(_) => {
            anyhow::bail!(
                "Timed out waiting for retried task {} after {}s",
                task_id,
                SERIAL_RETRY_TIMEOUT_SECS
            )
        }
    }
}

#[cfg(test)]
#[path = "batch_retry_tests.rs"]
mod tests;
