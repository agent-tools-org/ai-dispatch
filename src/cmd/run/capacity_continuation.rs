// One capacity continuation per retry chain, then existing cascade/advice routing.
// Exports continue_capacity; depends on captured errors, salvage, session retry and holds.

use super::{RunArgs, inherit_cascade_target, run, take_next_cascade_agent};
use crate::{provider_capacity, rate_limit, store::Store, types::*};
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn continue_capacity(
    store: &Arc<Store>,
    args: &RunArgs,
    task_id: &TaskId,
) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }
    let Some(message) = provider_capacity::read(task_id, task.agent) else {
        return Ok(None);
    };
    let model = task
        .requested_model
        .as_deref()
        .or(task.observed_model.as_deref());
    let hold = rate_limit::dispatch_blocking_hold_for_model(
        &task.agent,
        task.custom_agent_name.as_deref(),
        model,
    )
    .unwrap_or_else(|| "provider capacity hold expired".to_string());
    let child = if !already_continued(store, &task)? {
        let child = continuation_args(store, args, &task)?;
        record(
            store,
            task_id,
            format!("provider capacity — {hold}; resuming session once after backoff"),
            true,
        )?;
        wait_for_capacity(&task, model).await;
        child
    } else if let Some(child) = cascade_args(store, args, &task)? {
        child
    } else {
        record(
            store,
            task_id,
            format!("provider capacity — {hold}; no launchable route remains: {message}"),
            false,
        )?;
        return Ok(None);
    };
    Box::pin(run(store.clone(), child)).await.map(Some)
}

async fn wait_for_capacity(task: &Task, model: Option<&str>) {
    let avail = provider_capacity::availability_for_model(
        &task.agent,
        task.custom_agent_name.as_deref(),
        model,
    );
    let delay = match avail.ends {
        crate::route_availability::HoldEnd::At(expiry) => (expiry
            - chrono::Local::now().naive_local())
        .to_std()
        .unwrap_or_default(),
        _ => std::time::Duration::ZERO,
    };
    tokio::time::sleep(delay.max(std::time::Duration::from_secs(5))).await;
}

fn cascade_args(store: &Store, args: &RunArgs, task: &Task) -> Result<Option<RunArgs>> {
    let mut child = args.clone();
    child.prompt =
        crate::cmd::retry_logic::root_prompt(store, task).unwrap_or_else(|| args.prompt.clone());
    child.on_done = None;
    child.background = false;
    child.parent_task_id = Some(task.id.as_str().to_string());
    inherit_cascade_target(&mut child, task)?;
    readonly_target(task, &mut child);
    if let Some((next, remaining)) = take_next_cascade_agent(args) {
        super::super::switch_agent(&mut child, next);
        child.cascade = remaining;
    } else if let Some(candidate) =
        super::super::advice_route::automatic_candidate(Some(store), args)
    {
        super::super::advice_route::apply_candidate(&mut child, &candidate);
    } else {
        return Ok(None);
    }
    Ok(Some(child))
}

fn continuation_args(store: &Store, args: &RunArgs, task: &Task) -> Result<RunArgs> {
    let mut child = RunArgs::for_retry(store, task)?;
    child.dir = args.dir.clone();
    child.env = args.env.clone();
    child.foreground = args.foreground;
    child.announce = args.announce;
    child.retry = args.retry;
    child.model = task.requested_model.clone().or(task.observed_model.clone());
    child.budget = false;
    child.force_default_model = child.model.is_none();
    child.on_done = None;
    child.background = false;
    child.parent_task_id = Some(task.id.as_str().to_string());
    child.prompt = if task.read_only {
        "Provider capacity interrupted this task. Continue the original read-only task in this session."
    } else {
        "Provider capacity interrupted this task. \
         Continue the original task from the salvaged work in this session."
    }.to_string();
    if child.session_id.is_none()
        || (task.agent == AgentKind::Codex
            && child
                .session_id
                .as_deref()
                .is_some_and(crate::agent::codex::resume_fallback_needed))
    {
        let original = crate::cmd::retry_logic::root_prompt(store, task)
            .unwrap_or_else(|| args.prompt.clone());
        child
            .prompt
            .push_str(&format!("\n\n[Original task]\n{original}"));
    }
    super::super::apply_retry_target(task, &mut child)?;
    readonly_target(task, &mut child);
    Ok(child)
}

fn readonly_target(task: &Task, child: &mut RunArgs) {
    if task.read_only {
        child.dir = task
            .effective_dir
            .clone()
            .or(task.worktree_path.clone())
            .or(child.dir.clone());
        child.worktree = None;
        child.base_branch = None;
    }
}

fn already_continued(store: &Store, task: &Task) -> Result<bool> {
    for entry in store.get_retry_chain(task.id.as_str())? {
        if store.get_events(entry.id.as_str())?.iter().any(|event| {
            event
                .metadata
                .as_ref()
                .and_then(|value| value.get("capacity_continuation"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn record(store: &Store, task_id: &TaskId, detail: String, continuation: bool) -> Result<()> {
    aid_warn!("[aid] {detail}");
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: Some(serde_json::json!({"capacity_continuation": continuation})),
    })
}

#[cfg(test)]
#[path = "capacity_continuation_tests.rs"]
mod tests;
