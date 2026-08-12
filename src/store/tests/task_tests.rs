// Task-focused Store tests.
// Exports: task query/mutation tests.
// Deps: Store, rusqlite.

use super::*;
use crate::store::TaskCompletionUpdate;

#[test]
fn insert_and_get_task() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0001", AgentKind::Codex, TaskStatus::Running);
    store.insert_task(&task).unwrap();

    let loaded = store.get_task("t-0001").unwrap().unwrap();
    assert_eq!(loaded.id, task.id);
    assert_eq!(loaded.agent, AgentKind::Codex);
    assert_eq!(loaded.status, TaskStatus::Running);
    assert_eq!(loaded.pending_reason, None);
}

#[test]
fn insert_and_get_task_persists_dispatch_flags() {
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-0004", AgentKind::Codex, TaskStatus::Pending);
    task.verify = Some("cargo test".to_string());
    task.exit_code = Some(0);
    task.read_only = true;
    task.budget = true;
    store.insert_task(&task).unwrap();

    let loaded = store.get_task("t-0004").unwrap().unwrap();
    assert_eq!(loaded.verify.as_deref(), Some("cargo test"));
    assert_eq!(loaded.exit_code, Some(0));
    assert!(loaded.read_only);
    assert!(loaded.budget);
}

#[test]
fn insert_and_update_task_persists_delivery_assessment() {
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-0004d", AgentKind::Codex, TaskStatus::Done);
    task.delivery_assessment = Some(DeliveryAssessment::HollowOutput);
    store.insert_task(&task).unwrap();

    let loaded = store.get_task("t-0004d").unwrap().unwrap();
    assert_eq!(loaded.delivery_assessment, Some(DeliveryAssessment::HollowOutput));

    store
        .update_delivery_assessment("t-0004d", Some(DeliveryAssessment::EmptyDiff))
        .unwrap();
    let updated = store.get_task("t-0004d").unwrap().unwrap();
    assert_eq!(updated.delivery_assessment, Some(DeliveryAssessment::EmptyDiff));

    store.update_delivery_assessment("t-0004d", None).unwrap();
    let cleared = store.get_task("t-0004d").unwrap().unwrap();
    assert_eq!(cleared.delivery_assessment, None);
}

#[test]
fn update_task_final_state_persists_terminal_git_state() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-final-state", AgentKind::Codex, TaskStatus::Done);
    store.insert_task(&task).unwrap();

    store
        .update_task_final_state("t-final-state", Some("abc123"), Some("feat/final"))
        .unwrap();

    let loaded = store.get_task("t-final-state").unwrap().unwrap();
    assert_eq!(loaded.final_head_sha.as_deref(), Some("abc123"));
    assert_eq!(loaded.final_branch.as_deref(), Some("feat/final"));
}

#[test]
fn insert_waiting_task_persists_retry_fields() {
    let store = Store::open_memory().unwrap();
    let prompt = "x".repeat(160);
    store
        .insert_waiting_task(
            "t-0004w",
            "codex",
            &prompt,
            Some("resolved prompt"),
            Some("wg-batch"),
            Some("/tmp/repo"),
            Some("feat/retry"),
            Some("o3"),
            Some("cargo check"),
            true,
            true,
        )
        .unwrap();

    let loaded = store.get_task("t-0004w").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Waiting);
    assert_eq!(loaded.prompt, prompt);
    assert_eq!(loaded.resolved_prompt.as_deref(), Some("resolved prompt"));
    assert_eq!(loaded.workgroup_id.as_deref(), Some("wg-batch"));
    assert_eq!(loaded.repo_path.as_deref(), Some("/tmp/repo"));
    assert_eq!(loaded.worktree_branch.as_deref(), Some("feat/retry"));
    assert_eq!(loaded.requested_model.as_deref(), Some("o3"));
    assert_eq!(loaded.verify.as_deref(), Some("cargo check"));
    assert!(loaded.read_only);
    assert!(loaded.budget);
}

