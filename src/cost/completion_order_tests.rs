// Cost regressions for chronological completion selection and timestamp ties.
// Compares component pricing to the original full-history reverse traversal.

use super::*;
use crate::cost::test_support::task;

#[test]
fn completion_cost_uses_timestamp_before_insertion_id_and_breaks_ties_by_id() {
    let store = Store::open_memory().expect("store");
    let task = task(AgentKind::Codex, 10, Some(99.0));
    store.insert_task(&task).expect("task");
    for (timestamp, cost) in [
        ("2026-10-23T12:00:00Z", 2),
        ("2026-10-23T11:00:00Z", 1),
        ("2026-10-23T12:00:00Z", 3),
    ] {
        store
            .db()
            .execute(
                "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES (?1, ?2, 'completion', 'complete', ?3)",
                rusqlite::params![
                    task.id.as_str(),
                    timestamp,
                    serde_json::json!({"tokens": 10, "cost_usd": cost}).to_string()
                ],
            )
            .expect("completion");
        let expected = store
            .get_events(task.id.as_str())
            .expect("history")
            .into_iter()
            .rev()
            .filter(|event| event.event_kind == EventKind::Completion)
            .filter_map(|event| event.metadata)
            .find(|metadata| metadata.get("tokens").is_some() || metadata.get("cost_usd").is_some())
            .and_then(|metadata| metadata.get("cost_usd").and_then(Value::as_f64));
        assert_eq!(task_cost(&store, &task).expect("cost"), expected);
        assert_eq!(expected, Some(if cost == 1 { 2.0 } else { cost as f64 }));
    }
}
