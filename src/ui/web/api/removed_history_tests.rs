// Web history regression for removed execution agents.
// Covers row decoding, task/event APIs, named statistics, and quota markers.
// Deps: parent API handlers, Store, rusqlite, isolated aid home.
use super::*;
use rusqlite::params;

#[tokio::test]
async fn removed_agent_rows_and_events_keep_plain_names_in_web_history() {
    let home = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    seed_history(&store, home.path());
    let Json(tasks) = list_tasks(Query(TaskListParams { filter: None }), State(store.clone()))
        .await
        .expect("list");
    assert_eq!(tasks.len(), 10);
    assert!(
        tasks
            .iter()
            .all(|task| matches!(task.agent.as_str(), "gemini" | "qwen"))
    );
    let Json(usage) = get_usage(State(store.clone())).await.expect("usage");
    assert_eq!(usage.agents.len(), 2);
    for row in usage.agents {
        assert!(matches!(row.agent.as_str(), "gemini" | "qwen"));
        assert_eq!(row.task_count, 5);
        assert_eq!(row.avg_cost, Some(0.25));
    }
    assert!(
        store
            .agent_success_rates()
            .expect("routing metrics")
            .is_empty()
    );
}

fn seed_history(store: &Store, home: &std::path::Path) {
    for agent in ["gemini", "qwen"] {
        seed_agent_tasks(store, agent);
        std::fs::write(
            home.join(format!("rate-limit-{agent}")),
            "recovery_at: Jan 01, 2099 12:00 PM\nmessage: Historical quota\n",
        )
        .expect("marker");
        let marker = crate::routing::rate_limit::get_rate_limit_info(
            &crate::types::AgentKind::Custom,
            Some(agent),
        )
        .expect("old marker");
        assert_eq!(marker.message.as_deref(), Some("Historical quota"));
    }
}

fn seed_agent_tasks(store: &Store, agent: &str) {
    for index in 0..5 {
        let id = format!("t-old-{agent}-{index}");
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at, cost_usd)
            VALUES (?1, ?2, 'Old task', 'done', ?3, 0.25)",
                params![id, agent, chrono::Local::now().to_rfc3339()],
            )
            .expect("task");
        store
            .db()
            .execute(
                "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
            VALUES (?1, ?2, 'completion', 'Old completion', ?3)",
                params![
                    id,
                    chrono::Local::now().to_rfc3339(),
                    serde_json::json!({"agent": agent}).to_string()
                ],
            )
            .expect("event");
        let task = store.get_task(&id).expect("load").expect("task");
        assert_eq!(task.agent_display_name(), agent);
        let events = store.get_events(&id).expect("events");
        assert_eq!(
            events[0].metadata.as_ref().expect("metadata")["agent"],
            agent
        );
    }
}
