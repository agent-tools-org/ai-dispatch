// Overage query regression coverage for event kind, periods, deduplication and query work.
// In-memory SQLite avoids external state; both routing path overrides are isolated.

use super::*;
use crate::routing::quota::live::CacheDirGuard;


fn insert(store: &Store, id: &str, created_at: &str, kind: &str, metadata: &str) {
    store
        .db()
        .execute(
            "INSERT OR IGNORE INTO tasks (id, agent, prompt, created_at) VALUES (?1, 'codex', '', ?2)",
            params![id, created_at],
        )
        .expect("task");
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         VALUES (?1, ?2, ?3, '', ?4)",
            params![id, created_at, kind, metadata],
        )
        .expect("event");
}

#[test]
fn overage_lookup_filters_kind_and_period_and_deduplicates_tasks() {
    let temp = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().expect("store");
    let now = Local::now();
    let current = (now - chrono::Duration::hours(1)).to_rfc3339();
    let old = (now - chrono::Duration::days(10)).to_rfc3339();
    let overage = r#"{"billing_state":"overage"}"#;
    insert(&store, "current", &current, "setup", overage);
    insert(&store, "current", &current, "setup", overage);
    insert(&store, "old", &old, "setup", overage);
    insert(&store, "wrong-kind", &current, "completion", overage);
    insert(
        &store,
        "in-plan",
        &current,
        "setup",
        r#"{"billing_state":"in_plan"}"#,
    );
    insert(&store, "malformed", &current, "setup", "malformed JSON");
    let ids = store
        .overage_task_ids(Some((now - chrono::Duration::days(7), now)))
        .expect("query");
    assert_eq!(ids, HashSet::from(["current".to_string()]));
    assert_eq!(store.overage_task_ids(None).expect("all").len(), 2);
}

#[test]
fn empty_overage_lookup_work_does_not_grow_with_unrelated_history() {
    let temp = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().expect("store");
    let before = query_steps(&store);
    for i in 0..100 {
        insert(
            &store,
            &format!("task-{i}"),
            &Local::now().to_rfc3339(),
            "setup",
            "{}",
        );
    }
    assert!(query_steps(&store) <= before, "unrelated rows must not be scanned");
    assert!(store.overage_task_ids(None).expect("empty").is_empty());
}

fn query_steps(store: &Store) -> i32 {
    let conn = store.db();
    let mut stmt = conn.prepare(OVERAGE_TASK_IDS_SQL).expect("prepare");
    let rows = stmt
        .query_map(params![Option::<String>::None, Option::<String>::None], |row| {
            row.get::<_, String>(0)
        })
        .expect("query");
    assert!(rows.collect::<Result<Vec<_>, _>>().expect("rows").is_empty());
    stmt.get_status(rusqlite::StatementStatus::VmStep)
}
