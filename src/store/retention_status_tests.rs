// Status-specific retention coverage for successful and unsuccessful terminal tasks.
// Checks provider payloads, all full event types, protected evidence, and status races.
use super::*;
use serde_json::{Value, json};

fn seed(store: &Store, status: &str) -> Vec<Value> {
    store
        .db()
        .execute(
            "INSERT INTO tasks (id,agent,prompt,status,created_at,completed_at)
         VALUES ('old','codex','test',?1,'2020-01-01','2020-01-01')",
            [status],
        )
        .unwrap();
    let value = json!({
        "full": "x".repeat(4096), "provider_stdout": "p".repeat(5000),
        "command": "c".repeat(5000), "exit_code": 1, "tokens": 100, "cost": 2.5,
        "model": "model", "api_route": "route", "billing_state": "overage",
        "custody": {"accepted": true}, "completion": {"status": "done"},
        "delivery": "evidence", "input_delivery": "input", "liveness": "alive",
        "backup_url": "backup", "quota": "quota", "detail": "original"
    });
    let mut originals = Vec::new();
    for kind in [
        "reasoning",
        "completion",
        "error",
        "build",
        "tool_call",
        "milestone",
    ] {
        insert(store, kind, &value);
        originals.push(value.clone());
    }
    for value in [
        json!({"full": "x".repeat(4095), "provider_stdout": "p".repeat(4095)}),
        json!({"provider_stdout": "p".repeat(5000)}),
        json!({"full": "x".repeat(4096), "command": "build", "exit_code": 0}),
        json!({"full": "x".repeat(5000)}),
        json!({"full": "c".repeat(5000), "command": "c".repeat(5000), "status": "in_progress"}),
    ] {
        insert(store, "build", &value);
        originals.push(value);
    }
    originals
}

fn insert(store: &Store, kind: &str, value: &Value) {
    store
        .db()
        .execute(
            "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
         VALUES ('old','2020-01-01',?1,'preserved detail',?2)",
            params![kind, value.to_string()],
        )
        .unwrap();
}

fn status_case(status: &str, successful: bool) {
    let store = Store::open_memory().unwrap();
    let mut expected = seed(&store, status);
    let report = store
        .trim_event_payloads("2021-01-01", 0, false, Duration::from_secs(10))
        .unwrap();
    assert_eq!(report.rows, if successful { 10 } else { 1 });
    assert_eq!(
        report.bytes,
        if successful {
            6 * 9096 + 5000 + 4096 + 10_000
        } else {
            4096
        }
    );
    if successful {
        for value in &mut expected {
            for key in ["full", "provider_stdout"] {
                if let Some(output) = value.get(key).and_then(Value::as_str) {
                    if output.len() >= 4096 {
                        value[key] = json!({"trimmed": true, "bytes": output.len()});
                    }
                }
            }
        }
    } else {
        expected[8]["full"] = json!({"trimmed": true, "bytes": 4096});
    }
    assert_events(&store, expected);
    assert_eq!(
        store
            .trim_event_payloads("2021-01-01", 0, false, Duration::from_secs(10))
            .unwrap()
            .rows,
        0
    );
}

fn assert_events(store: &Store, expected: Vec<Value>) {
    let conn = store.db();
    let mut stmt = conn
        .prepare("SELECT metadata,detail FROM events ORDER BY id")
        .unwrap();
    let actual = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for ((raw, detail), value) in actual.iter().zip(expected) {
        assert_eq!(raw, &value.to_string());
        assert_eq!(detail, "preserved detail");
    }
    drop(stmt);
    drop(conn);
}

#[test]
fn done_tasks_trim_provider_stdout_and_all_full_payloads() {
    status_case("done", true);
}

#[test]
fn merged_tasks_trim_provider_stdout_and_all_full_payloads() {
    status_case("merged", true);
}

#[test]
fn failed_tasks_retain_provider_and_nonzero_exit_evidence() {
    status_case("failed", false);
}

#[test]
fn stopped_tasks_retain_provider_and_nonzero_exit_evidence() {
    status_case("stopped", false);
}

#[test]
fn skipped_tasks_retain_provider_and_nonzero_exit_evidence() {
    status_case("skipped", false);
}

#[test]
fn success_payloads_remain_when_task_becomes_failed_before_commit() {
    let store = Store::open_memory().unwrap();
    seed(&store, "done");
    let row = candidates(&store.db(), 0, "2021-01-01").unwrap().remove(0);
    let (updated, bytes) = replacement(&row, &Deadline::new(Duration::from_secs(10)))
        .unwrap()
        .unwrap();
    store
        .db()
        .execute("UPDATE tasks SET status='failed' WHERE id='old'", [])
        .unwrap();
    let mut report = TrimReport::default();
    apply_batch(
        &store.db(),
        "2021-01-01",
        &[(row.id, row.metadata.clone(), updated, bytes, row.status)],
        false,
        &mut report,
    )
    .unwrap();
    assert_eq!(report.rows, 0);
    let raw: String = store
        .db()
        .query_row("SELECT metadata FROM events WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(Some(raw), row.metadata);
}
