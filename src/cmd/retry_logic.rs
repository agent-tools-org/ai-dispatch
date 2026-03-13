// Retry logic for failed tasks: depth tracking, backoff, re-dispatch.
// Called from run.rs on task failure when --retry > 0.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

use super::run::RunArgs;
use crate::paths;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus};

pub(crate) async fn prepare_retry(
    store: Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<RunArgs>> {
    if args.retry == 0 {
        return Ok(None);
    }
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }
    let stderr_tail = read_stderr_tail(task_id.as_str(), 5);
    if let Some(parent_id) = args.parent_task_id.as_deref()
        && stderr_tail == read_stderr_tail(parent_id, 5)
    {
        println!("Retry stopped: identical stderr to previous attempt.");
        return Ok(None);
    }
    let depth = retry_depth(&store, args.parent_task_id.as_deref())?;
    let attempt = depth + 1;
    let backoff_secs = backoff_for_attempt(attempt);
    println!("Retry {attempt}/{}: re-dispatching after {backoff_secs}s...", depth + args.retry);
    sleep(Duration::from_secs(backoff_secs)).await;
    let prompt = root_prompt(&store, &task).unwrap_or_else(|| args.prompt.clone());
    let mut retry_args = args.clone();
    retry_args.prompt =
        format!("[Previous attempt failed]\nError: {stderr_tail}\n\n[Original task]\n{prompt}");
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.background = false;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    Ok(Some(retry_args))
}

pub(crate) fn read_stderr_tail(task_id: &str, lines: usize) -> String {
    let Ok(stderr) = std::fs::read_to_string(paths::stderr_path(task_id)) else {
        return "stderr unavailable".to_string();
    };
    let tail: Vec<_> = stderr.lines().rev().take(lines).collect();
    if tail.is_empty() { "stderr unavailable".to_string() } else { tail.into_iter().rev().collect::<Vec<_>>().join("\n") }
}

fn retry_depth(store: &Store, parent_task_id: Option<&str>) -> Result<u32> {
    let mut depth = 0;
    let mut current = parent_task_id.map(str::to_string);
    while let Some(task_id) = current {
        let Some(task) = store.get_task(&task_id)? else { break };
        depth += 1;
        current = task.parent_task_id;
    }
    Ok(depth)
}

fn backoff_for_attempt(attempt: u32) -> u64 {
    match attempt { 0 | 1 => 5, 2 => 15, _ => 45 }
}

fn root_prompt(store: &Store, task: &Task) -> Option<String> {
    let mut prompt = task.prompt.clone();
    let mut current = task.parent_task_id.clone();
    while let Some(task_id) = current {
        let Some(parent) = store.get_task(&task_id).ok().flatten() else { break };
        prompt = parent.prompt;
        current = parent.parent_task_id;
    }
    Some(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::AgentKind;

    fn task(id: &str) -> Task {
        Task {
            id: TaskId(id.to_string()), agent: AgentKind::Codex, prompt: "prompt".to_string(),
            status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
            caller_kind: None, caller_session_id: None, worktree_path: None, worktree_branch: None,
            log_path: None, output_path: None, tokens: None, duration_ms: None, model: None,
            cost_usd: None, created_at: Local::now(), completed_at: None,
        }
    }

    #[test]
    fn backoff_for_attempt_increases() {
        assert!(backoff_for_attempt(1) < backoff_for_attempt(2));
        assert!(backoff_for_attempt(2) < backoff_for_attempt(3));
    }

    #[test]
    fn retry_depth_is_zero_without_parent() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&task("t-root")).unwrap();
        assert_eq!(retry_depth(&store, None).unwrap(), 0);
    }
}
