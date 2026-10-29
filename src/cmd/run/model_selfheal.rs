// Self-heal retry when an agent fails on an unavailable/deprecated model id.
// Exports: maybe_auto_retry_after_model_unavailable, read_model_unavailable_message.
// Deps: model_health classifier, store retry chain, run dispatch.

use anyhow::Result;
use std::sync::Arc;

use crate::{store::Store, types::*};

use super::{RunArgs, apply_retry_target, inherit_retry_base_branch, run};

/// When a task fails because its model id is unavailable (deprecated/renamed/
/// unsupported), auto-retry once on the agent's own current default model. This
/// self-heals stale model selections without any manual table edit. Runs at most
/// once per retry chain.
pub(crate) async fn maybe_auto_retry_after_model_unavailable(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<TaskId>> {
    if args.policy.is_some() { return Ok(None); }
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

    let failed_model = task.requested_model.clone().unwrap_or_else(|| "(selected)".to_string());
    aid_warn!(
        "[aid] Model '{}' unavailable for {} — auto-retrying on its default model",
        failed_model,
        task.agent
    );

    let mut retry_args = RunArgs::for_retry(store, &task)?;
    retry_args.dir = args.dir.clone();
    retry_args.env = args.env.clone();
    retry_args.foreground = args.foreground;
    retry_args.announce = args.announce;
    retry_args.on_done = None;
    retry_args.prompt = task.prompt.clone();
    retry_args.force_default_model = true;
    retry_args.model_source = crate::agent::model_validation::ModelSource::AidResolved;
    retry_args.model = None;
    retry_args.budget = false;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.background = false;
    apply_retry_target(&task, &mut retry_args)?;
    inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);

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
    store.insert_event(&TaskEvent::new(
        task_id.clone(),
        EventKind::Milestone,
        format!(
            "model unavailable → retry on default: {}",
            crate::agent::truncate::truncate_text(message, 120)
        ),
    ).with_metadata(Some(serde_json::json!({ "model_self_healed": true }))))?;
    Ok(())
}

pub(crate) fn read_model_unavailable_message(task_id: &TaskId) -> Option<String> {
    let stderr = std::fs::read_to_string(crate::support::paths::stderr_path(task_id.as_str())).ok();
    let from_stderr = stderr.as_deref().and_then(find_model_unavailable_stderr_line);
    from_stderr.or_else(|| {
        std::fs::read_to_string(crate::support::paths::log_path(task_id.as_str()))
            .ok()
            .as_deref()
            .and_then(find_model_unavailable_json_error)
    })
}

fn find_model_unavailable_stderr_line(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(crate::routing::model_health::extract_model_unavailable_stderr_line)
}

fn find_model_unavailable_json_error(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(crate::routing::model_health::extract_model_unavailable_json_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_task(id: &str) -> Task {
        crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Failed)
    }

    fn marker_event(task_id: &str) -> TaskEvent {
        TaskEvent::new(
            TaskId(task_id.to_string()),
            EventKind::Error,
            "model unavailable → retry on default".to_string(),
        ).with_metadata(Some(serde_json::json!({ "model_self_healed": true })))
    }

    fn plain_event(task_id: &str) -> TaskEvent {
        TaskEvent::new(TaskId(task_id.to_string()), EventKind::Error, "some other failure".to_string())
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

    #[test]
    fn model_retry_notice_preserves_settlement_winner() {
        let store = Store::open_memory().expect("store");
        let task = failed_task("t-notice-winner");
        store.insert_task(&task).expect("task");
        store.insert_event(&plain_event(task.id.as_str())).expect("original failure");
        insert_model_selfheal_event(&store, &task.id, "unavailable model").expect("notice");
        assert_eq!(store.latest_error(task.id.as_str()).as_deref(), Some("some other failure"));
        let events = store.get_events(task.id.as_str()).expect("events");
        assert_eq!(events.last().expect("notice").event_kind, EventKind::Milestone);
        assert!(was_model_self_healed(&events));
    }
}
