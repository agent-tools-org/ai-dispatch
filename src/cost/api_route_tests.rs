// API-route token estimates survive missing results and preserve actual provider costs.
// Uses persisted usage events and existing Claude model pricing without live providers.
use super::*;

fn home() -> (tempfile::TempDir, crate::support::paths::AidHomeGuard) {
    let home = tempfile::tempdir().unwrap();
    let guard = crate::support::paths::AidHomeGuard::set(home.path());
    (home, guard)
}

fn task(store: &Store, id: &str) -> Task {
    store
        .insert_waiting_task(
            id,
            "claude-api-ming",
            "test",
            None,
            None,
            None,
            None,
            Some("sonnet"),
            None,
            false,
            false,
        )
        .unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET agent = 'custom', custom_agent_name = 'claude-api-ming',
        tokens = 1000000 WHERE id = ?1",
            [id],
        )
        .unwrap();
    store.get_task(id).unwrap().unwrap()
}

#[test]
fn stopped_timed_out_and_crashed_tasks_use_existing_token_estimate() {
    let (_home, _guard) = home();
    let store = Store::open_memory().unwrap();
    let original = task(&store, "t-estimate");
    let expected =
        crate::cost::estimate_cost(1_000_000, original.costing_model(), AgentKind::Claude).unwrap();
    assert!(expected > 0.0);
    for status in [
        crate::types::TaskStatus::Stopped,
        crate::types::TaskStatus::Failed,
    ] {
        let mut stopped = original.clone();
        stopped.status = status;
        assert_eq!(task_cost(&store, &stopped).unwrap(), Some(expected));
    }
    let mut reported = original;
    reported.cost_usd = Some(0.25);
    assert_eq!(task_cost(&store, &reported).unwrap(), Some(0.25));
}

#[test]
fn running_assistant_usage_is_estimated_and_deduplicated_without_result() {
    let (_home, _guard) = home();
    let store = Store::open_memory().unwrap();
    let mut task = task(&store, "t-running");
    task.tokens = None;
    let mut info = CompletionInfo {
        tokens: None,
        status: crate::types::TaskStatus::Done,
        cost_usd: None,
        model: None,
        exit_code: None,
    };
    let line = r#"{"type":"assistant","message":{"id":"message-1","model":"sonnet",
        "usage":{"input_tokens":1000000,"output_tokens":1000}}}"#;
    observe_usage(&store, &task.id, line, &mut info).unwrap();
    let once = task_cost(&store, &task).unwrap().unwrap();
    assert!(once > 0.0);
    observe_usage(&store, &task.id, line, &mut info).unwrap();
    assert_eq!(task_cost(&store, &task).unwrap(), Some(once));
    assert_eq!(info.tokens, Some(1_001_000));
    observe_usage(
        &store,
        &task.id,
        r#"{"type":"result","total_cost_usd":0.75}"#,
        &mut info,
    )
    .unwrap();
    assert_eq!(task_cost(&store, &task).unwrap(), Some(0.75));
}

#[test]
fn historical_provider_stdout_usage_is_estimated_without_a_result() {
    let (_home, _guard) = home();
    let store = Store::open_memory().unwrap();
    let mut task = task(&store, "t-legacy-usage");
    task.tokens = None;
    store
        .insert_agent_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "assistant output".into(),
            metadata: Some(json!({"provider_stdout":
                r#"{"type":"assistant","message":{"id":"legacy","model":"sonnet",
                    "usage":{"input_tokens":1000000,"output_tokens":1000}}}"#,
            })),
        })
        .unwrap();
    assert!(task_cost(&store, &task).unwrap().is_some_and(|cost| cost > 0.0));
}

#[test]
fn provider_result_replaces_a_previously_stored_estimate() {
    let (_home, _guard) = home();
    let store = Store::open_memory().unwrap();
    let mut task = task(&store, "t-final-authority");
    task.cost_usd = Some(0.5);
    store
        .insert_agent_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Completion,
            detail: "reported cost".into(),
            metadata: Some(json!({"cost_usd": 0.75})),
        })
        .unwrap();
    assert_eq!(task_cost(&store, &task).unwrap(), Some(0.75));
}
