// CLI maintenance tests for dry runs, cadence and cache-field preservation.
// Uses in-memory stores and isolated temporary homes only.
use super::*;
use serde_json::json;

fn seed(store: &Store) {
    store
        .db()
        .execute_batch(
            "INSERT INTO tasks (id,agent,prompt,status,created_at,completed_at)
         VALUES ('old','codex','test','done','2020-01-01','2020-01-01');",
        )
        .unwrap();
    store
        .db()
        .execute(
            "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
         VALUES ('old','2020-01-01','build','output',?1)",
            [json!({"full": "x".repeat(5000), "command": "build", "exit_code": 0}).to_string()],
        )
        .unwrap();
}

#[test]
fn clean_dry_run_changes_neither_events_nor_state_even_with_vacuum() {
    let home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    seed(&store);
    let changes = store.db().total_changes();
    run(&store, 7, true, true).unwrap();
    assert_eq!(store.db().total_changes(), changes);
    assert!(!home.path().join("update-check.json").exists());
    assert_eq!(
        store
            .trim_event_payloads(&cutoff(7).unwrap(), 0, true, Duration::from_secs(1))
            .unwrap()
            .bytes,
        5000
    );
}

#[test]
fn automatic_trim_runs_once_daily_and_preserves_update_cache() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("update-check.json");
    std::fs::write(&path, r#"{"last_check":"today","latest_version":"1.0.0"}"#).unwrap();
    let store = Store::open_memory().unwrap();
    seed(&store);
    auto_trim_at(&store, &path, 100_000).unwrap();
    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(state["latest_version"], "1.0.0");
    assert_eq!(state["payload_trim_last_run"], 100_000);
    let changes = store.db().total_changes();
    auto_trim_at(&store, &path, 100_001).unwrap();
    assert_eq!(store.db().total_changes(), changes);
    auto_trim_at(&store, &path, 186_400).unwrap();
    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(state["payload_trim_last_run"], 186_400);
}

#[test]
fn rejects_retention_overflow_and_defers_when_cache_is_locked() {
    assert!(cutoff(u64::MAX).is_err());
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("update-check.json");
    let store = Store::open_memory().unwrap();
    with_cache_state(&path, |_| {
        auto_trim_at(&store, &path, 100_000)?;
        assert!(!path.exists());
        Ok(Some(()))
    })
    .unwrap();
}
