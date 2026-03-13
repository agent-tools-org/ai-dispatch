// Task-focused Store tests.
// Exports: task query/mutation tests.
// Deps: Store, rusqlite.

use super::*;

#[test]
fn insert_and_get_task() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0001", AgentKind::Codex, TaskStatus::Running);
    store.insert_task(&task).unwrap();

    let loaded = store.get_task("t-0001").unwrap().unwrap();
    assert_eq!(loaded.id, task.id);
    assert_eq!(loaded.agent, AgentKind::Codex);
    assert_eq!(loaded.status, TaskStatus::Running);
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
}

#[test]
fn update_completion() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-0002", AgentKind::Gemini, TaskStatus::Running);
    store.insert_task(&task).unwrap();
    store
        .update_task_completion(
            "t-0002",
            TaskStatus::Done,
            Some(3000),
            47000,
            Some("gemini-2.5-flash"),
            Some(0.0038),
        )
        .unwrap();

    let loaded = store.get_task("t-0002").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);
    assert_eq!(loaded.tokens, Some(3000));
    assert_eq!(loaded.duration_ms, Some(47000));
    assert_eq!(loaded.model.as_deref(), Some("gemini-2.5-flash"));
    assert!((loaded.cost_usd.unwrap() - 0.0038).abs() < 0.0001);
    assert!(loaded.completed_at.is_some());
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
