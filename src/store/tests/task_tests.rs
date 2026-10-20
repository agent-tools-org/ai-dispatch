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
fn insert_and_get_task_roundtrips_effective_dir() {
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-0004e", AgentKind::Codex, TaskStatus::Done);
    task.effective_dir = Some("/tmp/audit-dir".to_string());
    task.output_path = Some("report.md".to_string());
    store.insert_task(&task).unwrap();

    let loaded = store.get_task("t-0004e").unwrap().unwrap();
    assert_eq!(loaded.effective_dir.as_deref(), Some("/tmp/audit-dir"));
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

#[path = "task_tests_more_1.rs"]
mod more;
