// Self-heal retry when an agent fails on an unavailable/deprecated model id.
// Exports: maybe_auto_retry_after_model_unavailable, read_model_unavailable_message.
// Deps: model_health classifier, store retry chain, run dispatch.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd::retry_logic;
use crate::{store::Store, types::*};

use super::{RunArgs, inherit_retry_base_branch, run};

/// When a task fails because its model id is unavailable (deprecated/renamed/
/// unsupported), auto-retry once on the agent's own current default model. This
/// self-heals stale model selections without any manual table edit. Runs at most
/// once per retry chain.
pub(crate) async fn maybe_auto_retry_after_model_unavailable(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }
    // Don't re-heal a task that is already a default-model retry, and don't loop.
    if args.force_default_model || already_self_healed_model(store.as_ref(), &task)? {
        return Ok(None);
    }
    let Some(message) = read_model_unavailable_message(task_id) else {
        return Ok(None);
    };

    let failed_model = task.model.clone().unwrap_or_else(|| "(selected)".to_string());
    aid_warn!(
        "[aid] Model '{}' unavailable for {} — auto-retrying on its default model",
        failed_model,
        task.agent
    );

    let root_prompt =
        retry_logic::root_prompt(store.as_ref(), &task).unwrap_or_else(|| args.prompt.clone());
    let mut retry_args = args.clone();
    retry_args.prompt = root_prompt;
    retry_args.force_default_model = true;
    retry_args.model = None;
    retry_args.budget = false;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task.output_path.clone().or_else(|| retry_args.output.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.background = false;
    let (dir, worktree) = super::retry_target(&task);
    retry_args.dir = dir.or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
    if task.agent.supports_session_resume() {
        retry_args.session_id = task.agent_session_id.clone();
    }

    insert_model_selfheal_event(store.as_ref(), task_id, &message)?;
    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    Ok(Some(retry_id))
}

fn already_self_healed_model(store: &Store, task: &Task) -> Result<bool> {
    let chain = store.get_retry_chain(task.id.as_str())?;
    Ok(chain.into_iter().any(|entry| {
        store
            .get_events(entry.id.as_str())
            .map(|events| was_model_self_healed(&events))
            .unwrap_or(false)
    }))
}

fn was_model_self_healed(events: &[TaskEvent]) -> bool {
    events.iter().any(|event| {
        event
            .metadata
            .as_ref()
            .and_then(|value| value.get("model_self_healed"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    })
}

fn insert_model_selfheal_event(store: &Store, task_id: &TaskId, message: &str) -> Result<()> {
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail: format!(
            "model unavailable → retry on default: {}",
            crate::agent::truncate::truncate_text(message, 120)
        ),
        metadata: Some(serde_json::json!({ "model_self_healed": true })),
    })?;
    Ok(())
}

pub(crate) fn read_model_unavailable_message(task_id: &TaskId) -> Option<String> {
    for path in [crate::paths::stderr_path(task_id.as_str()), crate::paths::log_path(task_id.as_str())] {
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Some(line) = find_model_unavailable_line(&content)
        {
            return Some(line);
        }
    }
    None
}

fn find_model_unavailable_line(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(crate::model_health::extract_model_unavailable_message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn failed_task(id: &str) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Failed,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    fn marker_event(task_id: &str) -> TaskEvent {
        TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: "model unavailable → retry on default".to_string(),
            metadata: Some(serde_json::json!({ "model_self_healed": true })),
        }
    }

    fn plain_event(task_id: &str) -> TaskEvent {
        TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: "some other failure".to_string(),
            metadata: None,
        }
    }

    #[test]
    fn was_model_self_healed_detects_marker() {
        assert!(was_model_self_healed(&[plain_event("t-1"), marker_event("t-1")]));
        assert!(!was_model_self_healed(&[plain_event("t-1")]));
        assert!(!was_model_self_healed(&[]));
    }

    #[test]
    fn already_self_healed_true_when_chain_has_marker() {
        let store = Store::open_memory().unwrap();
        let task = failed_task("t-heal");
        store.insert_task(&task).unwrap();
        store.insert_event(&marker_event("t-heal")).unwrap();
        assert!(already_self_healed_model(&store, &task).unwrap());
    }

    #[test]
    fn already_self_healed_false_without_marker() {
        let store = Store::open_memory().unwrap();
        let task = failed_task("t-fresh");
        store.insert_task(&task).unwrap();
        store.insert_event(&plain_event("t-fresh")).unwrap();
        assert!(!already_self_healed_model(&store, &task).unwrap());
    }
}
