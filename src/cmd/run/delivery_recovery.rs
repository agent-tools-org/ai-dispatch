// One-shot Codex recovery for successful process exits without final delivery.
// Exports maybe_auto_recover_missing_delivery for post-run lifecycle orchestration.
// Depends on retry-chain state, saved Codex session IDs, and normal run dispatch.

use anyhow::Result;
use std::sync::Arc;

use crate::store::Store;
use crate::types::{AgentKind, DeliveryAssessment, EventKind, TaskEvent, TaskId, TaskStatus};

use super::{run, RunArgs};

const RECOVERY_PROMPT: &str = "Your previous turn ended without a final deliverable. \
Do not perform more investigation or call tools unless the existing evidence is insufficient \
to avoid a factual error. Produce the requested final answer now using the evidence already \
collected. Follow the original output and citation requirements.";

pub(crate) async fn maybe_auto_recover_missing_delivery(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if !is_recovery_candidate(&task) || recovery_already_attempted(store.as_ref(), task_id)? {
        return Ok(None);
    }
    let Some(session_id) = task.agent_session_id.clone() else {
        insert_unavailable_event(store.as_ref(), task_id)?;
        return Ok(None);
    };

    insert_started_event(store.as_ref(), task_id)?;
    let mut retry_args = args.clone();
    retry_args.prompt = RECOVERY_PROMPT.to_string();
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.session_id = Some(session_id);
    retry_args.existing_task_id = None;
    retry_args.background = false;
    retry_args.retry = 0;
    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    Ok(Some(retry_id))
}

fn is_recovery_candidate(task: &crate::types::Task) -> bool {
    task.status == TaskStatus::Failed
        && task.agent == AgentKind::Codex
        && task.read_only
        && task.delivery_assessment() == Some(DeliveryAssessment::MissingFinalDelivery)
}

fn recovery_already_attempted(store: &Store, task_id: &TaskId) -> Result<bool> {
    let chain = store.get_retry_chain(task_id.as_str())?;
    let missing_count = chain
        .iter()
        .filter(|entry| {
            entry.delivery_assessment()
                == Some(DeliveryAssessment::MissingFinalDelivery)
        })
        .count();
    Ok(missing_count > 1)
}

fn insert_started_event(store: &Store, task_id: &TaskId) -> Result<()> {
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: "Missing final delivery: resuming Codex session once for final report".to_string(),
        metadata: Some(serde_json::json!({
            "delivery_recovery": "started",
        })),
    })
}

fn insert_unavailable_event(store: &Store, task_id: &TaskId) -> Result<()> {
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail: "Missing final delivery: Codex session ID unavailable; manual retry required"
            .to_string(),
        metadata: Some(serde_json::json!({
            "delivery_recovery": "unavailable",
        })),
    })
}
