// Dispatch-time notice when `max_task_cost` cannot be enforced for the route.
// Exports: warn_unenforceable_cost_ceiling().
// Deps: cost::has_known_price, Store, TaskEvent.

use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId};

/// The cost watcher compares a known task cost against the ceiling; with no
/// price for the route the cost stays unknown and the ceiling never fires.
/// Prints one warning and records one event naming that.
pub(crate) fn warn_unenforceable_cost_ceiling(
    store: &Store,
    task_id: &TaskId,
    max_task_cost: Option<f64>,
    agent_kind: AgentKind,
    agent_name: &str,
    model: Option<&str>,
) {
    let Some(max) = max_task_cost else { return };
    if crate::cost::has_known_price(model, agent_kind) {
        return;
    }
    let detail = format!(
        "cost ceiling ${max} cannot be enforced: no known price for {agent_name}/{}",
        model.unwrap_or("agent default"),
    );
    aid_warn!("[aid] Warning: {detail}");
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Setup,
        detail,
        metadata: None,
    });
}

#[cfg(test)]
mod tests {
    use super::super::run_dispatch_prepare::prepare_dispatch_with;
    use super::super::RunArgs;
    use crate::store::Store;
    use crate::types::TaskId;
    use std::sync::Arc;

    /// Prepares a real dispatch (task row, route resolution) and returns the
    /// cost-ceiling events it recorded.
    fn dispatch(agent: &str, model: &str, max_task_cost: Option<f64>) -> Vec<String> {
        let home = tempfile::tempdir().expect("temporary aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        crate::cost::clear_feed_for_tests();
        let dir = tempfile::tempdir().expect("non-project dir");
        let store = Arc::new(Store::open_memory().expect("store"));
        let task_id = TaskId(format!("t-ceiling-{agent}"));
        let mut args = RunArgs {
            agent_name: agent.to_string(),
            prompt: "Investigate a concrete cost ceiling path.".to_string(),
            model: Some(model.to_string()),
            dir: Some(dir.path().to_string_lossy().to_string()),
            existing_task_id: Some(task_id.clone()),
            max_task_cost,
            ..Default::default()
        };
        prepare_dispatch_with(&store, &mut args, |_| true).expect("prepared dispatch");
        store.get_events(task_id.as_str()).expect("events").into_iter()
            .map(|event| event.detail)
            .filter(|detail| detail.starts_with("cost ceiling"))
            .collect()
    }

    #[test]
    fn unpriced_route_with_ceiling_records_one_event() {
        assert_eq!(dispatch("grok", "grok-4.6", Some(2.5)), vec![
            "cost ceiling $2.5 cannot be enforced: no known price for grok/grok-4.6".to_string(),
        ]);
    }

    #[test]
    fn priced_route_is_silent() {
        assert!(dispatch("codex", "gpt-5.6-sol", Some(2.5)).is_empty());
    }

    #[test]
    fn no_ceiling_is_silent() {
        assert!(dispatch("grok", "grok-4.6", None).is_empty());
    }
}
