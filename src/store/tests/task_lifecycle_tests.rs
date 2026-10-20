// Task completion updates, retry chains, and model history regressions.
// Deps: parent store test fixtures.

use super::*;

#[test]
fn a_later_silent_completion_does_not_erase_an_observation() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-attr2", AgentKind::Cursor, TaskStatus::Running);
    store.insert_task(&task).unwrap();

    for observed in [Some("composer-2"), None] {
        store
            .update_task_completion(TaskCompletionUpdate {
                id: "t-attr2",
                status: TaskStatus::Done,
                tokens: Some(10),
                duration_ms: 1000,
                observed_model: observed,
                attribution_source: observed.map(|_| AttributionSource::Echoed),
                cost_usd: None,
                exit_code: Some(0),
            })
            .unwrap();
    }

    let loaded = store.get_task("t-attr2").unwrap().unwrap();
    assert_eq!(loaded.observed_model.as_deref(), Some("composer-2"));
}

#[test]
fn update_completion() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0002", AgentKind::Grok, TaskStatus::Running);
    store.insert_task(&task).unwrap();
    store
        .update_task_completion(TaskCompletionUpdate {
            id: "t-0002",
            status: TaskStatus::Done,
            tokens: Some(3000),
            duration_ms: 47000,
            observed_model: Some("gemini-2.5-flash"),
            attribution_source: None,
            cost_usd: Some(0.0038),
            exit_code: None,
        })
        .unwrap();

    let loaded = store.get_task("t-0002").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);
    assert_eq!(loaded.tokens, Some(3000));
    assert_eq!(loaded.duration_ms, Some(47000));
    assert_eq!(loaded.observed_model.as_deref(), Some("gemini-2.5-flash"));
    assert!((loaded.cost_usd.unwrap() - 0.0038).abs() < 0.0001);
    assert_eq!(loaded.exit_code, None);
    assert!(loaded.completed_at.is_some());
}

#[test]
fn update_completion_does_not_override_terminal_intent_statuses() {
    let store = Store::open_memory().unwrap();
    for (id, status) in [
        ("t-0002s", TaskStatus::Stopped),
        ("t-0002f", TaskStatus::Failed),
    ] {
        let task = make_task(id, AgentKind::Codex, status);
        store.insert_task(&task).unwrap();
        store
            .update_task_completion(TaskCompletionUpdate {
                id,
                status: TaskStatus::Done,
                tokens: Some(55),
                duration_ms: 1234,
                observed_model: Some("gpt-5.4"),
                attribution_source: None,
                cost_usd: Some(0.01),
                exit_code: Some(0),
            })
            .unwrap();

        let loaded = store.get_task(id).unwrap().unwrap();
        assert_eq!(loaded.status, status);
        assert_eq!(loaded.tokens, None);
        assert_eq!(loaded.completed_at, None);
    }
}

#[test]
fn update_resolved_prompt_persists() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0003", AgentKind::Codex, TaskStatus::Pending);
    store.insert_task(&task).unwrap();

    store
        .update_resolved_prompt("t-0003", "resolved prompt")
        .unwrap();

    let loaded = store.get_task("t-0003").unwrap().unwrap();
    assert_eq!(loaded.resolved_prompt.as_deref(), Some("resolved prompt"));
}

#[test]
fn update_output_path_sets_field() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0005", AgentKind::Codex, TaskStatus::Pending);
    store.insert_task(&task).unwrap();
    store
        .update_output_path("t-0005", "/tmp/output.md")
        .unwrap();
    let loaded = store.get_task("t-0005").unwrap().unwrap();
    assert_eq!(loaded.output_path.as_deref(), Some("/tmp/output.md"));
}

#[test]
fn settle_pending_failure_persists_reason() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0006", AgentKind::Codex, TaskStatus::Pending);
    store.insert_task(&task).unwrap();

    let changed = store
        .settle_task_status(
            "t-0006",
            TaskStatus::Failed,
            Some(&[TaskStatus::Pending]),
            Some(PendingReason::RateLimited),
            None,
        )
        .unwrap();

    assert!(changed);
    let loaded = store.get_task("t-0006").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Failed);
    assert_eq!(loaded.pending_reason.as_deref(), Some("rate_limited"));
}

#[test]
fn list_running_filter() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-0010", AgentKind::Codex, TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task(
            "t-0012",
            AgentKind::Cursor,
            TaskStatus::AwaitingInput,
        ))
        .unwrap();
    store
        .insert_task(&make_task("t-0011", AgentKind::Grok, TaskStatus::Done))
        .unwrap();

    let running = store.list_tasks(TaskFilter::Running).unwrap();
    assert_eq!(running.len(), 2);
    let ids = running
        .into_iter()
        .map(|task| task.id.0)
        .collect::<Vec<_>>();
    assert!(ids.contains(&"t-0010".to_string()));
    assert!(ids.contains(&"t-0012".to_string()));
}

