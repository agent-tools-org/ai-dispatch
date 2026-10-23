// Retention integration tests against SQLite, including bounded writes and VACUUM.
// Uses disposable databases only; checks preserved evidence and restart behavior.
use super::*;
use serde_json::json;

fn task(store: &Store, id: &str, status: &str, completed: &str) {
    store
        .db()
        .execute(
            "INSERT INTO tasks (id,agent,prompt,status,created_at,completed_at)
         VALUES (?1,'codex','test',?2,'2020-01-01',?3)",
            params![id, status, completed],
        )
        .unwrap();
}

fn event(store: &Store, id: &str, kind: &str, metadata: &serde_json::Value) {
    store
        .db()
        .execute(
            "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
         VALUES (?1,'2020-01-01',?2,'preserved detail',?3)",
            params![id, kind, metadata.to_string()],
        )
        .unwrap();
}

fn trim_all(store: &Store, dry: bool) -> TrimReport {
    store
        .trim_event_payloads("2021-01-01", 0, dry, Duration::from_secs(10))
        .unwrap()
}

#[test]
fn trims_only_allowlisted_old_terminal_output_and_is_idempotent() {
    let store = Store::open_memory().unwrap();
    let evidence = evidence();
    for (id, status, completed) in [
        ("old", "done", "2020-01-01"),
        ("running", "running", "2020-01-01"),
        ("recent", "done", "2022-01-01"),
        ("unknown", "done", "invalid"),
    ] {
        task(&store, id, status, completed);
        event(&store, id, "test", &evidence);
    }
    for kind in ["completion", "reasoning", "error", "milestone"] {
        event(&store, "old", kind, &evidence);
    }
    event(
        &store,
        "old",
        "test",
        &json!({"full": "x".repeat(4095), "command": "build", "exit_code": 0}),
    );
    let report = trim_all(&store, false);
    assert_eq!(report.rows, 1);
    assert_eq!(report.bytes, 4096);
    let conn = store.db();
    let raw: String = conn
        .query_row("SELECT metadata FROM events WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let mut expected = evidence.clone();
    expected["full"] = json!({"trimmed": true, "bytes": 4096});
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&raw).unwrap(),
        expected
    );
    let untouched: i64 = conn
        .query_row(
            "SELECT count(*) FROM events WHERE id>1 AND metadata=?1",
            [evidence.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(untouched, 7);
    let detail: String = conn
        .query_row("SELECT detail FROM events WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(detail, "preserved detail");
    drop(conn);
    assert_eq!(trim_all(&store, false).rows, 0);
}

#[test]
fn batches_commit_with_limits_and_resume_after_interruption() {
    let store = Store::open_memory().unwrap();
    task(&store, "old", "done", "2020-01-01");
    for _ in 0..BATCH_ROWS + 7 {
        event(
            &store,
            "old",
            "build",
            &json!({"full": "x".repeat(4096), "command": "build", "exit_code": 0}),
        );
    }
    // Fail a later write: the earlier committed batch must survive.
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER interrupt_trim BEFORE UPDATE ON events WHEN NEW.id=150
         BEGIN SELECT RAISE(ABORT, 'interrupted'); END;",
        )
        .unwrap();
    assert!(
        store
            .trim_event_payloads("2021-01-01", 0, false, Duration::from_secs(10))
            .is_err()
    );
    let committed: i64 = store
        .db()
        .query_row(
            "SELECT count(*) FROM events WHERE json_extract(metadata,'$.full.trimmed')=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(committed > 0 && committed <= BATCH_ROWS as i64);
    store
        .db()
        .execute_batch("DROP TRIGGER interrupt_trim;")
        .unwrap();
    let rest = store
        .trim_event_payloads("2021-01-01", committed, false, Duration::from_secs(10))
        .unwrap();
    assert_eq!(rest.rows + committed as u64, (BATCH_ROWS + 7) as u64);
    assert!(rest.batches >= (rest.rows as usize).div_ceil(BATCH_ROWS));
    assert!(rest.largest_batch <= BATCH_ROWS);
    assert!(rest.complete);
    assert_eq!(trim_all(&store, false).rows, 0);
}

#[test]
fn dry_run_and_zero_budget_leave_database_unchanged() {
    let store = Store::open_memory().unwrap();
    task(&store, "old", "done", "2020-01-01");
    let value = json!({"full": "x".repeat(9000), "command": "build", "exit_code": 0});
    event(&store, "old", "tool_call", &value);
    let before = store.db().total_changes();
    let report = trim_all(&store, true);
    assert_eq!((report.rows, report.bytes), (1, 9000));
    assert_eq!(store.db().total_changes(), before);
    let interrupted = store
        .trim_event_payloads("2021-01-01", 0, false, Duration::ZERO)
        .unwrap();
    assert_eq!(interrupted.cursor, 0);
    assert!(!interrupted.complete);
    assert_eq!(trim_all(&store, false).rows, 1);
}

#[test]
fn idle_vacuum_shrinks_after_trim_and_refuses_active_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("test.db")).unwrap();
    task(&store, "old", "done", "2020-01-01");
    for _ in 0..20 {
        event(
            &store,
            "old",
            "tool_call",
            &json!({"full": "x".repeat(100_000), "command": "build", "exit_code": 0}),
        );
    }
    store
        .db()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    task(&store, "active", "running", "2020-01-01");
    assert!(
        store
            .vacuum_events()
            .unwrap_err()
            .to_string()
            .contains("refused")
    );
    store
        .db()
        .execute("UPDATE tasks SET status='waiting' WHERE id='active'", [])
        .unwrap();
    assert!(
        store
            .vacuum_events()
            .unwrap_err()
            .to_string()
            .contains("refused")
    );
    store
        .db()
        .execute("UPDATE tasks SET status='done' WHERE id='active'", [])
        .unwrap();
    assert_eq!(trim_all(&store, false).rows, 20);
    let (before, after) = store.vacuum_events().unwrap();
    assert!(after < before / 2, "before={before}, after={after}");
    assert!(
        require_space(2_000_000, 1)
            .unwrap_err()
            .to_string()
            .contains("free disk")
    );
}

fn evidence() -> serde_json::Value {
    json!({
        "full": "x".repeat(4096), "exit_code": 0, "tokens": 100, "cost": 2.50, "model": "model",
        "api_route": "route", "billing_state": "overage", "custody": {"accepted": true},
        "completion": {"status": "done"}, "delivery": "evidence", "input_delivery": "input",
        "liveness": "alive", "backup_url": "backup", "quota": "quota", "detail": "original",
        "provider_stdout": "p".repeat(5000), "command": "c".repeat(5000), "stderr": "e".repeat(5000)
    })
}

#[test]
fn preserves_inputs_and_nonterminal_tasks_and_trims_all_terminal_statuses() {
    let store = Store::open_memory().unwrap();
    let output = evidence();
    for status in [
        "done",
        "merged",
        "failed",
        "skipped",
        "stopped",
        "pending",
        "running",
        "waiting",
        "awaiting_input",
        "stalled",
    ] {
        task(&store, status, status, "2020-01-01");
        event(&store, status, "build", &output);
    }
    let mut input = output.clone();
    input["status"] = json!("in_progress");
    event(&store, "done", "tool_call", &input);
    input = output.clone();
    input["full"] = input["command"].clone();
    event(&store, "done", "build", &input);
    input = output.clone();
    input.as_object_mut().unwrap().remove("exit_code");
    event(&store, "done", "test", &input);
    assert_eq!(trim_all(&store, false).rows, 5);
    assert_eq!(trim_all(&store, false).rows, 0);
}