#[test]
fn migrate_adds_repo_path_column() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            agent TEXT NOT NULL,
            prompt TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            worktree_path TEXT,
            worktree_branch TEXT,
            log_path TEXT,
            output_path TEXT,
            tokens INTEGER,
            duration_ms INTEGER,
            created_at TEXT NOT NULL,
            completed_at TEXT
        );
        CREATE TABLE events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            event_type TEXT NOT NULL,
            detail TEXT NOT NULL
        );",
    )
    .unwrap();
    let store = Store {
        conn: std::sync::Mutex::new(conn),
    };

    store.migrate().unwrap();

    let conn = store.db();
    let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(|row| row.unwrap())
        .collect::<Vec<_>>();
    assert!(columns.contains(&"repo_path".to_string()));
    assert!(columns.contains(&"resolved_prompt".to_string()));
    assert!(columns.contains(&"verify".to_string()));
    assert!(columns.contains(&"read_only".to_string()));
    assert!(columns.contains(&"budget".to_string()));
    assert!(columns.contains(&"category".to_string()));
    assert!(columns.contains(&"pending_reason".to_string()));
    assert!(columns.contains(&"delivery_assessment".to_string()));
    assert!(columns.contains(&"final_head_sha".to_string()));
    assert!(columns.contains(&"final_branch".to_string()));
    assert!(columns.contains(&"project_id".to_string()));
}

#[test]
fn project_id_round_trips_and_null_stays_unattributed() {
    let store = Store::open_memory().unwrap();
    let mut attributed = make_task("t-proj-a", AgentKind::Codex, TaskStatus::Done);
    attributed.project_id = Some("ai-dispatch".into());
    store.insert_task(&attributed).unwrap();
    let unattributed = make_task("t-proj-u", AgentKind::Codex, TaskStatus::Done);
    store.insert_task(&unattributed).unwrap();

    let loaded_a = store.get_task("t-proj-a").unwrap().unwrap();
    assert_eq!(loaded_a.project_id.as_deref(), Some("ai-dispatch"));
    let loaded_u = store.get_task("t-proj-u").unwrap().unwrap();
    assert!(loaded_u.project_id.is_none());
}

#[test]
fn migrate_moves_legacy_delivery_status_out_of_verify_status() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            agent TEXT NOT NULL,
            prompt TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL,
            verify_status TEXT NOT NULL DEFAULT 'skipped'
        );
        INSERT INTO tasks (id, agent, prompt, status, created_at, verify_status)
        VALUES ('t-empty', 'codex', 'prompt', 'done', '2026-04-16T00:00:00+00:00', 'empty_diff');
        CREATE TABLE events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            event_type TEXT NOT NULL,
            detail TEXT NOT NULL
        );",
    )
    .unwrap();
    let store = Store {
        conn: std::sync::Mutex::new(conn),
    };

    store.migrate().unwrap();

    let conn = store.db();
    let (verify_status, delivery_assessment): (String, Option<String>) = conn
        .query_row(
            "SELECT verify_status, delivery_assessment FROM tasks WHERE id = 't-empty'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(verify_status, "skipped");
    assert_eq!(delivery_assessment.as_deref(), Some("empty_diff"));
}

/// The defect this split exists for: a CLI that reports no model must leave
/// `observed_model` NULL rather than having the dispatched request copied into
/// it. Before the split, `info.model.as_deref().or(model)` recorded
/// `gemini-3.6-flash-low` as the model the `claude` CLI ran (`t-bd455a68`) and
/// cursor's `composer-2` as an `agy` model (`t-702f7bcb`) — both failed runs,
/// both stored as fact.
#[test]
fn completion_without_a_reported_model_leaves_the_request_alone() {
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-attr1", AgentKind::Claude, TaskStatus::Running);
    task.requested_model = Some("gemini-3.6-flash-low".to_string());
    store.insert_task(&task).unwrap();

    store
        .update_task_completion(TaskCompletionUpdate {
            id: "t-attr1",
            status: TaskStatus::Done,
            tokens: Some(10),
            duration_ms: 1000,
            observed_model: None, attribution_source: None,
            cost_usd: None,
            exit_code: Some(0),
        })
        .unwrap();

    let loaded = store.get_task("t-attr1").unwrap().unwrap();
    assert_eq!(loaded.requested_model.as_deref(), Some("gemini-3.6-flash-low"));
    assert_eq!(loaded.observed_model, None, "a silent CLI must not be quoted");
    assert_eq!(loaded.attributed_model(), None, "unknown must stay unknown");
    assert_eq!(loaded.costing_model(), Some("gemini-3.6-flash-low"));
}

