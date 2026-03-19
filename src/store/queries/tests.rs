// Query module tests covering task, event, and memory query behavior.
// Exports: test-only helpers and regression coverage for split query modules.
// Deps: crate::store::Store, crate::types, rusqlite.

use chrono::{Duration, Local};
use rusqlite::params;

use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use crate::usage::parse_window;

fn insert_task(
    store: &Store,
    id: &str,
    agent: AgentKind,
    status: TaskStatus,
    prompt: &str,
) {
    let conn = store.db();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, agent.as_str(), prompt, status.as_str(), "2026-03-15T00:00:00Z"],
    )
    .unwrap();
}

fn insert_event(
    store: &Store,
    task_id: &str,
    timestamp: &str,
    event_type: &str,
    detail: &str,
    metadata: Option<&str>,
) {
    let conn = store.db();
    conn.execute(
        "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![task_id, timestamp, event_type, detail, metadata],
    )
    .unwrap();
}

#[test]
fn finds_matching_tasks_with_keyword_scores() {
    let store = Store::open_memory().unwrap();
    insert_task(
        &store,
        "t-routing",
        AgentKind::Codex,
        TaskStatus::Done,
        "Implement cross-session hints for agent selection",
    );
    insert_task(
        &store,
        "t-gemini",
        AgentKind::Gemini,
        TaskStatus::Done,
        "Document routing hints and selection strategy",
    );
    insert_task(
        &store,
        "t-cursor",
        AgentKind::Cursor,
        TaskStatus::Failed,
        "Cleanup the build pipeline wiring",
    );

    let results = store
        .find_similar_tasks("Add routing hints for agent selection", 5)
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "t-gemini");
    assert_eq!(results[0].1, AgentKind::Gemini);
    assert_eq!(results[1].0, "t-routing");
    assert_eq!(results[1].1, AgentKind::Codex);
}

#[test]
fn includes_merged_tasks_in_results() {
    let store = Store::open_memory().unwrap();
    insert_task(
        &store,
        "t-merged",
        AgentKind::Codex,
        TaskStatus::Merged,
        "Implement routing hints for budget selection",
    );
    let results = store
        .find_similar_tasks("Add routing hints for selection", 5)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "t-merged");
    assert_eq!(results[0].2, TaskStatus::Merged);
}

#[test]
fn limits_results_to_positive_scores() {
    let store = Store::open_memory().unwrap();
    insert_task(
        &store,
        "t-empty",
        AgentKind::Kilo,
        TaskStatus::Done,
        "Refactor the logging toolchain",
    );
    let results = store.find_similar_tasks("Short", 3).unwrap();
    assert!(results.is_empty());
}

#[test]
fn latest_awaiting_reasons_batch_returns_latest_prompt_per_task() {
    let store = Store::open_memory().unwrap();
    insert_task(&store, "t-await-1", AgentKind::Codex, TaskStatus::AwaitingInput, "Prompt one");
    insert_task(&store, "t-await-2", AgentKind::Gemini, TaskStatus::AwaitingInput, "Prompt two");
    insert_task(&store, "t-other", AgentKind::Cursor, TaskStatus::Running, "Prompt three");
    insert_event(
        &store,
        "t-await-1",
        "2026-03-15T00:00:00Z",
        "status",
        "awaiting",
        Some(r#"{"awaiting_input":true,"awaiting_prompt":"First prompt"}"#),
    );
    insert_event(
        &store,
        "t-await-1",
        "2026-03-15T01:00:00Z",
        "status",
        "awaiting",
        Some(r#"{"awaiting_input":true,"awaiting_prompt":"Latest prompt"}"#),
    );
    insert_event(
        &store,
        "t-await-2",
        "2026-03-15T02:00:00Z",
        "status",
        "awaiting",
        Some(r#"{"awaiting_input":true,"awaiting_prompt":"Second task prompt"}"#),
    );
    insert_event(
        &store,
        "t-other",
        "2026-03-15T03:00:00Z",
        "status",
        "running",
        Some(r#"{"awaiting_input":false,"awaiting_prompt":"Ignored"}"#),
    );

    let reasons = store
        .latest_awaiting_reasons_batch(&["t-await-1", "t-await-2", "t-other"])
        .unwrap();

    assert_eq!(reasons.len(), 2);
    assert_eq!(reasons.get("t-await-1").map(String::as_str), Some("Latest prompt"));
    assert_eq!(reasons.get("t-await-2").map(String::as_str), Some("Second task prompt"));
    assert!(!reasons.contains_key("t-other"));
}

#[test]
fn latest_awaiting_reasons_batch_skips_missing_prompts() {
    let store = Store::open_memory().unwrap();
    insert_task(&store, "t-await", AgentKind::Codex, TaskStatus::AwaitingInput, "Prompt");
    insert_event(
        &store,
        "t-await",
        "2026-03-15T00:00:00Z",
        "status",
        "awaiting",
        Some(r#"{"awaiting_input":true}"#),
    );

    let reasons = store.latest_awaiting_reasons_batch(&["t-await"]).unwrap();

    assert!(reasons.is_empty());
}

#[test]
fn aggregates_budget_usage_by_agent_and_window() {
    let store = Store::open_memory().unwrap();
    let conn = store.db();
    let now = Local::now();
    let within_window = (now - Duration::hours(6)).to_rfc3339();
    let outside_window = (now - Duration::days(2)).to_rfc3339();

    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, tokens, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["t-recent-1", "codex", "recent", "done", 120_i64, 1.25_f64, &within_window],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, tokens, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["t-recent-2", "codex", "recent", "done", 80_i64, 0.75_f64, &within_window],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, tokens, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["t-old", "codex", "old", "done", 500_i64, 4.0_f64, &outside_window],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, tokens, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["t-other", "gemini", "other", "done", 999_i64, 9.0_f64, &within_window],
    )
    .unwrap();
    drop(conn);

    let all_time = store.budget_usage_summary("codex", None).unwrap();
    assert_eq!(all_time, (3, 700, 6.0));

    let since = parse_window("24h").map(|window| now - window).unwrap();
    let recent = store.budget_usage_summary("codex", Some(since)).unwrap();
    assert_eq!(recent, (2, 200, 2.0));
}

#[test]
fn search_memories_escapes_wildcards() {
    let store = Store::open_memory().unwrap();
    let conn = store.db();
    conn.execute(
        "INSERT INTO memories (id, memory_type, content, source_task_id, agent, project_path,
         content_hash, created_at, expires_at, supersedes, version, inject_count, last_injected_at,
         success_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            "m-100-percent",
            "task_output",
            "100% success rate",
            Option::<String>::None,
            Option::<String>::None,
            Option::<String>::None,
            "hash-100-percent",
            "2026-03-15T00:00:00Z",
            Option::<String>::None,
            Option::<String>::None,
            1,
            0,
            Option::<String>::None,
            0,
        ],
    )
    .unwrap();
    drop(conn);

    let percent_results = store.search_memories("100%", None, 10).unwrap();
    assert_eq!(percent_results.len(), 1);
    assert_eq!(percent_results[0].content, "100% success rate");

    let plain_results = store.search_memories("100", None, 10).unwrap();
    assert_eq!(plain_results.len(), 1);
    assert_eq!(plain_results[0].content, "100% success rate");
}