#[test]
fn gets_retry_chain_from_root_to_current() {
    let store = Store::open_memory().unwrap();
    let root = make_task("t-1001", AgentKind::Codex, TaskStatus::Done);
    let mut retry_1 = make_task("t-1002", AgentKind::Codex, TaskStatus::Failed);
    retry_1.parent_task_id = Some("t-1001".to_string());
    let mut retry_2 = make_task("t-1003", AgentKind::Codex, TaskStatus::Done);
    retry_2.parent_task_id = Some("t-1002".to_string());

    store.insert_task(&root).unwrap();
    store.insert_task(&retry_1).unwrap();
    store.insert_task(&retry_2).unwrap();

    let chain = store.get_retry_chain("t-1003").unwrap();
    let ids = chain
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["t-1001", "t-1002", "t-1003"]);
}

#[test]
fn recent_tasks_for_agent_filters_to_recent_done_tasks() {
    let store = Store::open_memory().unwrap();
    let now = chrono::Local::now();
    let mut recent = make_task("t-2001", AgentKind::Codex, TaskStatus::Done);
    recent.created_at = now - chrono::Duration::days(1);
    recent.duration_ms = Some(120_000);
    let mut older = make_task("t-2002", AgentKind::Codex, TaskStatus::Done);
    older.created_at = now - chrono::Duration::days(8);
    older.duration_ms = Some(240_000);
    let mut failed = make_task("t-2003", AgentKind::Codex, TaskStatus::Failed);
    failed.created_at = now - chrono::Duration::days(1);
    failed.duration_ms = Some(180_000);
    let mut other_agent = make_task("t-2004", AgentKind::Grok, TaskStatus::Done);
    other_agent.created_at = now - chrono::Duration::days(1);
    other_agent.duration_ms = Some(90_000);

    store.insert_task(&recent).unwrap();
    store.insert_task(&older).unwrap();
    store.insert_task(&failed).unwrap();
    store.insert_task(&other_agent).unwrap();

    let recent_tasks = store.recent_tasks_for_agent(AgentKind::Codex, 10).unwrap();
    let ids = recent_tasks
        .into_iter()
        .map(|task| task.id.0)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["t-2001".to_string()]);
}

#[test]
fn latest_default_model_prefers_most_recent_successful_gemini() {
    let store = Store::open_memory().unwrap();
    let now = chrono::Local::now();
    let mut g_old = make_task("t-la1", AgentKind::Grok, TaskStatus::Done);
    g_old.created_at = now - chrono::Duration::hours(10);
    g_old.observed_model = Some("gemini-2.5-flash".to_string());
    g_old.attribution_source = Some(AttributionSource::Echoed);

    let mut g_new = make_task("t-la2", AgentKind::Grok, TaskStatus::Done);
    g_new.created_at = now - chrono::Duration::hours(1);
    g_new.observed_model = Some("gemini-3.1-pro-preview".to_string());
    g_new.attribution_source = Some(AttributionSource::Echoed);

    let mut failed = make_task("t-la3", AgentKind::Grok, TaskStatus::Failed);
    failed.created_at = now;
    failed.observed_model = Some("should-ignore".to_string());
    failed.attribution_source = Some(AttributionSource::Echoed);

    store.insert_task(&g_old).unwrap();
    store.insert_task(&g_new).unwrap();
    store.insert_task(&failed).unwrap();

    assert_eq!(
        store
            .latest_default_model(AgentKind::Grok)
            .unwrap()
            .as_deref(),
        Some("gemini-3.1-pro-preview")
    );
}

/// An agent's learned default model must come from the CLI naming it, not from
/// aid inferring it because a run did not fail. Otherwise the first
/// confirmed-by-success row pins a default that nothing ever confirmed, and
/// every later dispatch inherits the inference as though it were fact.
#[test]
fn latest_default_model_ignores_a_merely_inferred_model() {
    let store = Store::open_memory().unwrap();
    let mut inferred = make_task("t-inf1", AgentKind::Codex, TaskStatus::Done);
    inferred.observed_model = Some("gpt-5.6-luna".to_string());
    inferred.attribution_source = Some(AttributionSource::ConfirmedBySuccess);
    store.insert_task(&inferred).unwrap();

    assert_eq!(store.latest_default_model(AgentKind::Codex).unwrap(), None);

    let mut echoed = make_task("t-inf2", AgentKind::Codex, TaskStatus::Done);
    echoed.observed_model = Some("gpt-5.6".to_string());
    echoed.attribution_source = Some(AttributionSource::Echoed);
    store.insert_task(&echoed).unwrap();

    assert_eq!(
        store
            .latest_default_model(AgentKind::Codex)
            .unwrap()
            .as_deref(),
        Some("gpt-5.6")
    );
}

#[path = "task_default_model_tests.rs"]
mod default_model;