/// A later completion write that learned nothing must not erase an observation
/// an earlier one made — hence COALESCE rather than plain assignment.
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
    let task = make_task("t-0002", AgentKind::Gemini, TaskStatus::Running);
    store.insert_task(&task).unwrap();
    store
        .update_task_completion(TaskCompletionUpdate {
            id: "t-0002",
            status: TaskStatus::Done,
            tokens: Some(3000),
            duration_ms: 47000,
            observed_model: Some("gemini-2.5-flash"), attribution_source: None,
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
                observed_model: Some("gpt-5.4"), attribution_source: None,
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
    store.update_output_path("t-0005", "/tmp/output.md").unwrap();
    let loaded = store.get_task("t-0005").unwrap().unwrap();
    assert_eq!(loaded.output_path.as_deref(), Some("/tmp/output.md"));
}

#[test]
fn fail_pending_with_reason_persists_reason() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0006", AgentKind::Codex, TaskStatus::Pending);
    store.insert_task(&task).unwrap();

    let changed = store
        .fail_pending_with_reason("t-0006", PendingReason::RateLimited)
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
        .insert_task(&make_task("t-0011", AgentKind::Gemini, TaskStatus::Done))
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
    let mut other_agent = make_task("t-2004", AgentKind::Gemini, TaskStatus::Done);
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
    let mut g_old = make_task("t-la1", AgentKind::Gemini, TaskStatus::Done);
    g_old.created_at = now - chrono::Duration::hours(10);
    g_old.observed_model = Some("gemini-2.5-flash".to_string());
    g_old.attribution_source = Some(AttributionSource::Echoed);

    let mut g_new = make_task("t-la2", AgentKind::Gemini, TaskStatus::Done);
    g_new.created_at = now - chrono::Duration::hours(1);
    g_new.observed_model = Some("gemini-3.1-pro-preview".to_string());
    g_new.attribution_source = Some(AttributionSource::Echoed);

    let mut failed = make_task("t-la3", AgentKind::Gemini, TaskStatus::Failed);
    failed.created_at = now;
    failed.observed_model = Some("should-ignore".to_string());
    failed.attribution_source = Some(AttributionSource::Echoed);

    store.insert_task(&g_old).unwrap();
    store.insert_task(&g_new).unwrap();
    store.insert_task(&failed).unwrap();

    assert_eq!(
        store.latest_default_model(AgentKind::Gemini).unwrap().as_deref(),
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
        store.latest_default_model(AgentKind::Codex).unwrap().as_deref(),
        Some("gpt-5.6")
    );
}

/// A Done task with hollow delivery is not a success. Learning a default model
/// from it would route future work based on a run that produced nothing.
#[test]
fn latest_default_model_skips_hollow_delivery_done_tasks() {
    let store = Store::open_memory().unwrap();
    let now = chrono::Local::now();

    let mut hollow = make_task("t-hollow-new", AgentKind::Codex, TaskStatus::Done);
    hollow.created_at = now;
    hollow.observed_model = Some("gpt-hollow".to_string());
    hollow.attribution_source = Some(AttributionSource::Echoed);
    hollow.delivery_assessment = Some(DeliveryAssessment::HollowOutput);

    let mut good = make_task("t-good-old", AgentKind::Codex, TaskStatus::Done);
    good.created_at = now - chrono::Duration::hours(2);
    good.observed_model = Some("gpt-good".to_string());
    good.attribution_source = Some(AttributionSource::Echoed);

    store.insert_task(&hollow).unwrap();
    store.insert_task(&good).unwrap();

    assert_eq!(
        store.latest_default_model(AgentKind::Codex).unwrap().as_deref(),
        Some("gpt-good")
    );
}
