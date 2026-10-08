// Completion persistence regressions using captured adapter events and typed usage scenarios.
// Exercises PTY task-row recording, component estimates, legacy fallback, and reported costs.

use super::record_completion;
use crate::agent::get_agent;
use crate::cost::test_support::*;
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, Task, TaskEvent, TaskStatus};
use std::sync::Arc;

fn captured_event(task: &Task) -> TaskEvent {
    let raw = if task.agent == AgentKind::Codex {
        CODEX_USAGE
    } else {
        CLAUDE_USAGE
    };
    get_agent(task.agent)
        .parse_event(&task.id, raw)
        .expect("captured completion")
}

fn record(
    task: &Task,
    event: Option<&TaskEvent>,
    reported_cost: Option<f64>,
) -> (Arc<Store>, Task) {
    crate::paths::ensure_dirs().expect("aid dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = task.clone();
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("task");
    if let Some(event) = event {
        store.insert_event(event).expect("event");
    }
    let info = CompletionInfo {
        tokens: task.tokens,
        status: TaskStatus::Done,
        model: task.requested_model.clone(),
        cost_usd: reported_cost,
        exit_code: Some(0),
    };
    record_completion(
        get_agent(task.agent).as_ref(),
        &task.id,
        &store,
        task.requested_model.as_deref(),
        1_000,
        &info,
    )
    .expect("record completion");
    let stored = store
        .get_task(task.id.as_str())
        .expect("query")
        .expect("stored task");
    (store, stored)
}

#[test]
fn process_completion_stores_codex_components_and_preserves_legacy_blend() {
    let _guard = seed_prices(Some(0.125));
    let legacy = crate::cost::test_support::task(AgentKind::Gemini, 2_020_000, None);
    let (_, stored) = record(&legacy, None, None);
    assert_eq!(
        stored.cost_usd,
        crate::cost::estimate_cost(2_020_000, legacy.costing_model(), AgentKind::Gemini)
    );
    assert_cost(stored.cost_usd, 7.8275);
    let task = task(AgentKind::Codex, 2_020_000, None);
    let mut event = captured_event(&task);
    // Keep the captured wire bytes; supply the brief's scenario as decoded usage parameters.
    let metadata = event.metadata.as_mut().expect("usage metadata");
    metadata["tokens"] = 2_020_000.into();
    metadata["input_tokens"] = 2_000_000.into();
    metadata["uncached_input_tokens"] = 200_000.into();
    metadata["cached_input_tokens"] = 1_800_000.into();
    metadata["output_tokens"] = 20_000.into();
    let (store, stored) = record(&task, Some(&event), None);
    assert_cost(stored.cost_usd, 0.675);
    assert_cost(
        crate::cost::task_cost(&store, &stored).expect("report"),
        0.675,
    );
}

#[test]
fn process_completion_claude_without_report_prices_cache_creation_at_input_multiplier() {
    let _guard = seed_prices(Some(0.125));
    let task = task(AgentKind::Claude, 63_701, None);
    let mut event = captured_event(&task);
    event
        .metadata
        .as_mut()
        .expect("metadata")
        .as_object_mut()
        .expect("object")
        .remove("cost_usd");
    let (_, stored) = record(&task, Some(&event), None);
    assert_cost(stored.cost_usd, 0.0364344375);
}

#[test]
fn process_completion_claude_reported_cost_stays_authoritative() {
    let _guard = seed_prices(Some(0.125));
    let task = task(AgentKind::Claude, 63_701, None);
    let event = captured_event(&task);
    let (_, stored) = record(&task, Some(&event), Some(0.14359275));
    assert_eq!(stored.cost_usd, Some(0.14359275));
}
