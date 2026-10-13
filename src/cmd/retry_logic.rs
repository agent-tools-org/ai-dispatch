// Retry logic for failed tasks: depth tracking, backoff, re-dispatch.
// Called from run.rs on task failure when --retry > 0.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

use super::run::{RunArgs, apply_retry_target};
use crate::paths;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

pub(crate) async fn prepare_retry(
    store: Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<RunArgs>> {
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }
    let capacity = crate::provider_capacity::read(&store, task_id, task.agent).is_some();
    if (capacity && capacity_retried(&store, &task)?) || (!capacity && args.retry == 0) {
        return Ok(None);
    }
    let stderr_tail = read_stderr_tail(task_id.as_str(), 5);
    if !capacity && let Some(parent_id) = args.parent_task_id.as_deref()
        && stderr_tail == read_stderr_tail(parent_id, 5)
    {
        println!("Retry stopped: identical stderr to previous attempt.");
        return Ok(None);
    }
    let attempt = retry_depth(&store, args.parent_task_id.as_deref())? + 1;
    let delay = if capacity {
        capacity_delay(&task)
    } else {
        Duration::from_secs(backoff_for_attempt(attempt))
    };
    let mut retry_args = if capacity {
        capacity_args(&store, args, &task)?
    } else {
        build_failed_retry_args(&store, task_id, args, &task, &stderr_tail)?
    };
    queue_retry(&store, &task, &mut retry_args)?;
    store.insert_event(&TaskEvent {
        task_id: task.id.clone(), timestamp: chrono::Local::now(), event_kind: EventKind::Error,
        detail: format!("{}retry {attempt}: {} waiting for {}s",
            if capacity { "provider capacity — " } else { "" },
            retry_args.existing_task_id.as_ref().map(TaskId::as_str).unwrap_or(""), delay.as_secs()),
        metadata: Some(serde_json::json!({"capacity_continuation": capacity})),
    })?;
    sleep(delay).await;
    Ok(Some(retry_args))
}

fn queue_retry(store: &Store, task: &Task, args: &mut RunArgs) -> Result<()> {
    let mut child = task.clone();
    child.id = TaskId::generate();
    child.status = TaskStatus::Waiting;
    child.parent_task_id = Some(task.id.to_string());
    child.prompt = args.prompt.clone();
    child.resolved_prompt = None;
    child.created_at = chrono::Local::now();
    child.completed_at = None;
    child.tokens = None;
    child.prompt_tokens = None;
    child.duration_ms = None;
    child.cost_usd = None;
    child.exit_code = None;
    child.final_head_sha = None;
    child.final_branch = None;
    child.delivery_assessment = None;
    child.audit_verdict = None;
    child.audit_report_path = None;
    child.requested_model = args.model.clone();
    child.observed_model = None;
    child.attribution_source = None;
    child.log_path = Some(paths::log_path(child.id.as_str()).display().to_string());
    store.insert_task(&child)?;
    args.existing_task_id = Some(child.id.clone());
    store.update_task_dispatch_args(child.id.as_str(), &args.dispatch_args_json()?)?;
    Ok(())
}

pub(super) fn capacity_args(store: &Store, args: &RunArgs, task: &Task) -> Result<RunArgs> {
    let mut child = RunArgs::for_retry(store, task)?;
    child.dir = args.dir.clone();
    child.env = args.env.clone();
    child.foreground = args.foreground;
    child.announce = args.announce;
    child.retry = args.retry;
    child.model = task.requested_model.clone().or(task.observed_model.clone());
    child.model_source = crate::agent::model_validation::ModelSource::UserSupplied;
    child.budget = false;
    child.force_default_model = child.model.is_none();
    child.on_done = None;
    child.background = false;
    child.parent_task_id = Some(task.id.to_string());
    let prompt = root_prompt(store, task).unwrap_or_else(|| args.prompt.clone());
    let mode = if task.read_only { "read-only " } else { "" };
    child.prompt = format!(
        "Provider capacity interrupted this task. Continue the original {mode}task in this session.\n\n\
         [Original task]\n{prompt}"
    );
    apply_retry_target(task, &mut child)?;
    if task.read_only {
        child.dir = task.effective_dir.clone().or(task.worktree_path.clone()).or(child.dir);
        child.worktree = None;
        child.base_branch = None;
    }
    Ok(child)
}

