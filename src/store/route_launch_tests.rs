// Setup-only launch lookup equivalence, query-plan and schema-v3 regression tests.
// Uses real SQLite fixtures with repeated launches and unrelated event payloads.
use super::*;
use rusqlite::{Connection, params};

const LEGACY: &str = "SELECT DISTINCT task_id FROM events
    WHERE metadata LIKE '%api_route_launch%'
      AND json_extract(CASE WHEN json_valid(metadata) THEN metadata END,
                       '$.api_route_launch') = ?1";

fn fixture() -> Store {
    let store = Store::open_memory().expect("store");
    let conn = store.db();
    for id in ["a", "b", "c"] {
        conn.execute(
            "INSERT INTO tasks(id, agent, prompt, created_at) VALUES (?1, 'codex', '', 'now')",
            [id],
        )
        .expect("task");
    }
    for (task, kind, metadata) in [
        ("a", "setup", r#"{"api_route_launch":"one"}"#),
        ("a", "setup", r#"{"api_route_launch":"one"}"#),
        ("a", "setup", r#"{"api_route_launch":"two"}"#),
        ("b", "setup", r#"{"api_route_launch":"one"}"#),
        ("b", "setup", "malformed api_route_launch"),
        ("b", "setup", r#"{"other":"api_route_launch"}"#),
        ("c", "tool_call", r#"{"text":"api_route_launch one"}"#),
        ("c", "error", r#"{"message":"api_route_launch two"}"#),
        ("c", "tool_call", r#"{"input":{"api_route_launch":"one"}}"#),
        ("c", "error", "malformed api_route_launch"),
        ("c", "setup", r#"{"api_route_launch":null}"#),
        ("c", "setup", r#"{"api_route_launch":12}"#),
    ] {
        conn.execute(
            "INSERT INTO events(task_id,timestamp,event_type,detail,metadata)
             VALUES (?1,'now',?2,'',?3)",
            params![task, kind, metadata],
        )
        .expect("event");
    }
    drop(conn);
    store
}

fn legacy_ids(conn: &Connection, route: &str) -> Vec<String> {
    let mut ids = conn
        .prepare(LEGACY)
        .expect("prepare")
        .query_map([route], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("rows");
    ids.sort();
    ids
}

fn equivalent(store: &Store) {
    for route in ["one", "two", "missing", "12"] {
        let mut actual = store.route_launch_task_ids(route).expect("lookup");
        actual.sort();
        assert_eq!(actual, legacy_ids(&store.db(), route), "{route}");
    }
}

#[test]
fn setup_lookup_matches_legacy_with_repeated_launches_and_payload_text() {
    let store = fixture();
    equivalent(&store);
    let mut one = store.route_launch_task_ids("one").expect("one");
    one.sort();
    assert_eq!(one, ["a", "b"]);
    assert_eq!(store.route_launch_task_ids("two").expect("two"), ["a"]);
}

#[test]
fn lookup_seeks_setup_events_without_scanning_events() {
    let store = fixture();
    let conn = store.db();
    let plans = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {LOOKUP}"))
        .expect("plan")
        .query_map(["one"], |row| row.get::<_, String>(3))
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("plans");
    assert!(
        plans.iter().any(|plan| plan
            .contains("SEARCH e USING INDEX idx_events_task_kind (task_id=? AND event_type=?)")),
        "{plans:?}"
    );
    assert!(
        !plans
            .iter()
            .any(|plan| plan.starts_with("SCAN e ") || plan.starts_with("SCAN events")),
        "{plans:?}"
    );
}

#[test]
fn v2_migration_only_drops_duplicate_index_and_preserves_events() {
    let store = fixture();
    let conn = store.db();
    conn.execute_batch(
        "CREATE INDEX idx_events_task_id_id ON events(task_id, id); PRAGMA user_version = 2;",
    )
    .expect("v2");
    let before = conn.total_changes();
    let expected = legacy_ids(&conn, "one");
    crate::store::schema::ensure_current(&conn).expect("migrate");
    assert_eq!(conn.total_changes(), before);
    assert_eq!(legacy_ids(&conn, "one"), expected);
    let removed: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name = 'idx_events_task_id_id'",
            [],
            |row| row.get(0),
        )
        .expect("index");
    assert_eq!(removed, 0);
    let projection: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE '%api_route_launch%'",
            [],
            |row| row.get(0),
        )
        .expect("no projection");
    assert_eq!(projection, 0);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 3);
}

#[test]
fn lookup_reflects_event_deletion_and_metadata_updates() {
    let store = fixture();
    for sql in [
        "DELETE FROM events WHERE id = 1",
        "DELETE FROM events WHERE id = 2",
        "UPDATE events SET metadata = '{\"api_route_launch\":\"two\"}' WHERE id = 4",
        "UPDATE events SET metadata = '{\"other\":\"api_route_launch\"}' WHERE id = 3",
        "UPDATE events SET metadata = 'malformed api_route_launch' WHERE id = 4",
    ] {
        store.db().execute(sql, []).expect("mutate");
        equivalent(&store);
    }
}