fn capacity_delay(task: &Task) -> Duration {
    let available = crate::provider_capacity::availability_for_model(
        &task.agent, task.custom_agent_name.as_deref(),
        task.requested_model.as_deref().or(task.observed_model.as_deref()),
    );
    let delay = match available.ends {
        crate::route_availability::HoldEnd::At(expiry) =>
            (expiry - chrono::Local::now().naive_local()).to_std().unwrap_or_default(),
        _ => Duration::ZERO,
    };
    delay.max(Duration::from_secs(5))
}

fn capacity_retried(store: &Store, task: &Task) -> Result<bool> {
    for ancestor in store.get_retry_chain(task.id.as_str())? {
        if store.get_events(ancestor.id.as_str())?.iter().any(|event|
            event.metadata.as_ref().and_then(|value| value.get("capacity_continuation"))
                == Some(&serde_json::Value::Bool(true)))
        {
            let model = task.requested_model.as_deref().or(task.observed_model.as_deref());
            let hold = crate::rate_limit::dispatch_blocking_hold_for_model(
                &task.agent, task.custom_agent_name.as_deref(), model).unwrap_or_default();
            store.insert_event(&TaskEvent {
                task_id: task.id.clone(), timestamp: chrono::Local::now(), event_kind: EventKind::Error,
                detail: format!("provider capacity for model {} — {hold}; continuation already used",
                    model.unwrap_or("(default)")), metadata: None,
            })?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn build_failed_retry_args(
    store: &Store,
    task_id: &TaskId,
    args: &RunArgs,
    task: &Task,
    stderr_tail: &str,
) -> Result<RunArgs> {
    let prompt = root_prompt(store, task).unwrap_or_else(|| args.prompt.clone());
    let mut retry_args = RunArgs::for_retry(store, task)?;
    retry_args.prompt =
        format!("[Previous attempt failed]\nError: {stderr_tail}\n\n[Original task]\n{prompt}");
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.background = false;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    apply_retry_target(task, &mut retry_args)?;
    Ok(retry_args)
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

pub(crate) fn root_prompt(store: &Store, task: &Task) -> Option<String> {
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
    use crate::types::{AgentKind, VerifyStatus};

    fn task(id: &str) -> Task {
        Task {
            id: TaskId(id.to_string()), agent: AgentKind::Codex, prompt: "prompt".to_string(),
            custom_agent_name: None,
            resolved_prompt: None, category: None, status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
            caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None, project_id: None, worktree_path: None, effective_dir: None,
            worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
            duration_ms: None, requested_model: None, observed_model: None, attribution_source: None, cost_usd: None, exit_code: None, created_at: Local::now(),
            completed_at: None, verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false, budget: false,
            audit_verdict: None, audit_report_path: None, delivery_assessment: None,
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

    #[test]
    fn test_retry_depth_with_chain() {
        let store = Store::open_memory().unwrap();
        let mut t_root = task("t-root");
        t_root.parent_task_id = None;
        store.insert_task(&t_root).unwrap();

        let mut t_r1 = task("t-r1");
        t_r1.parent_task_id = Some("t-root".to_string());
        store.insert_task(&t_r1).unwrap();

        let mut t_r2 = task("t-r2");
        t_r2.parent_task_id = Some("t-r1".to_string());
        store.insert_task(&t_r2).unwrap();

        assert_eq!(retry_depth(&store, Some("t-root")).unwrap(), 1);
        assert_eq!(retry_depth(&store, Some("t-r1")).unwrap(), 2);
    }

    #[test]
    fn test_root_prompt_walks_chain() {
        let store = Store::open_memory().unwrap();
        let mut t_root = task("root");
        t_root.prompt = "original".to_string();
        t_root.parent_task_id = None;
        store.insert_task(&t_root).unwrap();

        let mut t_r1 = task("r1");
        t_r1.prompt = "retry1".to_string();
        t_r1.parent_task_id = Some("root".to_string());
        store.insert_task(&t_r1).unwrap();

        let mut t_r2 = task("r2");
        t_r2.prompt = "retry2".to_string();
        t_r2.parent_task_id = Some("r1".to_string());
        store.insert_task(&t_r2).unwrap();

        let r2_task = store.get_task("r2").unwrap().unwrap();
        assert_eq!(root_prompt(&store, &r2_task), Some("original".to_string()));
    }

    #[test]
    fn test_backoff_capped() {
        assert_eq!(backoff_for_attempt(10), backoff_for_attempt(3));
    }
}

#[cfg(test)]
#[path = "retry_logic_tests.rs"]
mod retry_logic_tests;
